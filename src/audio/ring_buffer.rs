//! Ring Buffer for Audio Data Transfer
//!
//! Provides a lock-free ring buffer for sending audio data from
//! the AudioWorklet (real-time thread) to the main thread (network I/O).
//!
//! Note: Receiving is handled directly by the LockFreeJitterBuffer,
//! which the worklet reads from directly - no intermediate buffer needed!

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use wasm_bindgen::prelude::*;

/// Size of the ring buffer in samples
const RING_BUFFER_SIZE: usize = 4096;

/// Shared ring buffer between worklet and main thread
/// 
/// Data flow:
///   Worklet: write() -> ring buffer -> Atomics.notify()
///   Main thread: Atomics.waitAsync() -> read() -> WebRTC send
#[wasm_bindgen]
pub struct RingBuffer {
    /// Ring buffer for audio going TO the network (local mic -> network)
    buffer: Vec<f32>,
    
    /// Write position (worklet writes, main thread reads)
    write_pos: AtomicU32,
    /// Read position (main thread updates after reading)
    read_pos: AtomicU32,
    
    /// Statistics
    overruns: AtomicU64,
    
    /// Flag indicating streaming is active
    streaming_active: AtomicU32,
    
    /// Total successful writes
    writes: AtomicU64,
    /// Total samples written
    samples_written: AtomicU64,
    
    /// Event-driven notification flag
    /// 0 = no data, 1 = data available
    /// Used with Atomics.waitAsync() for zero-CPU idle behavior
    has_data_flag: AtomicU32,
}

#[wasm_bindgen]
impl RingBuffer {
    /// Create a new ring buffer
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            buffer: vec![0.0; RING_BUFFER_SIZE],
            write_pos: AtomicU32::new(0),
            read_pos: AtomicU32::new(0),
            overruns: AtomicU64::new(0),
            streaming_active: AtomicU32::new(0),
            writes: AtomicU64::new(0),
            samples_written: AtomicU64::new(0),
            has_data_flag: AtomicU32::new(0),
        }
    }

    /// Enable/disable streaming
    pub fn set_streaming(&self, active: bool) {
        self.streaming_active.store(if active { 1 } else { 0 }, Ordering::SeqCst);
    }

    /// Check if streaming is active
    pub fn is_streaming(&self) -> bool {
        self.streaming_active.load(Ordering::SeqCst) != 0
    }

    /// Get overrun count
    pub fn overruns(&self) -> u64 {
        self.overruns.load(Ordering::Relaxed)
    }

    /// Get total write count
    pub fn writes(&self) -> u64 {
        self.writes.load(Ordering::Relaxed)
    }

    /// Get total samples written
    pub fn samples_written(&self) -> u64 {
        self.samples_written.load(Ordering::Relaxed)
    }

    /// Get available samples to read
    pub fn available(&self) -> u32 {
        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Acquire);
        write.wrapping_sub(read)
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        self.overruns.store(0, Ordering::Relaxed);
        self.writes.store(0, Ordering::Relaxed);
        self.samples_written.store(0, Ordering::Relaxed);
    }

    /// Get pointer to the has_data flag for JavaScript Atomics.waitAsync()
    /// 
    /// This enables event-driven wake-up instead of polling:
    /// - AudioWorklet sets flag to 1 and calls Atomics.notify()
    /// - Main thread waits with Atomics.waitAsync()
    /// - Zero CPU usage when idle!
    #[wasm_bindgen(js_name = getHasDataFlagPtr)]
    pub fn get_has_data_flag_ptr(&self) -> usize {
        &self.has_data_flag as *const AtomicU32 as usize
    }

    // === Called from AudioWorklet (real-time thread) ===

    /// Write audio samples to the buffer
    /// 
    /// After writing, sets the has_data_flag to signal waiting threads.
    /// AudioWorklet should call Atomics.notify() after this returns true.
    pub fn write(&mut self, samples: &[f32]) -> bool {
        if !self.is_streaming() {
            return false;
        }

        let write_pos = self.write_pos.load(Ordering::Acquire);
        let read_pos = self.read_pos.load(Ordering::Acquire);
        
        let used = write_pos.wrapping_sub(read_pos) as usize;
        let available = RING_BUFFER_SIZE - used;
        
        if samples.len() > available {
            self.overruns.fetch_add(1, Ordering::Relaxed);
            return false;
        }

        let start = (write_pos as usize) % RING_BUFFER_SIZE;
        for (i, &sample) in samples.iter().enumerate() {
            let idx = (start + i) % RING_BUFFER_SIZE;
            self.buffer[idx] = sample;
        }

        self.write_pos.store(
            write_pos.wrapping_add(samples.len() as u32),
            Ordering::Release
        );

        // Track statistics
        self.writes.fetch_add(1, Ordering::Relaxed);
        self.samples_written.fetch_add(samples.len() as u64, Ordering::Relaxed);

        // Signal that data is available (for event-driven wake-up)
        self.has_data_flag.store(1, Ordering::Release);

        true
    }

    // === Called from main thread (network I/O) ===

    /// Read audio samples from the buffer
    /// 
    /// Clears the has_data_flag when buffer becomes empty,
    /// allowing the main thread to sleep via Atomics.waitAsync().
    pub fn read(&mut self, output: &mut [f32]) -> bool {
        let write_pos = self.write_pos.load(Ordering::Acquire);
        let read_pos = self.read_pos.load(Ordering::Acquire);
        
        let available = write_pos.wrapping_sub(read_pos) as usize;
        
        if available < output.len() {
            output.fill(0.0);
            // No data available - clear flag so main thread can sleep
            self.has_data_flag.store(0, Ordering::Release);
            return false;
        }

        let start = (read_pos as usize) % RING_BUFFER_SIZE;
        for (i, sample) in output.iter_mut().enumerate() {
            let idx = (start + i) % RING_BUFFER_SIZE;
            *sample = self.buffer[idx];
        }

        self.read_pos.store(
            read_pos.wrapping_add(output.len() as u32),
            Ordering::Release
        );

        // Check if we've consumed all data
        let new_available = write_pos.wrapping_sub(read_pos.wrapping_add(output.len() as u32));
        if new_available == 0 {
            // Buffer is now empty - clear flag
            self.has_data_flag.store(0, Ordering::Release);
        }

        true
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos.store(0, Ordering::SeqCst);
        self.read_pos.store(0, Ordering::SeqCst);
    }
}

