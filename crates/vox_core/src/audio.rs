//! Audio capture pipeline for the Vox dictation engine.
//!
//! Provides microphone capture via cpal, a lock-free SPSC ring buffer for
//! passing samples from the real-time audio callback to the processing thread,
//! and an FFT-based resampler for converting device-native sample rates to
//! the 16 kHz mono f32 format required by the downstream VAD and ASR stages.

pub mod capture;
pub mod resampler;
pub mod ring_buffer;

pub use capture::{list_input_devices, AudioCapture, AudioConfig, AudioDeviceInfo};
pub use resampler::AudioResampler;
pub use ring_buffer::AudioRingBuffer;
