use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::StreamConfig;
use ringbuf::traits::Producer;
use ringbuf::HeapCons;

use super::ring_buffer::AudioRingBuffer;

/// Configuration for the audio capture pipeline.
///
/// The defaults target 16 kHz mono, which is the native input format for
/// Whisper ASR. When [`device_name`](AudioConfig::device_name) is `None`,
/// the system's default input device is selected.
pub struct AudioConfig {
    /// Target sample rate in Hz (default: 16 000).
    pub sample_rate: u32,
    /// Number of channels (default: 1, mono).
    pub channels: u16,
    /// Name of the input device to open, or `None` for the system default.
    pub device_name: Option<String>,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 16_000,
            channels: 1,
            device_name: None,
        }
    }
}

/// Metadata for a single audio input device, returned by [`list_input_devices`].
pub struct AudioDeviceInfo {
    /// Human-readable device name as reported by the OS.
    pub name: String,
    /// Whether this device is the system default input.
    pub is_default: bool,
}

/// Enumerate all available audio input devices on the current host.
///
/// Returns one [`AudioDeviceInfo`] per device, with exactly one marked as the
/// system default. Used by the settings UI to populate the device selector.
pub fn list_input_devices() -> Result<Vec<AudioDeviceInfo>> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.description().ok())
        .map(|desc| desc.name().to_string());

    let mut devices = Vec::new();
    for device in host
        .input_devices()
        .context("failed to enumerate input devices")?
    {
        let desc = match device.description() {
            Ok(d) => d,
            Err(_) => continue,
        };
        let name = desc.name().to_string();
        let is_default = default_name.as_deref() == Some(&name);
        devices.push(AudioDeviceInfo { name, is_default });
    }
    Ok(devices)
}

/// Captures audio from an input device into a lock-free ring buffer.
///
/// The capture callback runs on a real-time OS thread managed by cpal and
/// must never allocate, lock, or block. Samples are written into the ring
/// buffer producer; the processing thread reads from the consumer side via
/// [`consumer()`](AudioCapture::consumer).
pub struct AudioCapture {
    stream: Option<cpal::Stream>,
    device: cpal::Device,
    stream_config: StreamConfig,
    device_name: String,
    native_sample_rate: u32,
    error_flag: Arc<AtomicBool>,
    /// Real-time RMS amplitude stored as f32 bits in an AtomicU32.
    /// Updated by the cpal audio callback (real-time safe: pure arithmetic,
    /// relaxed atomic store, no allocations or locks).
    rms_atomic: Arc<std::sync::atomic::AtomicU32>,
    consumer: HeapCons<f32>,
    // Kept alive so the producer can be moved into the next stream's callback
    producer: Option<ringbuf::HeapProd<f32>>,
    channels: u16,
    consumer_taken: bool,
}

impl AudioCapture {
    /// Open an input device and prepare a ring buffer for capture.
    ///
    /// Selects the device specified by [`AudioConfig::device_name`], or the
    /// system default if `None`. Does not start the audio stream — call
    /// [`start()`](AudioCapture::start) to begin capturing.
    pub fn new(config: &AudioConfig) -> Result<Self> {
        let host = cpal::default_host();

        let device = match &config.device_name {
            Some(name) => {
                let mut found = None;
                for d in host.input_devices().context("failed to list input devices")? {
                    if let Ok(desc) = d.description()
                        && desc.name() == name
                    {
                        found = Some(d);
                        break;
                    }
                }
                found.with_context(|| format!("input device not found: {name}"))?
            }
            None => host
                .default_input_device()
                .context("no default input device available")?,
        };

        let device_desc = device
            .description()
            .context("failed to get device description")?;
        let device_name = device_desc.name().to_string();

        let supported_config = device
            .default_input_config()
            .context("failed to get default input config")?;
        let native_sample_rate = supported_config.sample_rate();
        let channels = supported_config.channels();

        let stream_config: StreamConfig = supported_config.into();

        let capacity = AudioRingBuffer::capacity_for_rate(native_sample_rate);
        let (producer, consumer) = AudioRingBuffer::new(capacity);

        Ok(Self {
            stream: None,
            device,
            stream_config,
            device_name,
            native_sample_rate,
            error_flag: Arc::new(AtomicBool::new(false)),
            rms_atomic: Arc::new(std::sync::atomic::AtomicU32::new(0)),
            consumer,
            producer: Some(producer),
            channels,
            consumer_taken: false,
        })
    }

