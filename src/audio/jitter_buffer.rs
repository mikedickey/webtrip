//! Lock-Free Jitter Buffer for Network Audio
//!
//! A thread-safe jitter buffer that can be accessed from both the main thread
//! (for pushing received network packets) and the audio worklet thread
//! (for popping audio samples for playback).
//!
//! Uses atomic operations for lock-free, wait-free synchronization.

use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
use wasm_bindgen::prelude::*;

/// Maximum number of packets the buffer can hold
const MAX_PACKETS: usize = 64;

/// Maximum samples per packet (supports up to 256 samples @ stereo)
const MAX_SAMPLES_PER_PACKET: usize = 512;

/// Default samples per packet (128 mono)
pub const DEFAULT_SAMPLES_PER_PACKET: usize = 128;

/// Buffer slot states
const SLOT_EMPTY: u32 = 0;
const SLOT_WRITING: u32 = 1;
const SLOT_READY: u32 = 2;
const SLOT_READING: u32 = 3;

/// Jitter buffer state
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum BufferState {
    /// Collecting packets before playback starts
    Buffering = 0,
    /// Normal playback
    Playing = 1,
    /// Buffer underrun, waiting for more data
    Underrun = 2,
}

/// A single packet slot in the buffer
struct PacketSlot {
    /// Slot state (empty, writing, ready, reading)
    state: AtomicU32,
    /// Sequence number of packet in this slot
    sequence: AtomicU64,
    /// Number of valid samples in this slot
    sample_count: AtomicU32,
    /// Audio samples (fixed size, only sample_count are valid)
    samples: [AtomicI32; MAX_SAMPLES_PER_PACKET],
}

impl PacketSlot {
    const fn new() -> Self {
        // Can't use a loop in const fn, so we use a macro-like approach
        // Actually in Rust we need to initialize each element
        // For simplicity, we'll initialize in a non-const way
        Self {
            state: AtomicU32::new(SLOT_EMPTY),
            sequence: AtomicU64::new(0),
            sample_count: AtomicU32::new(0),
            samples: unsafe { std::mem::zeroed() }, // Will be properly initialized at runtime
        }
    }

    fn init(&self) {
        self.state.store(SLOT_EMPTY, Ordering::SeqCst);
        self.sequence.store(0, Ordering::SeqCst);
        self.sample_count.store(0, Ordering::SeqCst);
        for sample in &self.samples {
            sample.store(0, Ordering::Relaxed);
        }
    }
}

/// Lock-free jitter buffer statistics
#[wasm_bindgen]
#[derive(Debug, Clone, Default)]
pub struct JitterBufferStats {
    pub packets_received: u64,
    pub packets_played: u64,
    pub packets_lost: u64,
    pub packets_late: u64,
    pub underruns: u64,
    pub current_depth: u32,
}

#[wasm_bindgen]
impl JitterBufferStats {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lock-free jitter buffer for network audio
/// 
/// Thread-safety:
/// - Main thread calls `push()` to add received packets
/// - Worklet thread calls `pop()` to get samples for playback
/// 
/// Both operations are lock-free and wait-free.
pub struct LockFreeJitterBuffer {
    /// Packet slots (circular buffer)
    slots: [PacketSlot; MAX_PACKETS],
    
    /// Next sequence number to play out
    read_sequence: AtomicU64,
    /// Highest sequence number received
    write_sequence: AtomicU64,
    
    /// Current buffer state
    state: AtomicU32,
    
    /// Target buffer depth before playback starts (in packets)
    target_depth: AtomicU32,
    /// Minimum depth before playback (in packets)
    min_depth: AtomicU32,
    
    /// Samples per packet (configurable)
    samples_per_packet: AtomicU32,
    
    /// Statistics (atomic for thread-safe access)
    packets_received: AtomicU64,
    packets_played: AtomicU64,
    packets_lost: AtomicU64,
    packets_late: AtomicU64,
    underruns: AtomicU64,
    
    /// Whether the buffer has been initialized with first packet
    initialized: AtomicU32,
    
    /// Previous samples for packet loss concealment
    prev_samples: [AtomicI32; MAX_SAMPLES_PER_PACKET],
}

impl LockFreeJitterBuffer {
    /// Create a new lock-free jitter buffer
    pub fn new() -> Self {
        let buffer = Self {
            slots: std::array::from_fn(|_| PacketSlot::new()),
            read_sequence: AtomicU64::new(0),
            write_sequence: AtomicU64::new(0),
            state: AtomicU32::new(BufferState::Buffering as u32),
            target_depth: AtomicU32::new(4),
            min_depth: AtomicU32::new(2),
            samples_per_packet: AtomicU32::new(DEFAULT_SAMPLES_PER_PACKET as u32),
            packets_received: AtomicU64::new(0),
            packets_played: AtomicU64::new(0),
            packets_lost: AtomicU64::new(0),
            packets_late: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            initialized: AtomicU32::new(0),
            prev_samples: std::array::from_fn(|_| AtomicI32::new(0)),
        };
        
        // Initialize all slots
        for slot in &buffer.slots {
            slot.init();
        }
        
        buffer
    }

