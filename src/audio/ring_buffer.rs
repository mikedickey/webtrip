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
///   Worklet: write() -> ring buffer
///   Main thread: read() -> WebRTC send
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

    /// Get available samples to read
    pub fn available(&self) -> u32 {
        let write = self.write_pos.load(Ordering::Acquire);
        let read = self.read_pos.load(Ordering::Acquire);
        write.wrapping_sub(read)
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        self.overruns.store(0, Ordering::Relaxed);
    }

    // === Called from AudioWorklet (real-time thread) ===

    /// Write audio samples to the buffer
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

        true
    }

    // === Called from main thread (network I/O) ===

    /// Read audio samples from the buffer
    pub fn read(&mut self, output: &mut [f32]) -> bool {
        let write_pos = self.write_pos.load(Ordering::Acquire);
        let read_pos = self.read_pos.load(Ordering::Acquire);
        
        let available = write_pos.wrapping_sub(read_pos) as usize;
        
        if available < output.len() {
            output.fill(0.0);
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