    /// Build and start the cpal input stream.
    ///
    /// Moves the ring buffer producer into the real-time audio callback.
    /// Returns an error if the producer has already been consumed by a
    /// prior call without an intervening [`stop()`](AudioCapture::stop).
    pub fn start(&mut self) -> Result<()> {
        let mut producer = self
            .producer
            .take()
            .context("producer already consumed by a running stream")?;

        let error_flag = self.error_flag.clone();
        let channels = self.channels as usize;
        let rms_sink = self.rms_atomic.clone();

        let data_callback = move |data: &[f32], _: &cpal::InputCallbackInfo| {
            // Compute RMS on incoming audio (real-time safe: pure arithmetic,
            // relaxed atomic, no allocations, no locks)
            if !data.is_empty() {
                let sum_sq: f32 = if channels <= 1 {
                    data.iter().map(|s| s * s).sum()
                } else {
                    data.iter().step_by(channels).map(|s| s * s).sum()
                };
                let count = if channels <= 1 {
                    data.len()
                } else {
                    data.len() / channels
                };
                let rms = (sum_sq / count as f32).sqrt();
                rms_sink.store(rms.to_bits(), Ordering::Relaxed);
            }

            if channels <= 1 {
                producer.push_slice(data);
            } else {
                // Extract first channel from interleaved data
                for sample in data.iter().step_by(channels) {
                    let _ = producer.try_push(*sample);
                }
            }
        };

        let err_flag = error_flag.clone();
        let error_callback = move |err: cpal::StreamError| {
            if matches!(
                err,
                cpal::StreamError::DeviceNotAvailable | cpal::StreamError::StreamInvalidated
            ) {
                err_flag.store(true, Ordering::Release);
            }
        };

        let stream = self
            .device
            .build_input_stream::<f32, _, _>(&self.stream_config, data_callback, error_callback, None)
            .context("failed to build input stream")?;

        stream.play().context("failed to start audio stream")?;
        self.stream = Some(stream);
        Ok(())
    }

    /// Stop the audio stream and release the OS capture resources.
    pub fn stop(&mut self) {
        self.stream = None;
    }

    /// The human-readable name of the currently selected input device.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// The device's native sample rate in Hz as reported by cpal.
    pub fn native_sample_rate(&self) -> u32 {
        self.native_sample_rate
    }

    /// Mutable reference to the ring buffer consumer for reading captured
    /// samples on the processing thread.
    pub fn consumer(&mut self) -> &mut HeapCons<f32> {
        &mut self.consumer
    }

    /// Take ownership of the ring buffer consumer for cross-thread use.
    ///
    /// Splits the consumer from the producer so it can be moved to the VAD
    /// processing thread. Returns `None` if the consumer has already been
    /// taken by a prior call. After taking, the `consumer()` method will
    /// panic — use this method only when the consumer needs to be moved
    /// to another thread (e.g., Pipeline::start).
    pub fn take_consumer(&mut self) -> Option<HeapCons<f32>> {
        if self.consumer_taken {
            return None;
        }
        self.consumer_taken = true;
        use ringbuf::traits::Split;
        let (_, dummy_consumer) = ringbuf::HeapRb::<f32>::new(1).split();
        Some(std::mem::replace(&mut self.consumer, dummy_consumer))
    }

    /// Returns a shared handle to the real-time RMS amplitude value.
    ///
    /// The atomic stores f32 bits via `to_bits()`. Read with
    /// `f32::from_bits(arc.load(Ordering::Relaxed))`. Updated on every
    /// cpal audio callback (~5-10ms intervals depending on buffer size).
    pub fn rms_atomic(&self) -> Arc<std::sync::atomic::AtomicU32> {
        self.rms_atomic.clone()
    }

    /// Returns `true` if the device has been disconnected or the stream
    /// invalidated since capture started.
    pub fn is_disconnected(&self) -> bool {
        self.error_flag.load(Ordering::Acquire)
    }

    /// Check if the audio device is healthy and the stream is valid.
    ///
    /// Returns `true` if no error has been flagged (device connected,
    /// stream not invalidated). This is a non-blocking, lock-free check.
    pub fn health_check(&self) -> bool {
        !self.error_flag.load(Ordering::Acquire)
    }

    /// Returns a shared handle to the error flag for cross-thread monitoring.
    ///
    /// The orchestrator can poll this flag to detect device disconnection
    /// without holding a reference to AudioCapture (which is NOT Send).
    pub fn error_flag(&self) -> Arc<AtomicBool> {
        self.error_flag.clone()
    }

    /// Switch to the system's default audio input device.
    ///
    /// Convenience wrapper around `switch_device(None)`. Used by the
    /// audio recovery handler after device disconnection.
    pub fn switch_to_default(&mut self) -> Result<()> {
        self.switch_device(None)
    }

