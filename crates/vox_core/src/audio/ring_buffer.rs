use ringbuf::traits::Split;
use ringbuf::{HeapCons, HeapProd, HeapRb};

/// Factory for creating SPSC (single-producer, single-consumer) lock-free ring
/// buffers used to pass audio samples from the real-time capture callback to
/// the processing thread without locks or allocations.
pub struct AudioRingBuffer;

impl AudioRingBuffer {
    /// Create a new ring buffer and split it into a producer/consumer pair.
    ///
    /// The producer is moved into the audio capture callback, and the consumer
    /// is read from on the processing thread. `capacity` is in f32 samples.
    #[allow(clippy::new_ret_no_self)] // factory method intentionally returns split pair, not Self
    pub fn new(capacity: usize) -> (HeapProd<f32>, HeapCons<f32>) {
        let rb = HeapRb::<f32>::new(capacity);
        rb.split()
    }

    /// Compute a ring buffer capacity (in samples) that holds approximately
    /// 2 seconds of audio at the given sample rate, rounded up to a power of
    /// two for alignment efficiency.
    pub fn capacity_for_rate(sample_rate: u32) -> usize {
        (sample_rate * 2).next_power_of_two() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ringbuf::traits::{Consumer, Observer, Producer};

    #[test]
    fn test_ring_buffer_basic() {
        let (mut producer, mut consumer) = AudioRingBuffer::new(1024);

        let input: Vec<f32> = (0..512).map(|i| i as f32).collect();
        let written = producer.push_slice(&input);
        assert_eq!(written, 512);
        assert_eq!(consumer.occupied_len(), 512);

        let mut output = vec![0.0f32; 512];
        let read = consumer.pop_slice(&mut output);
        assert_eq!(read, 512);
        assert_eq!(input, output);
    }

    #[test]
    fn test_ring_buffer_overflow() {
        let (mut producer, mut consumer) = AudioRingBuffer::new(64);

        let input: Vec<f32> = (0..128).map(|i| i as f32).collect();
        let written = producer.push_slice(&input);
        assert_eq!(written, 64);

        let mut output = vec![0.0f32; 64];
        let read = consumer.pop_slice(&mut output);
        assert_eq!(read, 64);

        let expected: Vec<f32> = (0..64).map(|i| i as f32).collect();
        assert_eq!(output, expected);
    }

    #[test]
    fn test_ring_buffer_concurrent() {
        let (mut producer, mut consumer) = AudioRingBuffer::new(1024);
        let total = 10_000usize;

        let producer_thread = std::thread::spawn(move || {
            let mut sent = 0usize;
            while sent < total {
                let batch_end = (sent + 64).min(total);
                let batch: Vec<f32> = (sent..batch_end).map(|i| i as f32).collect();
                let written = producer.push_slice(&batch);
                sent += written;
                if written == 0 {
                    std::thread::yield_now();
                }
            }
        });

        let consumer_thread = std::thread::spawn(move || {
            let mut received = Vec::with_capacity(total);
            let mut buf = vec![0.0f32; 64];
            while received.len() < total {
                let read = consumer.pop_slice(&mut buf);
                if read > 0 {
                    received.extend_from_slice(&buf[..read]);
                } else {
                    std::thread::yield_now();
                }
            }
            received
        });

        producer_thread.join().expect("producer panicked");
        let received = consumer_thread.join().expect("consumer panicked");

        assert_eq!(received.len(), total);
        let expected: Vec<f32> = (0..total).map(|i| i as f32).collect();
        assert_eq!(received, expected);
    }
}