impl Default for RingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    /// Read the `has_data` flag through the same raw pointer JavaScript uses.
    fn has_data_flag(rb: &RingBuffer) -> u32 {
        let ptr = rb.get_has_data_flag_ptr() as *const AtomicU32;
        unsafe { (*ptr).load(Ordering::Acquire) }
    }

    /// Convenience: a streaming-enabled buffer ready for writes.
    fn streaming_buffer() -> RingBuffer {
        let rb = RingBuffer::new();
        rb.set_streaming(true);
        rb
    }

    #[test]
    fn push_pop_roundtrip_preserves_values() {
        let mut rb = streaming_buffer();
        let input = [0.1, -0.2, 0.3, -0.4, 0.5];

        assert!(rb.write(&input));
        assert_eq!(rb.available(), input.len() as u32);

        let mut output = [0.0; 5];
        assert!(rb.read(&mut output));
        assert_eq!(output, input);
        assert_eq!(rb.available(), 0);
        assert_eq!(rb.samples_written(), input.len() as u64);
        assert_eq!(rb.writes(), 1);
    }

    #[test]
    fn fill_to_capacity_then_pop_all() {
        let mut rb = streaming_buffer();
        let input: Vec<f32> = (0..RING_BUFFER_SIZE).map(|i| i as f32).collect();

        // Buffer holds exactly RING_BUFFER_SIZE samples.
        assert!(rb.write(&input));
        assert_eq!(rb.available(), RING_BUFFER_SIZE as u32);

        // One more sample must be rejected as an overrun.
        assert!(!rb.write(&[1.0]));
        assert_eq!(rb.overruns(), 1);

        let mut output = vec![0.0; RING_BUFFER_SIZE];
        assert!(rb.read(&mut output));
        assert_eq!(output, input);
        assert_eq!(rb.available(), 0);
    }

    #[test]
    fn wrap_around_across_boundary() {
        let mut rb = streaming_buffer();

        // Advance both positions close to the buffer boundary so the next
        // write/read pair straddles the end of the backing Vec.
        let pad = vec![0.0; RING_BUFFER_SIZE - 16];
        assert!(rb.write(&pad));
        let mut sink = vec![0.0; RING_BUFFER_SIZE - 16];
        assert!(rb.read(&mut sink));
        assert_eq!(rb.available(), 0);

        // This 64-sample chunk wraps from the tail of the Vec back to index 0.
        let input: Vec<f32> = (0..64).map(|i| (i as f32) * 0.01).collect();
        assert!(rb.write(&input));
        assert_eq!(rb.available(), 64);

        let mut output = vec![0.0; 64];
        assert!(rb.read(&mut output));
        assert_eq!(output, input);
    }

    #[test]
    fn available_invariants_under_interleaved_push_pop() {
        let mut rb = streaming_buffer();
        let mut expected_used: i64 = 0;

        for round in 0..1000 {
            let write_len = (round % 7) + 1;
            let chunk: Vec<f32> = (0..write_len).map(|i| (round + i) as f32).collect();
            if rb.write(&chunk) {
                expected_used += write_len as i64;
            }
            assert_eq!(rb.available() as i64, expected_used);
            // available-to-write is the complement of available-to-read.
            let writable = RING_BUFFER_SIZE as i64 - expected_used;
            assert!(writable >= 0 && writable <= RING_BUFFER_SIZE as i64);

            let read_len = (round % 5) + 1;
            if expected_used >= read_len as i64 {
                let mut out = vec![0.0; read_len];
                assert!(rb.read(&mut out));
                expected_used -= read_len as i64;
            }
            assert_eq!(rb.available() as i64, expected_used);
        }
    }

    #[test]
    fn has_data_flag_reflects_state() {
        let mut rb = streaming_buffer();
        assert_eq!(has_data_flag(&rb), 0);

        assert!(rb.write(&[1.0, 2.0, 3.0, 4.0]));
        assert_eq!(has_data_flag(&rb), 1, "flag set after write");

        // Partial read leaves data behind; flag stays set.
        let mut out = [0.0; 2];
        assert!(rb.read(&mut out));
        assert_eq!(has_data_flag(&rb), 1, "flag set while data remains");

        // Draining the rest clears the flag.
        assert!(rb.read(&mut out));
        assert_eq!(rb.available(), 0);
        assert_eq!(has_data_flag(&rb), 0, "flag cleared when empty");
    }

    #[test]
    fn write_requires_streaming() {
        let mut rb = RingBuffer::new();
        assert!(!rb.is_streaming());
        assert!(!rb.write(&[1.0, 2.0]), "write rejected while not streaming");
        assert_eq!(rb.available(), 0);

        rb.set_streaming(true);
        assert!(rb.write(&[1.0, 2.0]));
    }

    #[test]
    fn read_with_insufficient_data_zeros_output() {
        let mut rb = streaming_buffer();
        assert!(rb.write(&[7.0, 8.0]));

        // Asking for more than is available fails and zero-fills the output.
        let mut out = [9.0; 4];
        assert!(!rb.read(&mut out));
        assert_eq!(out, [0.0; 4]);
        // Underlying data is untouched and still readable.
        assert_eq!(rb.available(), 2);
        assert_eq!(has_data_flag(&rb), 0, "flag cleared when not enough data");
    }

    #[test]
    fn clear_resets_positions() {
        let mut rb = streaming_buffer();
        assert!(rb.write(&[1.0, 2.0, 3.0]));
        assert_eq!(rb.available(), 3);

        rb.clear();
        assert_eq!(rb.available(), 0);

        // Buffer is usable again after clearing.
        assert!(rb.write(&[4.0, 5.0]));
        let mut out = [0.0; 2];
        assert!(rb.read(&mut out));
        assert_eq!(out, [4.0, 5.0]);
    }

    #[test]
    fn reset_stats_zeros_counters() {
        let mut rb = streaming_buffer();
        let full: Vec<f32> = vec![0.0; RING_BUFFER_SIZE];
        assert!(rb.write(&full));
        assert!(!rb.write(&[1.0])); // force an overrun
        assert!(rb.writes() >= 1);
        assert!(rb.samples_written() >= 1);
        assert_eq!(rb.overruns(), 1);

        rb.reset_stats();
        assert_eq!(rb.writes(), 0);
        assert_eq!(rb.samples_written(), 0);
        assert_eq!(rb.overruns(), 0);
    }

    #[test]
    fn concurrent_producer_consumer_preserves_order() {
        // Share one buffer across two threads via a raw pointer. This mirrors
        // real usage where the buffer lives in a SharedArrayBuffer and the
        // producer (worklet) and consumer (main thread) touch disjoint regions.
        struct Shared(*mut RingBuffer);
        unsafe impl Send for Shared {}

        let mut rb = streaming_buffer();
        let shared = Shared(&mut rb as *mut RingBuffer);

        const CHUNK: usize = 128;
        const TOTAL: usize = CHUNK * 2000;

        let producer = thread::spawn(move || {
            let rb = unsafe { &mut *shared.0 };
            let mut next: usize = 0;
            while next < TOTAL {
                let chunk: Vec<f32> = (next..next + CHUNK).map(|i| i as f32).collect();
                // Spin until there is room (consumer is draining concurrently).
                while !rb.write(&chunk) {
                    std::hint::spin_loop();
                }
                next += CHUNK;
            }
        });

        let mut received: usize = 0;
        let mut out = vec![0.0; CHUNK];
        while received < TOTAL {
            if rb.read(&mut out) {
                for (i, &v) in out.iter().enumerate() {
                    assert_eq!(v, (received + i) as f32, "out-of-order sample");
                }
                received += CHUNK;
            } else {
                std::hint::spin_loop();
            }
        }

        producer.join().unwrap();
        assert_eq!(received, TOTAL);
        assert_eq!(rb.available(), 0, "no samples left behind");
    }
}