    /// Configure the buffer parameters
    pub fn configure(&self, samples_per_packet: u32, target_depth: u32, min_depth: u32) {
        self.samples_per_packet.store(samples_per_packet.min(MAX_SAMPLES_PER_PACKET as u32), Ordering::SeqCst);
        self.target_depth.store(target_depth.min(MAX_PACKETS as u32 / 2), Ordering::SeqCst);
        self.min_depth.store(min_depth.min(target_depth), Ordering::SeqCst);
    }

    /// Get current buffer depth (packets available)
    pub fn depth(&self) -> u32 {
        let write = self.write_sequence.load(Ordering::Acquire);
        let read = self.read_sequence.load(Ordering::Acquire);
        if write >= read {
            (write - read) as u32
        } else {
            0
        }
    }

    /// Get current buffer state
    pub fn state(&self) -> BufferState {
        match self.state.load(Ordering::Acquire) {
            0 => BufferState::Buffering,
            1 => BufferState::Playing,
            _ => BufferState::Underrun,
        }
    }

    /// Get statistics
    pub fn stats(&self) -> JitterBufferStats {
        JitterBufferStats {
            packets_received: self.packets_received.load(Ordering::Relaxed),
            packets_played: self.packets_played.load(Ordering::Relaxed),
            packets_lost: self.packets_lost.load(Ordering::Relaxed),
            packets_late: self.packets_late.load(Ordering::Relaxed),
            underruns: self.underruns.load(Ordering::Relaxed),
            current_depth: self.depth(),
        }
    }

    /// Calculate slot index for a sequence number
    fn slot_index(sequence: u64) -> usize {
        (sequence as usize) % MAX_PACKETS
    }

    /// Convert f32 sample to i32 for atomic storage (fixed-point)
    fn f32_to_i32(sample: f32) -> i32 {
        (sample.clamp(-1.0, 1.0) * (i32::MAX as f32)) as i32
    }

    /// Convert i32 back to f32
    fn i32_to_f32(sample: i32) -> f32 {
        (sample as f32) / (i32::MAX as f32)
    }

    /// Push a packet into the buffer (called from main thread)
    /// Returns true if packet was accepted
    pub fn push(&self, sequence: u64, samples: &[f32]) -> bool {
        // First packet initialization
        if self.initialized.compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            self.read_sequence.store(sequence, Ordering::SeqCst);
            self.write_sequence.store(sequence, Ordering::SeqCst);
        }

        let read_seq = self.read_sequence.load(Ordering::Acquire);
        
        // Check if packet is too old (already played)
        if sequence < read_seq {
            self.packets_late.fetch_add(1, Ordering::Relaxed);
            return false;
        }

        // Check if packet is too far in the future (would overflow buffer)
        if sequence >= read_seq + MAX_PACKETS as u64 {
            // Stream reset - reinitialize
            self.reset();
            self.read_sequence.store(sequence, Ordering::SeqCst);
            self.initialized.store(1, Ordering::SeqCst);
        }

        let slot_idx = Self::slot_index(sequence);
        let slot = &self.slots[slot_idx];

        // Try to acquire the slot for writing
        if slot.state.compare_exchange(
            SLOT_EMPTY,
            SLOT_WRITING,
            Ordering::AcqRel,
            Ordering::Relaxed
        ).is_err() {
            // Slot is in use - skip this packet
            return false;
        }

        // Write samples to slot
        let sample_count = samples.len().min(MAX_SAMPLES_PER_PACKET);
        for (i, &sample) in samples.iter().take(sample_count).enumerate() {
            slot.samples[i].store(Self::f32_to_i32(sample), Ordering::Relaxed);
        }
        slot.sample_count.store(sample_count as u32, Ordering::Relaxed);
        slot.sequence.store(sequence, Ordering::Relaxed);

        // Mark slot as ready
        slot.state.store(SLOT_READY, Ordering::Release);

        // Update write sequence if this is a new high
        let mut current_write = self.write_sequence.load(Ordering::Acquire);
        while sequence > current_write {
            match self.write_sequence.compare_exchange_weak(
                current_write,
                sequence,
                Ordering::AcqRel,
                Ordering::Acquire
            ) {
                Ok(_) => break,
                Err(actual) => current_write = actual,
            }
        }

        self.packets_received.fetch_add(1, Ordering::Relaxed);

        // State transition: Buffering/Underrun -> Playing
        let current_state = self.state.load(Ordering::Acquire);
        if current_state != BufferState::Playing as u32 {
            let depth = self.depth();
            let min = self.min_depth.load(Ordering::Relaxed);
            if depth >= min {
                self.state.store(BufferState::Playing as u32, Ordering::Release);
            }
        }