    /// Attempt to reconnect to a specific device, falling back to default.
    ///
    /// Tries the named device first. If that fails (device not found or
    /// unavailable), falls back to the system default device.
    pub fn reconnect(&mut self, device_name: Option<&str>) -> Result<()> {
        if let Some(name) = device_name {
            match self.switch_device(Some(name)) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    tracing::warn!(
                        device = name,
                        error = %err,
                        "failed to reconnect to named device, falling back to default"
                    );
                }
            }
        }
        self.switch_to_default()
    }

    /// Stop the current stream, switch to a different input device, and
    /// restart capture.
    ///
    /// Pass `None` to switch to the system default. The ring buffer is
    /// rebuilt and the consumer is replaced, so any previously held
    /// consumer reference becomes invalid. If the new device has a
    /// different native sample rate, the caller should recreate the
    /// [`AudioResampler`](super::AudioResampler) — check with
    /// [`needs_resampler_update()`](AudioCapture::needs_resampler_update).
    pub fn switch_device(&mut self, device_name: Option<&str>) -> Result<()> {
        self.stop();

        // Drain any remaining samples from the consumer
        let mut drain_buf = vec![0.0f32; 1024];
        loop {
            use ringbuf::traits::Consumer;
            if self.consumer.pop_slice(&mut drain_buf) == 0 {
                break;
            }
        }

        let host = cpal::default_host();
        let device = match device_name {
            Some(name) => {
                let mut found = None;
                for d in host.input_devices().context("failed to list input devices")? {
                    if let Ok(desc) = d.description()
                        && desc.name() == name
                    {
                        found = Some(d);
                        break;
                    }
                }
                found.with_context(|| format!("input device not found: {name}"))?
            }
            None => host
                .default_input_device()
                .context("no default input device available")?,
        };

        let device_desc = device
            .description()
            .context("failed to get device description")?;
        self.device_name = device_desc.name().to_string();

        let supported_config = device
            .default_input_config()
            .context("failed to get default input config")?;
        let new_rate = supported_config.sample_rate();
        self.channels = supported_config.channels();
        self.stream_config = supported_config.into();

        // Always rebuild ring buffer — the old producer was consumed by the previous stream's callback
        let capacity = AudioRingBuffer::capacity_for_rate(new_rate);
        let (producer, consumer) = AudioRingBuffer::new(capacity);
        self.consumer = consumer;
        self.producer = Some(producer);
        self.consumer_taken = false;

        self.native_sample_rate = new_rate;
        self.device = device;
        self.error_flag.store(false, Ordering::Release);

        self.start()
    }

    /// Returns `true` if the device's native sample rate differs from the
    /// rate the current resampler was built for, indicating the caller
    /// should recreate the [`AudioResampler`](super::AudioResampler).
    pub fn needs_resampler_update(&self, current_resampler_rate: u32) -> bool {
        self.native_sample_rate != current_resampler_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::traits::Consumer;

    #[test]
    fn test_capture_to_buffer() {
        let config = AudioConfig::default();
        let mut capture = AudioCapture::new(&config).expect("failed to create capture");
        capture.start().expect("failed to start capture");

        std::thread::sleep(std::time::Duration::from_millis(200));

        let mut buf = vec![0.0f32; 4096];
        let read = capture.consumer().pop_slice(&mut buf);
        assert!(read > 0, "expected samples in buffer, got 0");

        capture.stop();
    }

    #[test]
    fn test_capture_stop_clean() {
        let handle = std::thread::spawn(|| {
            let config = AudioConfig::default();
            let mut capture = AudioCapture::new(&config).expect("failed to create capture");

            capture.start().expect("failed to start capture (1st)");
            std::thread::sleep(std::time::Duration::from_millis(100));
            capture.stop();

            // Recreate to get a fresh producer
            let mut capture = AudioCapture::new(&config).expect("failed to create capture (2nd)");
            capture.start().expect("failed to start capture (2nd)");
            std::thread::sleep(std::time::Duration::from_millis(100));
            capture.stop();
        });

        let timeout = std::time::Duration::from_secs(5);
        let start = std::time::Instant::now();
        loop {
            if handle.is_finished() {
                handle.join().expect("capture thread panicked");
                return;
            }
            if start.elapsed() > timeout {
                panic!("test_capture_stop_clean timed out — possible hang");
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    #[test]
    fn test_audio_recovery_default_device() {
        let config = AudioConfig::default();
        let mut capture = AudioCapture::new(&config).expect("create capture");
        capture.start().expect("start capture");

        // Health check should pass on a healthy device
        assert!(capture.health_check(), "healthy device should pass health check");

        // Switch to default should succeed (already on default)
        capture.switch_to_default().expect("switch_to_default failed");
        assert!(capture.health_check(), "should be healthy after switch_to_default");

        // Reconnect with None should also succeed
        capture.reconnect(None).expect("reconnect(None) failed");
        assert!(capture.health_check(), "should be healthy after reconnect(None)");

        // Reconnect with a bogus device name should fall back to default
        capture
            .reconnect(Some("NonexistentDevice12345"))
            .expect("reconnect with fallback should succeed");
        assert!(
            capture.health_check(),
            "should be healthy after reconnect fallback to default"
        );

        capture.stop();
    }

    #[test]
    fn test_device_enumeration() {
        let devices = list_input_devices().expect("failed to enumerate devices");
        assert!(!devices.is_empty(), "expected at least one input device");

        let default_count = devices.iter().filter(|d| d.is_default).count();
        assert_eq!(
            default_count, 1,
            "expected exactly one default device, found {default_count}"
        );
    }
}