        true
    }

    /// Pop samples for playback (called from worklet thread)
    /// Writes samples to the output buffer
    /// Returns true if valid samples were written, false if silence was output
    pub fn pop(&self, output: &mut [f32]) -> bool {
        let samples_per_packet = self.samples_per_packet.load(Ordering::Relaxed) as usize;
        let output_len = output.len().min(samples_per_packet);

        // Check buffer state
        let current_state = self.state.load(Ordering::Acquire);
        if current_state == BufferState::Buffering as u32 {
            output[..output_len].fill(0.0);
            return false;
        }

        let read_seq = self.read_sequence.load(Ordering::Acquire);
        let slot_idx = Self::slot_index(read_seq);
        let slot = &self.slots[slot_idx];

        // Try to acquire slot for reading
        let slot_state = slot.state.load(Ordering::Acquire);
        
        if slot_state == SLOT_READY {
            // Slot has data - try to acquire for reading
            if slot.state.compare_exchange(
                SLOT_READY,
                SLOT_READING,
                Ordering::AcqRel,
                Ordering::Relaxed
            ).is_ok() {
                // Read samples
                let sample_count = slot.sample_count.load(Ordering::Relaxed) as usize;
                let read_count = output_len.min(sample_count);
                
                for i in 0..read_count {
                    let sample = Self::i32_to_f32(slot.samples[i].load(Ordering::Relaxed));
                    output[i] = sample;
                    // Save for concealment
                    self.prev_samples[i].store(slot.samples[i].load(Ordering::Relaxed), Ordering::Relaxed);
                }
                
                // Zero-fill if output is larger than packet
                for sample in output.iter_mut().skip(read_count).take(output_len - read_count) {
                    *sample = 0.0;
                }

                // Mark slot as empty
                slot.state.store(SLOT_EMPTY, Ordering::Release);
                
                // Advance read sequence
                self.read_sequence.fetch_add(1, Ordering::AcqRel);
                self.packets_played.fetch_add(1, Ordering::Relaxed);

                // Check for underrun
                if self.depth() == 0 {
                    self.state.store(BufferState::Underrun as u32, Ordering::Release);
                    self.underruns.fetch_add(1, Ordering::Relaxed);
                }

                return true;
            }
        }

        // Packet missing or slot busy - use concealment
        self.packets_lost.fetch_add(1, Ordering::Relaxed);
        
        // Fade out previous samples for concealment
        for i in 0..output_len {
            let prev = Self::i32_to_f32(self.prev_samples[i].load(Ordering::Relaxed));
            let fade = 1.0 - (i as f32 / output_len as f32) * 0.5;
            output[i] = prev * fade;
        }

        // Advance read sequence even for missing packet
        self.read_sequence.fetch_add(1, Ordering::AcqRel);

        // Check for underrun
        if self.depth() == 0 {
            self.state.store(BufferState::Underrun as u32, Ordering::Release);
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }

        false
    }

    /// Reset the buffer
    pub fn reset(&self) {
        for slot in &self.slots {
            slot.state.store(SLOT_EMPTY, Ordering::SeqCst);
        }
        self.read_sequence.store(0, Ordering::SeqCst);
        self.write_sequence.store(0, Ordering::SeqCst);
        self.state.store(BufferState::Buffering as u32, Ordering::SeqCst);
        self.initialized.store(0, Ordering::SeqCst);
        
        for sample in &self.prev_samples {
            sample.store(0, Ordering::Relaxed);
        }
    }

    /// Check if buffer is ready for playback
    pub fn is_playing(&self) -> bool {
        self.state.load(Ordering::Acquire) == BufferState::Playing as u32
    }

    /// Get current latency in milliseconds (approximate)
    pub fn latency_ms(&self, sample_rate: u32) -> f32 {
        let depth = self.depth();
        let samples_per_packet = self.samples_per_packet.load(Ordering::Relaxed);
        let total_samples = depth * samples_per_packet;
        (total_samples as f32 / sample_rate as f32) * 1000.0
    }
}

impl Default for LockFreeJitterBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// Keep the old types for compatibility during transition
pub use BufferState as OldBufferState;

/// Legacy buffer config (for compatibility)
#[derive(Debug, Clone, Copy)]
pub struct JitterBufferConfig {
    pub target_depth: usize,
    pub max_capacity: usize,
    pub min_depth: usize,
    pub adaptive: bool,
    pub samples_per_packet: usize,
    pub channels: usize,
}

impl Default for JitterBufferConfig {
    fn default() -> Self {
        Self {
            target_depth: 4,
            max_capacity: 32,
            min_depth: 2,
            adaptive: true,
            samples_per_packet: 128,
            channels: 1,
        }
    }
}

/// Legacy JitterBuffer type alias
pub type JitterBuffer = LockFreeJitterBuffer;
