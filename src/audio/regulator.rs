//! Regulator: Adaptive Jitter Buffer with Burg Algorithm PLC
//!
//! A Rust reimplementation of the JackTrip Regulator, which uses the Burg
//! algorithm for autoregressive prediction to perform packet loss concealment.
//!
//! References:
//! - http://www.emptyloop.com/technotes/A%20tutorial%20on%20Burg's%20method,%20algorithm%20and%20recursion.pdf
//! - https://metacpan.org/source/SYP/Algorithm-Burg-0.001/README
//!
//! Original C++ implementation by Chris Chafe, CCRMA Stanford University.
//!
//! # Sequence space vs. slot space
//!
//! Two things wrap around in this file, at different periods, and they are not
//! interchangeable:
//!
//! - **Sequence space** is the full `u16` the wire protocol carries (65536
//!   values; see `protocol::AudioPacket`). It answers *identity and ordering*:
//!   which packet is this, and is it newer than the one before it
//!   ([`seq_is_newer`])?
//! - **Slot space** is [`NUM_SLOTS`] entries deep ([`slot_index`]). It answers
//!   *storage*: where does this packet's audio live, and is it still there?
//!
//! Sequence space is 16x larger, so the seam between them has two rules:
//!
//! - Every `NUM_SLOTS`th sequence number aliases onto the same slot, so a slot
//!   holds only the most recent packet congruent to its index. Reading one is
//!   meaningful only for a sequence number already known to be within
//!   `NUM_SLOTS` of the write pointer.
//! - A span wider than the ring is representable in sequence space but not
//!   retrievable from slot space. When the read pointer falls that far behind
//!   the write pointer it has lost contact with the stream permanently — no
//!   amount of waiting brings those packets back — and the only recovery is to
//!   drop it and resynchronize. That is the single place the seam is handled;
//!   see [`ConsumerState::find_best_packet`].
//!
//! Upstream JackTrip has no seam: it folds `seq_num %= NumSlots` on arrival,
//! collapsing the two spaces into one. That makes an unretrievable span
//! unrepresentable, but it also shrinks the window over which [`seq_is_newer`]
//! can tell a forward jump from a reordered straggler, from ~32768 packets
//! (~87 s at fpp=128/48 kHz) down to 2048 (~5.5 s) — which is why upstream then
//! needs a wall-clock escape hatch to unstick a pinned write pointer
//! (`gUdpWaitTimeout`, jacktrip/jacktrip#1500). We keep the full `u16` and its
//! unambiguous ordering, and handle the one case folding would have prevented
//! explicitly instead.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering};

/// Sentinel value for "no sequence number" (None)
const SEQ_NONE: i32 = -1;

/// Bits of an `f64` stored in an [`AtomicU64`] for cross-thread stats publication.
#[inline]
fn f64_to_bits(v: f64) -> u64 {
    v.to_bits()
}

#[inline]
fn f64_from_bits(v: u64) -> f64 {
    f64::from_bits(v)
}

// ============================================================================
// Constants
// ============================================================================

/// Number of past packets to use for prediction history
const HIST: usize = 2;
/// Default FPP used for calibrating burg window
const HIST_FPP: usize = 128;
/// Depth of the packet ring, and with it the horizon over which a packet stays
/// retrievable: ~10.9 s at fpp=128/48 kHz. Not the sequence-number space — see
/// the module header.
const NUM_SLOTS: usize = 4096;
/// Maximum channels a single stream may claim. A local mirror of
/// `protocol::MAX_CHANNELS`, not an import of it: this module deliberately has
/// no `crate::` imports (see the module header) so a native host can reuse it
/// against a different wire format with its own limit.
const MAX_CHANNELS: usize = 8;
/// Maximum auto tolerance in milliseconds
const AUTO_MAX_MS: f64 = 250.0;
/// Duration before auto mode kicks in
const AUTO_INIT_DURATION_MS: f64 = 3000.0;
/// Scale factor for initial tolerance during init phase
const AUTO_INIT_VAL_FACTOR: f64 = 0.5;
/// Window divisor for faster auto tracking
const WINDOW_DIVISOR: usize = 8;
/// Acceptable glitch rate before increasing headroom (0.6%)
const AUTO_HEADROOM_GLITCH_TOLERANCE: f64 = 0.006;
/// Rolling window of time for auto tolerance adjustment (seconds)
const AUTO_HISTORY_WINDOW: f64 = 60.0;
/// EWMA smoothing factor for auto tolerance
const AUTO_SMOOTHING_FACTOR: f64 = 1.0 / (WINDOW_DIVISOR as f64 * AUTO_HISTORY_WINDOW);

// ============================================================================
// The sequence/slot seam
// ============================================================================

/// Map a sequence number onto its slot in the packet ring.
///
/// Lossy by construction: every `NUM_SLOTS`th sequence number lands on the same
/// slot, so the packet found there is the one asked for only when that sequence
/// number is within `NUM_SLOTS` of the write pointer. Callers establish that
/// first — see the module header.
#[inline]
fn slot_index(seq: u16) -> usize {
    seq as usize % NUM_SLOTS
}

/// Is `seq` newer than `reference` in sequence space?
///
/// Serial-number arithmetic: a forward distance of less than half the space
/// reads as newer, anything else as a reordered straggler. Unambiguous for gaps
/// up to 32767 packets (~87 s at fpp=128/48 kHz), which is far beyond any
/// outage the ring itself can survive.
#[inline]
fn seq_is_newer(seq: u16, reference: u16) -> bool {
    seq.wrapping_sub(reference) < u16::MAX / 2
}

/// How many sequence numbers the inclusive span `first..=last` covers.
///
/// Zero means `last` is exactly one before `first` — nothing new. Any larger
/// backwards relationship reads as a near-full-space span, which is precisely
/// how a read pointer that has lost contact with the stream is detected.
#[inline]
fn seq_span(first: u16, last: u16) -> usize {
    last.wrapping_sub(first).wrapping_add(1) as usize
}

// ============================================================================
// Burg Algorithm
// ============================================================================

/// Burg algorithm for autoregressive (AR) parameter estimation and prediction.
///
/// Uses Burg's method to estimate AR coefficients from a time series, then
/// uses those coefficients to predict future samples (extrapolation).
pub struct BurgAlgorithm {
    /// AR model order (m = N - 1)
    order: usize,
    /// Input size (N)
    input_size: usize,
    /// Working buffer for AR coefficients
    ak: Vec<f32>,
    /// Reset state for AR coefficients
    ak_reset: Vec<f32>,
    /// Forward prediction error
    f: Vec<f32>,
    /// Backward prediction error
    b: Vec<f32>,
}

impl BurgAlgorithm {
    /// Create a new Burg algorithm instance for the given input size.
    ///
    /// # Arguments
    /// * `size` - The size of the input signal (typically packets_in_past * fpp)
    pub fn new(size: usize) -> Self {
        let order = size.saturating_sub(1);

        let mut ak_reset = vec![0.0f32; size];
        if !ak_reset.is_empty() {
            ak_reset[0] = 1.0;
        }

        Self {
            order,
            input_size: size,
            ak: vec![0.0; size],
            ak_reset,
            f: vec![0.0; size],
            b: vec![0.0; size],
        }
    }

    /// Train the model by estimating AR coefficients from input signal.
    ///
    /// Uses Burg's recursive method to compute optimal AR coefficients.
    ///
    /// # Arguments
    /// * `x` - Input time series (training data)
    /// * `coeffs` - Output buffer for AR coefficients (length = order)
    pub fn train(&mut self, x: &[f32], coeffs: &mut [f32]) {
        let size = x.len().min(self.input_size);
        let n = size.saturating_sub(1);
        let m = n.min(self.order);

        // Initialize Ak
        self.ak.copy_from_slice(&self.ak_reset);

        // Initialize forward and backward prediction errors
        for i in 0..size {
            self.f[i] = x[i];
            self.b[i] = x[i];
        }

        // Initialize Dk (denominator for reflection coefficient)
        let mut dk: f32 = self.f[..=n]
            .iter()
            .map(|&v| 2.00002 * v * v) // Slightly more than 2.0 for damping
            .sum();
        dk -= self.f[0] * self.f[0] + self.b[n] * self.b[n];

        // Burg recursion
        for k in 0..m {
            // Compute reflection coefficient (mu)
            let mut mu: f32 = 0.0;
            for i in 0..=(n - k - 1) {
                mu += self.f[i + k + 1] * self.b[i];
            }

            // Avoid division by zero
            if dk.abs() < f32::EPSILON {
                dk = f32::EPSILON;
            }
            mu *= -2.0 / dk;

            // Update Ak (Levinson-Durbin update)
            for i in 0..=((k + 1) / 2) {
                let t1 = self.ak[i] + mu * self.ak[k + 1 - i];
                let t2 = self.ak[k + 1 - i] + mu * self.ak[i];
                self.ak[i] = t1;
                self.ak[k + 1 - i] = t2;
            }

            // Update forward and backward errors
            for i in 0..=(n - k - 1) {
                let t1 = self.f[i + k + 1] + mu * self.b[i];
                let t2 = self.b[i] + mu * self.f[i + k + 1];
                self.f[i + k + 1] = t1;
                self.b[i] = t2;
            }

            // Update Dk
            dk = (1.0 - mu * mu) * dk
                - self.f[k + 1] * self.f[k + 1]
                - self.b[n - k - 1] * self.b[n - k - 1];
        }

        // Output coefficients (skip Ak[0] which is always 1.0)
        let coeff_count = coeffs.len().min(m);
        coeffs[..coeff_count].copy_from_slice(&self.ak[1..=coeff_count]);
    }

    /// Predict future samples using trained AR coefficients.
    ///
    /// # Arguments
    /// * `coeffs` - AR coefficients from training
    /// * `tail` - Buffer containing past samples and space for predictions.
    ///            First `order` samples are input, remaining are filled with predictions.
    /// * `predict_count` - Total size of tail buffer (past + future)
    pub fn predict(&self, coeffs: &[f32], tail: &mut [f32], predict_count: usize) {
        let m = self.order.min(coeffs.len());
        let count = predict_count.min(tail.len());

        for i in m..count {
            let mut prediction = 0.0f32;
            for j in 0..m {
                prediction -= coeffs[j] * tail[i - 1 - j];
            }
            tail[i] = prediction;
        }
    }
}

// ============================================================================
// Channel State
// ============================================================================

/// Per-channel state for audio processing and prediction.
struct ChannelState {
    /// Temporary buffer for incoming packet samples
    tmp_buf: Vec<f32>,
    /// Ring buffer of past packets
    packet_ring: Vec<Vec<f32>>,
    /// Ring buffer write pointer
    ring_wptr: usize,
    /// Ring buffer size
    ring_size: usize,
    /// Real samples from current packet
    real_now_packet: Vec<f32>,
    /// Predicted samples for current packet (when missing)
    predicted_now_packet: Vec<f32>,
    /// Output samples (after blending)
    output_now_packet: Vec<f32>,
    /// Future predicted packet (for crossfade)
    future_predicted_packet: Vec<f32>,
    /// History of predicted packets
    predicted_past: Vec<Vec<f32>>,
    /// Buffer for prediction (past + future)
    prediction: Vec<f32>,
    /// AR coefficients
    coeffs: Vec<f32>,
    /// Pre-allocated buffer for Burg training (avoids allocation in audio path)
    train_data: Vec<f32>,
}

impl ChannelState {
    /// Create new channel state for the given parameters.
    ///
    /// # Arguments
    /// * `fpp` - Frames (samples) per packet
    /// * `up_to_now` - History size in samples (packets_in_past * fpp)
    /// * `packets_in_past` - Number of past packets to track
    fn new(fpp: usize, up_to_now: usize, packets_in_past: usize) -> Self {
        let tail_size = up_to_now + fpp * 2;
        let coeffs_size = up_to_now.saturating_sub(1);

        Self {
            tmp_buf: vec![0.0; fpp],
            packet_ring: vec![vec![0.0; fpp]; packets_in_past],
            ring_wptr: packets_in_past / 2,
            ring_size: packets_in_past,
            real_now_packet: vec![0.0; fpp],
            predicted_now_packet: vec![0.0; fpp],
            output_now_packet: vec![0.0; fpp],
            future_predicted_packet: vec![0.0; fpp],
            predicted_past: vec![vec![0.0; fpp]; packets_in_past],
            prediction: vec![0.0; tail_size],
            coeffs: vec![0.0; coeffs_size],
            train_data: vec![0.0; up_to_now],
        }
    }

    /// Return to the freshly-constructed state without reallocating.
    fn clear(&mut self) {
        self.tmp_buf.fill(0.0);
        self.real_now_packet.fill(0.0);
        self.predicted_now_packet.fill(0.0);
        self.output_now_packet.fill(0.0);
        self.future_predicted_packet.fill(0.0);
        for ring_pkt in &mut self.packet_ring {
            ring_pkt.fill(0.0);
        }
        for pred_pkt in &mut self.predicted_past {
            pred_pkt.fill(0.0);
        }
        self.prediction.fill(0.0);
        self.coeffs.fill(0.0);
        self.train_data.fill(0.0);
        self.ring_wptr = self.ring_size / 2;
    }

    /// Push current tmp_buf to ring buffer
    fn ring_buffer_push(&mut self) {
        self.packet_ring[self.ring_wptr].copy_from_slice(&self.tmp_buf);
        self.ring_wptr = (self.ring_wptr + 1) % self.ring_size;
    }

    /// Pull a past packet from ring buffer
    ///
    /// # Arguments
    /// * `past` - How many packets in the past (1 = most recent)
    fn ring_buffer_pull(&mut self, past: usize) {
        let idx = (self.ring_wptr + self.ring_size - past) % self.ring_size;
        self.tmp_buf.copy_from_slice(&self.packet_ring[idx]);
    }
}

// ============================================================================
// Timing Statistics (simplified from StdDev)
// ============================================================================

/// Rolling statistics for timing measurements used in auto-adaptive mode.
struct TimingStats {
    /// Window size for statistics
    window: usize,
    /// Sample counter
    count: usize,
    /// Accumulated values
    accumulator: f64,
    /// Current minimum
    min: f64,
    /// Current maximum
    max: f64,
    /// Data buffer
    data: Vec<f64>,
    /// Last computed mean
    last_mean: f64,
    /// Last computed standard deviation
    last_std_dev: f64,
    /// Last computed max
    last_max: f64,
    /// Long term standard deviation (EWMA)
    long_term_std_dev: f64,
    /// Long term max (EWMA)
    long_term_max: f64,
    /// Long term accumulator for std dev
    long_term_std_dev_acc: f64,
    /// Long term accumulator for max
    long_term_max_acc: f64,
    /// Long term sample counter
    long_term_count: usize,
    /// Last timestamp
    last_time: f64,
    /// PLC overruns (skipped packets)
    pub overruns: u64,
    /// PLC underruns (missing packets)
    pub underruns: u64,
}

impl TimingStats {
    fn new(sample_rate: u32, fpp: usize) -> Self {
        let window = ((sample_rate as usize) / fpp) / WINDOW_DIVISOR;
        Self {
            window: window.max(1),
            count: 0,
            accumulator: 0.0,
            min: f64::MAX,
            max: f64::MIN,
            data: vec![0.0; window.max(1)],
            last_mean: 0.0,
            last_std_dev: 0.0,
            last_max: 0.0,
            long_term_std_dev: 0.0,
            long_term_max: 0.0,
            long_term_std_dev_acc: 0.0,
            long_term_max_acc: 0.0,
            long_term_count: 0,
            last_time: 0.0,
            overruns: 0,
            underruns: 0,
        }
    }

    /// Add a timing measurement.
    /// Returns true if statistics window is complete.
    fn tick(&mut self, elapsed_ms: f64, now: f64) -> bool {
        self.last_time = now;

        // Discard extreme measurements
        if elapsed_ms > 10000.0 || elapsed_ms <= 0.0 {
            return false;
        }

        self.data[self.count] = elapsed_ms;
        self.accumulator += elapsed_ms;
        self.min = self.min.min(elapsed_ms);
        self.max = self.max.max(elapsed_ms);
        self.count += 1;

        if self.count < self.window {
            return false;
        }

        // Window complete - compute statistics
        let mean = self.accumulator / self.window as f64;
        let variance: f64 = self.data[..self.window]
            .iter()
            .map(|&x| (x - mean).powi(2))
            .sum::<f64>()
            / self.window as f64;
        let std_dev = variance.sqrt();

        // Update long term stats
        if self.long_term_count <= 3 {
            // Initialize
            self.long_term_max = self.max;
            self.long_term_max_acc = self.max;
            self.long_term_std_dev = std_dev;
            self.long_term_std_dev_acc = std_dev;
        } else {
            self.long_term_std_dev_acc += std_dev;
            self.long_term_max_acc += self.max;

            let threshold = WINDOW_DIVISOR * AUTO_HISTORY_WINDOW as usize;
            if self.long_term_count <= threshold {
                // Simple average during startup
                let n = (self.long_term_count - 3) as f64;
                self.long_term_std_dev = self.long_term_std_dev_acc / n;
                self.long_term_max = self.long_term_max_acc / n;
            } else {
                // EWMA after startup
                self.long_term_std_dev = Self::ewma(self.long_term_std_dev, std_dev);
                self.long_term_max = Self::ewma(self.long_term_max, self.max);
            }
        }

        self.last_mean = mean;
        self.last_std_dev = std_dev;
        self.last_max = self.max;
        self.long_term_count += 1;

        // Reset for next window
        self.count = 0;
        self.accumulator = 0.0;
        self.min = f64::MAX;
        self.max = f64::MIN;

        true
    }

    /// Calculate auto tolerance value
    fn calc_auto(&self) -> f64 {
        if self.long_term_std_dev == 0.0 || self.long_term_max == 0.0 {
            return AUTO_MAX_MS;
        }
        self.long_term_std_dev + self.long_term_max.min(AUTO_MAX_MS)
    }

    /// Exponentially weighted moving average
    fn ewma(avg: f64, current: f64) -> f64 {
        avg + AUTO_SMOOTHING_FACTOR * (current - avg)
    }

    /// Reset rolling and long-term statistics state.
    fn reset(&mut self) {
        self.count = 0;
        self.accumulator = 0.0;
        self.min = f64::MAX;
        self.max = f64::MIN;
        self.data.fill(0.0);
        self.last_mean = 0.0;
        self.last_std_dev = 0.0;
        self.last_max = 0.0;
        self.long_term_std_dev = 0.0;
        self.long_term_max = 0.0;
        self.long_term_std_dev_acc = 0.0;
        self.long_term_max_acc = 0.0;
        self.long_term_count = 0;
        self.last_time = 0.0;
        self.overruns = 0;
        self.underruns = 0;
    }
}

// ============================================================================
// Packet Slot
// ============================================================================

/// The producer-written contents of a [`PacketSlot`].
struct SlotPayload {
    /// Arrival timestamp in milliseconds
    timestamp: f64,
    /// One packet of audio (interleaved channels), `fpp * channels` long
    data: Vec<f32>,
}

/// A slot for storing an incoming packet.
///
/// A JackTrip connection fixes frames-per-packet and channel count when it is
/// established, so every packet belonging to the stream carries exactly
/// `fpp * channels` samples. `data` is sized to that once per stream, and is
/// always full: a packet of any other size is rejected before it reaches a
/// slot ([`Regulator::push`]). That leaves no partially-filled state to track
/// and no allocation on the audio path.
///
/// # Publication
///
/// `seq` is the per-slot publication stamp. The producer writes the payload,
/// then stores `seq` with [`Ordering::Release`]. The consumer reaches the
/// payload only through [`read`](Self::read), which loads `seq` with
/// [`Ordering::Acquire`] first. This is independent of the stream-wide
/// [`Shared::last_seq_in`] pointer: a reordered straggler must still publish
/// its slot without advancing that pointer.
///
/// The payload sits in its own [`UnsafeCell`] so the producer's `&mut` to one
/// slot never overlaps a reference to any other slot or to the rest of the
/// regulator.
struct PacketSlot {
    payload: UnsafeCell<SlotPayload>,
    /// Sequence number occupying this slot, or [`SEQ_NONE`] if empty.
    /// Because every `NUM_SLOTS`th sequence number aliases onto the same slot
    /// (see the module header), `data` alone doesn't say *which* packet it
    /// belongs to — this is the identity check the read side verifies before
    /// trusting it. Atomic so a straggler's store synchronizes-with the
    /// consumer without needing to advance `last_seq_in`.
    seq: AtomicI32,
}

impl PacketSlot {
    fn new(samples_per_packet: usize) -> Self {
        Self {
            payload: UnsafeCell::new(SlotPayload {
                timestamp: 0.0,
                data: vec![0.0; samples_per_packet],
            }),
            seq: AtomicI32::new(SEQ_NONE),
        }
    }

    /// Exclusive access to the payload, for writing it.
    ///
    /// # Safety
    ///
    /// Only the producer may call this (or [`Regulator::reset`] once both
    /// sides are quiesced), and no reference from an earlier call may still be
    /// live. The consumer must not be reading this slot — see "Slot lapping"
    /// on [`Regulator`] for the one case the stamps do not rule out.
    #[allow(clippy::mut_from_ref)]
    unsafe fn payload_mut(&self) -> &mut SlotPayload {
        &mut *self.payload.get()
    }

    /// The payload, if this slot currently holds `seq` (Acquire).
    fn read(&self, seq: u16) -> Option<&SlotPayload> {
        if !self.holds(seq) {
            return None;
        }
        // SAFETY: the Acquire in `holds` synchronizes-with the producer's
        // Release in `publish_seq`, so every payload write for `seq`
        // happened-before this read, and the producer does not write this
        // slot again until the ring laps it (see "Slot lapping" on
        // `Regulator`).
        Some(unsafe { &*self.payload.get() })
    }

    /// Publish `seq` as the identity of the data currently in this slot.
    /// Must be called **after** the payload is written.
    #[inline]
    fn publish_seq(&self, seq: u16) {
        self.seq.store(seq as i32, Ordering::Release);
    }

    /// Clear the identity stamp (reset / invalidate).
    #[inline]
    fn clear_seq(&self) {
        self.seq.store(SEQ_NONE, Ordering::Release);
    }

    /// Whether this slot currently holds `seq` (Acquire).
    #[inline]
    fn holds(&self, seq: u16) -> bool {
        self.seq.load(Ordering::Acquire) == seq as i32
    }
}

// ============================================================================
// Regulator
// ============================================================================

/// Statistics for the Regulator jitter buffer
#[derive(Debug, Clone, Default)]
pub struct RegulatorStats {
    /// Current tolerance in milliseconds
    pub tolerance_ms: f64,
    /// Current headroom in milliseconds
    pub headroom_ms: f64,
    /// Maximum latency observed
    pub max_latency_ms: f64,
    /// Number of glitches (underruns + overruns)
    pub glitches: u64,
    /// Packets received
    pub packets_received: u64,
    /// Packets played
    pub packets_played: u64,
    /// Last packet sequence number received (u16, wraps at 65535)
    pub last_seq_received: u16,
    /// Packets rejected by `push` (bad channel count, size mismatch, or a
    /// channel count that changed mid-stream) since the last `reset()`.
    pub packets_rejected: u64,
}

/// Regulator: Adaptive jitter buffer with Burg-based packet loss concealment.
///
/// This is a Rust reimplementation of the JackTrip Regulator, designed for
/// real-time audio streaming over the network. Key features:
///
/// - **Adaptive buffering**: Automatically adjusts tolerance based on network jitter
/// - **Burg algorithm PLC**: Uses autoregressive prediction to conceal packet loss
/// - **Smooth crossfading**: Blends between predicted and real audio to hide glitches
///
/// # Concurrency (SPSC)
///
/// One network-side producer calls [`push`](Self::push); one audio-side consumer
/// calls [`pop`](Self::pop). Main-thread [`stats`](Self::stats) /
/// [`depth`](Self::depth) / [`latency_ms`](Self::latency_ms) read only atomics.
/// Every method takes `&self`, and state is split by owner so that no thread
/// ever creates a `&mut` overlapping memory another thread can reach:
///
/// - [`Shared`] is only ever borrowed `&`. It holds the configuration that is
///   fixed at construction, the atomics (the `last_seq_in` / `last_seq_out`
///   pointers, the stream's adopted channel count and start time, and the
///   published stats snapshot), and the slot ring.
/// - Each slot's payload sits behind its own [`UnsafeCell`]. The producer
///   writes it before a `Release` store of the slot's `seq` stamp; the consumer
///   reads it only after an `Acquire` load of that stamp shows the `seq` it
///   expects ([`PacketSlot::read`]).
/// - [`ConsumerState`] — tolerance, timing stats, Burg/channel state — sits
///   behind one [`UnsafeCell`] that only `pop` (and a quiesced `reset`)
///   borrows mutably. The producer has no non-atomic state of its own, so it
///   never needs a `&mut` to anything but the one slot payload it is writing.
///
/// Calling `push` from two threads at once, or `pop` from two threads at once,
/// breaks this and is undefined behavior.
///
/// # Slot lapping
///
/// The slot stamps do not cover one case: the producer overwriting the slot
/// the consumer is in the middle of copying. That needs the producer to be a
/// whole ring ([`NUM_SLOTS`] packets, ~10.9 s at fpp=128/48 kHz) ahead of a
/// read the consumer is performing right now, or the transport to deliver the
/// same sequence number twice. Neither is possible with today's transports and
/// callback timing, but nothing here enforces it: if it happens, the copy
/// races the producer's write.
///
/// See WEB-53. Do not weaken this to `&mut self` + concurrent `SharedPtr::as_mut`.
pub struct Regulator {
    shared: Shared,
    consumer: UnsafeCell<ConsumerState>,
}

// SAFETY: `Regulator` is shared across the network producer, audio consumer, and
// main-thread stats readers. The only non-`Sync` parts are the `UnsafeCell`s:
// `consumer` is borrowed mutably only by the single consumer (or a quiesced
// `reset`), and each slot payload only per the stamp protocol on `PacketSlot`.
// See the struct docs.
unsafe impl Sync for Regulator {}

/// Regulator state readable from every thread. Only ever borrowed `&`.
struct Shared {
    // Configuration, fixed at construction
    fpp: usize,
    sample_rate: u32,

    /// Packet storage (circular buffer by sequence number)
    slots: Box<[PacketSlot]>,

    // Sequence tracking
    /// Last sequence number received (SEQ_NONE = not initialized).
    /// Stream-wide publication pointer between push (writer) and pop (reader).
    last_seq_in: AtomicI32,
    /// Last sequence number played. Written only by the consumer; atomic so
    /// main-thread `depth()` can read it.
    last_seq_out: AtomicI32,

    // Stream shape, written by the producer on a stream's first packet before
    // the `Release` of `last_seq_in`, so a consumer that has `Acquire`d a
    // non-`SEQ_NONE` value sees the current stream's values. Relaxed atomics
    // rather than plain fields because the consumer's pre-stream path and
    // `channels()` may read them while that first packet is being adopted.
    /// Channel count adopted from the peer's first packet.
    num_channels: AtomicUsize,
    /// Wall-clock time of the stream's first packet (f64 bits).
    start_time_bits: AtomicU64,

    /// Packets rejected by push. Written only by the producer.
    packets_rejected: AtomicU64,

    // ---- Published snapshot for main-thread stats()/depth()/latency_ms() ----
    // Written by the consumer from its working fields; read from any thread.
    // Atomic so concurrent reads are not a data race (no torn f64/u64), but
    // Relaxed: these are display-only values that guard no other memory, so a
    // momentarily stale or mutually inconsistent snapshot is fine. Do not read
    // payload/config based on one of these — `last_seq_in` / `PacketSlot::seq`
    // carry the Acquire/Release handshake.
    pub_tolerance_bits: AtomicU64,
    pub_headroom_bits: AtomicU64,
    pub_max_latency_bits: AtomicU64,
    pub_glitches: AtomicU64,
    pub_packets: AtomicU64,
}

/// State owned by the consumer (`pop`). Reached only through
/// [`Regulator::consumer_mut`].
struct ConsumerState {
    // Configuration, fixed at construction
    packets_in_past: usize,
    up_to_now: usize,
    beyond_now: usize,
    auto_mode: bool,
    auto_headroom: f64,
    fade_up: Vec<f32>,
    fade_down: Vec<f32>,

    // Burg algorithm state
    burg: BurgAlgorithm,

    /// Per-channel state, preallocated for the widest stream a peer may
    /// adopt so adoption never touches it. Only the first `num_channels`
    /// entries are in use.
    channels: Vec<ChannelState>,
    /// Channel count this consumer is rendering: `Shared::num_channels`, as
    /// loaded after the `Acquire` of a started stream.
    num_channels: usize,

    last_stashed: Option<(u16, usize)>,

    // Timing
    last_pop_time_ms: f64,
    push_stats: TimingStats,
    pull_stats: TimingStats,

    // Auto-adaptive tolerance
    tolerance_ms: f64,
    current_headroom: f64,
    skip_auto_headroom: bool,
    auto_headroom_start_time: f64,

    // Statistics
    packet_count: u64,
    plc_packet_count: u64,
    last_glitches: u64,
    stats_glitches: u64,
    last_max_latency: f64,
    stats_max_latency: f64,

    // State
    last_was_glitch: bool,
}

enum PacketDecision {
    Packet { seq: u16, slot_idx: usize },
    ConcealSkippedPacket { skipped: u64 },
}

/// Outcome of [`Regulator::push`].
///
/// A JackTrip stream fixes frames-per-packet locally but *adopts* its channel
/// count from the peer's first packet ([`Shared::adopt_channel_count`]).
/// Every variant other than `Stored` means the packet was dropped before it
/// could reach a slot or move the write pointer.
#[must_use]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushOutcome {
    /// The packet was stored (including the first-packet adoption case).
    Stored,
    /// `channels` was `0` or greater than [`MAX_CHANNELS`].
    UnsupportedChannelCount { got: usize },
    /// `channels` differs from the count already adopted from an earlier
    /// packet in this stream. Unlike the first-packet case, this is rejected
    /// rather than re-adopted: a mid-stream change is either a peer bug or a
    /// renegotiation this regulator was never told about, and silently
    /// resizing state to match would corrupt whatever a concurrent `pop` is
    /// reading (see [`Shared::adopt_channel_count`]'s concurrency note).
    ChannelCountChanged { adopted: usize, got: usize },
    /// `samples.len()` was not exactly `channels * fpp`.
    WrongPacketSize { expected: usize, got: usize },
}


impl Regulator {
    /// Get current time in milliseconds.
    fn now_ms() -> f64 {
        #[cfg(target_arch = "wasm32")]
        {
            js_sys::Date::now()
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use std::time::{SystemTime, UNIX_EPOCH};
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs_f64()
                * 1000.0
        }
    }

    /// Exclusive access to the consumer's state.
    ///
    /// # Safety
    ///
    /// Only the single consumer thread may call this (or [`reset`](Self::reset)
    /// once both sides are quiesced), and no reference from an earlier call
    /// may still be live.
    #[allow(clippy::mut_from_ref)]
    unsafe fn consumer_mut(&self) -> &mut ConsumerState {
        &mut *self.consumer.get()
    }

    /// Create a new Regulator with default parameters.
    pub fn new() -> Self {
        Self::with_params(1, 128, 48000, -1.0)
    }

    /// Create a new Regulator with specific parameters.
    pub fn with_params(channels: usize, fpp: usize, sample_rate: u32, tolerance_ms: f64) -> Self {
        let samples_per_packet = fpp * channels;

        let packets_in_past = if fpp < HIST_FPP {
            HIST * (HIST_FPP / fpp)
        } else if fpp > HIST_FPP * 2 {
            1
        } else {
            HIST
        };

        let up_to_now = packets_in_past * fpp;
        let beyond_now = (packets_in_past + 1) * fpp;

        let (auto_mode, auto_headroom, initial_tolerance) = if tolerance_ms <= 0.0 {
            let headroom = if tolerance_ms == -500.0 {
                -1.0
            } else {
                tolerance_ms.abs()
            };
            (true, headroom, fpp as f64 * AUTO_INIT_VAL_FACTOR)
        } else {
            (false, tolerance_ms, tolerance_ms)
        };

        let fade_up: Vec<f32> = (0..fpp).map(|i| i as f32 / fpp as f32).collect();
        let fade_down: Vec<f32> = fade_up.iter().map(|&x| 1.0 - x).collect();

        let channel_states: Vec<ChannelState> = (0..channels.max(MAX_CHANNELS))
            .map(|_| ChannelState::new(fpp, up_to_now, packets_in_past))
            .collect();

        let slots = (0..NUM_SLOTS)
            .map(|_| PacketSlot::new(samples_per_packet))
            .collect();

        let current_headroom = if auto_headroom < 0.0 { 0.0 } else { auto_headroom };
        let shared = Shared {
            fpp,
            sample_rate,
            slots,
            last_seq_in: AtomicI32::new(SEQ_NONE),
            last_seq_out: AtomicI32::new(SEQ_NONE),
            num_channels: AtomicUsize::new(channels),
            start_time_bits: AtomicU64::new(f64_to_bits(0.0)),
            packets_rejected: AtomicU64::new(0),
            pub_tolerance_bits: AtomicU64::new(f64_to_bits(initial_tolerance)),
            pub_headroom_bits: AtomicU64::new(f64_to_bits(current_headroom)),
            pub_max_latency_bits: AtomicU64::new(0),
            pub_glitches: AtomicU64::new(0),
            pub_packets: AtomicU64::new(0),
        };
        let consumer = ConsumerState {
            packets_in_past,
            up_to_now,
            beyond_now,
            auto_mode,
            auto_headroom,
            fade_up,
            fade_down,
            burg: BurgAlgorithm::new(up_to_now),
            channels: channel_states,
            num_channels: channels,
            last_stashed: None,
            last_pop_time_ms: 0.0,
            push_stats: TimingStats::new(sample_rate, fpp),
            pull_stats: TimingStats::new(sample_rate, fpp),
            tolerance_ms: initial_tolerance,
            current_headroom,
            skip_auto_headroom: true,
            auto_headroom_start_time: 6000.0,
            packet_count: 0,
            plc_packet_count: 0,
            last_glitches: 0,
            stats_glitches: 0,
            last_max_latency: 0.0,
            stats_max_latency: 0.0,
            last_was_glitch: false,
        };
        Self {
            shared,
            consumer: UnsafeCell::new(consumer),
        }
    }

    /// Push a received packet into the buffer.
    ///
    /// # Arguments
    /// * `sequence` - Packet sequence number (u16 wraps at 65535)
    /// * `channels` - The channel count this packet claims to carry, from the
    ///   wire header
    /// * `samples` - Interleaved audio samples
    ///
    /// # Returns
    /// See [`PushOutcome`].
    pub fn push(&self, sequence: u16, channels: usize, samples: &[f32]) -> PushOutcome {
        self.push_internal(sequence, channels, samples, Self::now_ms())
    }

    fn push_internal(&self, sequence: u16, channels: usize, samples: &[f32], now_ms: f64) -> PushOutcome {
        self.shared.push_internal(sequence, channels, samples, now_ms)
    }

    /// Pop samples for playback.
    ///
    /// Returns `true` if real packet data was output, `false` if concealment
    /// was used. After returning, [`channels`](Self::channels) reflects the
    /// peer width synchronized by this pop's Acquire (WEB-53 Bug 3).
    pub fn pop(&self, output: &mut [f32]) -> bool {
        self.pop_internal(output, Self::now_ms())
    }

    fn pop_internal(&self, output: &mut [f32], now_ms: f64) -> bool {
        // SAFETY: `pop` is only called from the single consumer thread
        // (struct docs), and the borrow ends before this returns.
        let consumer = unsafe { self.consumer_mut() };
        consumer.pop_internal(&self.shared, output, now_ms)
    }

    /// Get current statistics (lock-free; safe from any thread).
    pub fn stats(&self) -> RegulatorStats {
        self.shared.stats()
    }

    /// Reset the regulator state.
    ///
    /// The caller must have quiesced both the producer and the consumer first:
    /// this rewrites slot payloads and the consumer's state in place.
    /// `WebTripSession::disconnect` does that by awaiting transport teardown
    /// and `AudioContext.suspend()` before calling it (WEB-53 Bug 7).
    pub fn reset(&self) {
        self.shared.reset();
        // SAFETY: the consumer is quiesced (see above), so this is the only
        // borrow of its state.
        let consumer = unsafe { self.consumer_mut() };
        consumer.reset(&self.shared);
    }

    /// Get the current tolerance in milliseconds.
    pub fn tolerance_ms(&self) -> f64 {
        f64_from_bits(self.shared.pub_tolerance_bits.load(Ordering::Relaxed))
    }

    /// Get the frames per packet.
    pub fn fpp(&self) -> usize {
        self.shared.fpp
    }

    /// Get the number of channels (peer-adopted).
    ///
    /// After a [`pop`](Self::pop) that rendered a started stream, this matches
    /// the width that pop used — the Acquire inside pop orders the adoption.
    pub fn channels(&self) -> usize {
        self.shared.channels()
    }

    /// Check if the regulator has been initialized (received first packet).
    pub fn is_initialized(&self) -> bool {
        self.shared.last_seq_in.load(Ordering::Acquire) != SEQ_NONE
    }

    /// Get current buffer depth (number of packets buffered).
    pub fn depth(&self) -> u32 {
        self.shared.depth()
    }

    /// Get approximate latency in milliseconds.
    pub fn latency_ms(&self) -> f32 {
        self.shared.latency_ms()
    }
}

impl Default for Regulator {
    fn default() -> Self {
        Self::new()
    }
}


impl Shared {
    fn channels(&self) -> usize {
        self.num_channels.load(Ordering::Relaxed)
    }

    fn start_time_ms(&self) -> f64 {
        f64_from_bits(self.start_time_bits.load(Ordering::Relaxed))
    }

    fn fpp_duration_ms(&self) -> f64 {
        1000.0 * self.fpp as f64 / self.sample_rate as f64
    }

    fn reject(&self) {
        self.packets_rejected.fetch_add(1, Ordering::Relaxed);
    }

    fn push_internal(&self, seq_num: u16, channels: usize, samples: &[f32], now_ms: f64) -> PushOutcome {
        if channels == 0 || channels > MAX_CHANNELS {
            self.reject();
            return PushOutcome::UnsupportedChannelCount { got: channels };
        }

        // Frames-per-packet is fixed locally for the life of this regulator;
        // channel count is instead adopted from the peer's first packet
        // (below), which is why this checks `channels * self.fpp` rather than
        // against the adopted count — that may not reflect `channels` yet on
        // the very packet that is about to establish it. Either way,
        // every packet on the wire must carry exactly that many samples:
        // anything else did not come from this stream's configuration, and
        // there is no honest way to place it in the timeline. A short packet
        // would play as a dropout in the middle of good audio, and a long one
        // would be silently truncated. Reject it before it can occupy a slot
        // or move the write pointer.
        let expected = channels * self.fpp;
        if samples.len() != expected {
            self.reject();
            return PushOutcome::WrongPacketSize { expected, got: samples.len() };
        }

        let current = self.last_seq_in.load(Ordering::Acquire);
        let adopted = self.channels();

        if current == SEQ_NONE {
            // First packet of a stream: adopt its channel count rather than
            // rejecting a mismatch against whatever this regulator happened
            // to be constructed with.
            if channels != adopted {
                self.adopt_channel_count(channels);
            }
            self.start_time_bits.store(f64_to_bits(now_ms), Ordering::Relaxed);
        } else if channels != adopted {
            // A later packet disagreeing with the already-adopted count is a
            // peer bug or an unsignaled renegotiation, not something to
            // re-adopt into — see `PushOutcome::ChannelCountChanged`.
            self.reject();
            return PushOutcome::ChannelCountChanged { adopted, got: channels };
        }

        let relative_now = now_ms - self.start_time_ms();

        // Store the audio unconditionally, even for a packet the ordering check
        // below rejects: it is still the freshest thing that belongs in this
        // slot, and a straggler the read side has not passed yet is playable.
        let slot = &self.slots[slot_index(seq_num)];
        {
            // SAFETY: this is the producer, and the borrow ends before the
            // Release below publishes the slot.
            let payload = unsafe { slot.payload_mut() };
            payload.timestamp = relative_now;
            payload.data.copy_from_slice(samples);
        }
        // Publish last: Release-store of seq synchronizes-with the consumer's
        // Acquire identity check. Independent of the conditional last_seq_in
        // advance below — stragglers must still publish their slots.
        slot.publish_seq(seq_num);

        // Advance the write pointer only for packets that are actually newer;
        // reordered stragglers leave it where it is. Sequence numbers on the
        // wire only ever count up, and both transports we run over authenticate
        // their payloads (WebTransport over QUIC, WebRTC data channels over
        // SCTP/DTLS), so a sequence number far enough ahead to read as a
        // straggler and pin this pointer cannot reach here. Upstream JackTrip
        // runs over bare UDP with an optional 16-bit checksum and does need a
        // wall-clock escape hatch for exactly that (jacktrip/jacktrip#1500).
        let should_update = current == SEQ_NONE || seq_is_newer(seq_num, current as u16);
        if should_update {
            self.last_seq_in.store(seq_num as i32, Ordering::Release);
        }

        PushOutcome::Stored
    }

    /// Resize the stream to a peer's first packet.
    ///
    /// Only ever called from [`push_internal`](Self::push_internal) while
    /// `last_seq_in == SEQ_NONE`, i.e. before this stream has stored anything.
    /// Stores the new `num_channels` and resizes every slot's `data` buffer —
    /// **zeroed**, for the same reason `reset()` zeroes slot data: a slot left
    /// over from a previous, differently-sized stream must not leak into this
    /// one as a burst of garbage.
    ///
    /// Nothing the consumer owns is touched. Its per-channel Burg/ring state
    /// is preallocated for [`MAX_CHANNELS`] and is already clean — freshly
    /// constructed, or cleared by `reset()`, which is the only way back to
    /// `SEQ_NONE` — so the consumer picks up the new width by reading
    /// `num_channels` after its `Acquire`. The jitter policy is untouched too:
    /// only `fpp` and the tolerance configuration survive from construction.
    ///
    /// # Concurrency
    ///
    /// This runs on the network thread, inside `push`, and resizes
    /// [`NUM_SLOTS`] slot `Vec`s while the audio thread may concurrently be
    /// inside `pop`. It is sound only because it happens strictly before the
    /// `Release` store of `last_seq_in` in `push_internal`: `pop_internal`'s
    /// `SEQ_NONE` early return touches no slot. Any code path past that early
    /// return is reached only after an `Acquire` load has observed this call's
    /// later `Release` store, so it always sees the fully resized slots, never
    /// a partial one.
    ///
    /// This does **not** cover a `pop` still running the *previous* stream's
    /// real (non-`SEQ_NONE`) path when `reset()` re-arms `SEQ_NONE` and a new
    /// peer then adopts a different channel count here — that `pop` could read
    /// a slot already resized out from under it.
    /// `WebTripSession::disconnect` closes that window by awaiting
    /// `AudioContext.suspend()` inside `stop_capture` before `reset()` (WEB-53
    /// Bug 7), so no worklet callback can still be inside `pop` when this runs.
    fn adopt_channel_count(&self, channels: usize) {
        let samples_per_packet = self.fpp * channels;
        for slot in self.slots.iter() {
            // SAFETY: this is the producer, and no consumer reads a slot
            // before `last_seq_in` leaves `SEQ_NONE` (see above).
            let payload = unsafe { slot.payload_mut() };
            payload.data.clear();
            payload.data.resize(samples_per_packet, 0.0);
        }
        self.num_channels.store(channels, Ordering::Relaxed);
    }

    /// Reset the shared half of the regulator. See [`Regulator::reset`] for
    /// the quiescence the caller must provide.
    fn reset(&self) {
        // Publish SEQ_NONE first so a concurrent pop's next Acquire exits early.
        self.last_seq_in.store(SEQ_NONE, Ordering::Release);
        self.last_seq_out.store(SEQ_NONE, Ordering::Release);
        self.start_time_bits.store(f64_to_bits(0.0), Ordering::Relaxed);
        self.packets_rejected.store(0, Ordering::Relaxed);

        // Reset slots without deallocating. The audio is zeroed, not just the
        // timestamp: after a reset the next stream can jump the write pointer
        // over slots this one filled, and those slots are readable again as
        // soon as they fall inside the new stream's span. Silence there is a
        // dropout; the previous connection's audio is a burst of garbage.
        for slot in self.slots.iter() {
            // SAFETY: producer and consumer are quiesced (`Regulator::reset`).
            let payload = unsafe { slot.payload_mut() };
            payload.timestamp = 0.0;
            payload.data.fill(0.0);
            slot.clear_seq();
        }
    }

    /// Get current statistics.
    fn stats(&self) -> RegulatorStats {
        // Only atomics — safe to call from the main thread while push/pop run
        // (WEB-53 Bug 6).
        let last_seq_raw = self.last_seq_in.load(Ordering::Acquire);
        let last_seq = if last_seq_raw == SEQ_NONE { 0 } else { last_seq_raw as u16 };
        let packets = self.pub_packets.load(Ordering::Relaxed);
        RegulatorStats {
            tolerance_ms: f64_from_bits(self.pub_tolerance_bits.load(Ordering::Relaxed)),
            headroom_ms: f64_from_bits(self.pub_headroom_bits.load(Ordering::Relaxed)),
            max_latency_ms: f64_from_bits(self.pub_max_latency_bits.load(Ordering::Relaxed)),
            glitches: self.pub_glitches.load(Ordering::Relaxed),
            packets_received: packets,
            packets_played: packets,
            last_seq_received: last_seq,
            packets_rejected: self.packets_rejected.load(Ordering::Relaxed),
        }
    }

    /// Get current buffer depth (number of packets buffered).
    fn depth(&self) -> u32 {
        let write_raw = self.last_seq_in.load(Ordering::Acquire);
        if write_raw == SEQ_NONE {
            return 0;
        }
        let write = write_raw as u16;
        let read_raw = self.last_seq_out.load(Ordering::Acquire);
        let read = if read_raw == SEQ_NONE { write } else { read_raw as u16 };
        // Use wrapping arithmetic to handle sequence number wraparound (u16)
        write.wrapping_sub(read) as u32
    }

    /// Get approximate latency in milliseconds.
    fn latency_ms(&self) -> f32 {
        let depth = self.depth();
        // `depth` counts packets (sequence numbers), each `fpp` frames long —
        // not `fpp * channels`, which also carries the channel count and
        // would double-count latency for anything wider than mono.
        let total_frames = depth * self.fpp as u32;
        (total_frames as f32 / self.sample_rate as f32) * 1000.0
    }
}

impl ConsumerState {
    /// Pop samples for playback (internal with explicit timestamp).
    ///
    /// # Arguments
    /// * `output` - Buffer to write samples to (interleaved)
    /// * `now_ms` - Current timestamp in milliseconds
    ///
    /// # Returns
    /// `true` if real packet data was output, `false` if concealment was used
    fn pop_internal(&mut self, sh: &Shared, output: &mut [f32], now_ms: f64) -> bool {
        // Acquire FIRST so the stream shape and slots written before the
        // matching Release are visible (WEB-53 Bug 2).
        let last_seq_in_raw = sh.last_seq_in.load(Ordering::Acquire);
        let last_seq_in = last_seq_in_raw as u16;
        let relative_now = now_ms - sh.start_time_ms();

        // Track inter-pop timing after the acquire so relative_now is coherent.
        if self.last_pop_time_ms > 0.0 {
            let elapsed = now_ms - self.last_pop_time_ms;
            self.pull_stats.tick(elapsed, relative_now);
        }
        self.last_pop_time_ms = now_ms;

        // Return silence during startup
        if last_seq_in_raw == SEQ_NONE || relative_now < self.tolerance_ms {
            output.fill(0.0);
            self.publish_stats(sh);
            return false;
        }

        // A started stream's width is fixed until the next reset, and the
        // Acquire above ordered its adoption.
        self.num_channels = sh.channels();

        // Check for underrun (no new packets)
        let last_out_raw = sh.last_seq_out.load(Ordering::Acquire);
        if last_out_raw != SEQ_NONE && last_seq_in == last_out_raw as u16 {
            return self.handle_underrun(sh, output, relative_now);
        }

        // Find best packet to output (NO allocations)
        let result = self.find_best_packet(sh, last_seq_in, relative_now);

        match result {
            Some(PacketDecision::Packet { seq, slot_idx }) => {
                // Identity must still hold before we trust the payload: a
                // `last_stashed` decision from an earlier pop bypasses
                // find_best_packet's own check, and if this thread stalled
                // since then the producer may have lapped the ring onto
                // this slot (WEB-53).
                let n = self.num_channels;
                let Some(payload) = sh.slots[slot_idx].read(seq) else {
                    // Stamp mismatch: conceal rather than play torn audio.
                    self.pull_stats.underruns += 1;
                    self.output_concealment(sh.fpp, output);
                    self.publish_stats(sh);
                    return false;
                };

                // Deinterleave from slot directly into channel tmp buffers (NO allocation)
                for (ch, channel) in self.channels[..n].iter_mut().enumerate() {
                    for s in 0..sh.fpp {
                        channel.tmp_buf[s] = payload.data[s * n + ch];
                    }
                }

                // Process with Burg algorithm
                self.process_burg(sh.fpp, false);
                self.interleave_output(sh.fpp, output);

                sh.last_seq_out.store(seq as i32, Ordering::Release);
                self.packet_count += 1;
                self.publish_stats(sh);
                true
            }
            Some(PacketDecision::ConcealSkippedPacket { skipped }) => {
                self.pull_stats.overruns += skipped;
                self.output_concealment(sh.fpp, output);
                self.publish_stats(sh);
                false
            }
            None => {
                self.handle_underrun(sh, output, relative_now)
            }
        }
    }

    /// Handle an underrun (missing packet).
    fn handle_underrun(&mut self, sh: &Shared, output: &mut [f32], now: f64) -> bool {
        self.pull_stats.underruns += 1;
        self.publish_stats(sh);

        // Check for stuck client (no packets for a long time). Validate the
        // slot stamp before trusting the timestamp — the producer may have
        // recycled this slot (WEB-53 Bug 1).
        let last_out_raw = sh.last_seq_out.load(Ordering::Acquire);
        if last_out_raw != SEQ_NONE {
            let last_out = last_out_raw as u16;
            if let Some(payload) = sh.slots[slot_index(last_out)].read(last_out) {
                if now - payload.timestamp > 10000.0 {
                    // Stuck - output silence
                    output.fill(0.0);
                    return false;
                }
            }
        }

        // Good underrun - use prediction
        self.output_concealment(sh.fpp, output);
        false
    }

    fn output_concealment(&mut self, fpp: usize, output: &mut [f32]) {
        for channel in &mut self.channels[..self.num_channels] {
            channel.tmp_buf.fill(0.0);
        }
        self.process_burg(fpp, true);
        self.interleave_output(fpp, output);
    }

    fn interleave_output(&self, fpp: usize, output: &mut [f32]) {
        let n = self.num_channels;
        for (ch, channel) in self.channels[..n].iter().enumerate() {
            for s in 0..fpp {
                let idx = s * n + ch;
                if idx < output.len() {
                    output[idx] = channel.output_now_packet[s];
                }
            }
        }
    }

    /// Find the best packet to output based on timing.
    fn find_best_packet(&mut self, sh: &Shared, last_seq_in: u16, now: f64) -> Option<PacketDecision> {
        if let Some((seq, slot_idx)) = self.last_stashed {
            self.last_stashed = None;
            return Some(PacketDecision::Packet { seq, slot_idx });
        }

        let last_out_raw = sh.last_seq_out.load(Ordering::Acquire);
        let last_out = (last_out_raw != SEQ_NONE).then_some(last_out_raw as u16);
        let start_seq = match last_out {
            Some(last_out) => last_out.wrapping_add(1),
            None => last_seq_in,
        };

        let new_pkts = seq_span(start_seq, last_seq_in);
        if new_pkts == 0 {
            return None;
        }
        if new_pkts > NUM_SLOTS {
            // The seam: this span is representable in sequence space but not
            // retrievable from slot space, because every slot it names has been
            // overwritten by a later packet. Waiting cannot bring those packets
            // back, so the read pointer has lost contact with the stream —
            // reached by an inbound outage longer than the ring's ~10.9 s
            // horizon, or by a wild `last_seq_out` dragged forward by a bad
            // candidate below. Holding the stale pointer conceals until the
            // stream counts all the way around to it (tens of seconds); drop it
            // and resynchronize onto the newest packet on the next callback
            // instead.
            //
            // The reset lives on this side of the buffer so that `last_seq_out`
            // keeps its single-writer (audio thread) property — `last_seq_in` is
            // atomic precisely because it is the one pointer both agents touch.
            sh.last_seq_out.store(SEQ_NONE, Ordering::Release);
            return None;
        }

        let mut skipped = 0u64;
        let mut first_good_skipped: Option<(u16, usize, f64)> = None;

        // Find the best candidate (NO allocations - just return indices)
        let mut best_candidate: Option<(u16, usize, f64)> = None;

        for i in (0..new_pkts).rev() {
            let seq = last_seq_in.wrapping_sub(i as u16);
            let slot_idx = slot_index(seq);

            let Some(timestamp) = sh.slots[slot_idx].read(seq).map(|p| p.timestamp) else {
                continue;
            };

            // Calculate skipped packet count (recalculate for each candidate)
            if let Some(last_out) = last_out {
                skipped = seq.wrapping_sub(last_out.wrapping_add(1)) as u64;
            }

            // Update max latency
            let latency = now - timestamp;
            if latency > self.stats_max_latency {
                self.stats_max_latency = latency;
            }

            // Check if packet meets tolerance or is the best candidate
            if timestamp + self.tolerance_ms >= now || i == 0 {
                if skipped == 1 && first_good_skipped.is_some() {
                    // special case where we are about to skip 1 good packet.
                    // this defers latency adjustments until they are at least
                    // 2 packets wide.
                    self.last_stashed = Some((seq, slot_idx));
                    best_candidate = first_good_skipped;
                } else if skipped > 0 {
                    // process a glitch to account for the skipped packets,
                    // but stash and use this good packet on next callback.
                    self.last_stashed = Some((seq, slot_idx));
                    self.update_push_stats(sh, seq, timestamp, now);
                    return Some(PacketDecision::ConcealSkippedPacket { skipped });
                } else {
                    best_candidate = Some((seq, slot_idx, timestamp));
                }
                break;
            }

            // Track first good packet that was skipped
            if first_good_skipped.is_none() {
                first_good_skipped = Some((seq, slot_idx, timestamp));
            }
        }

        // Update push stats if we found a candidate
        if let Some((seq, slot_idx, timestamp)) = best_candidate {
            self.update_push_stats(sh, seq, timestamp, now);
            return Some(PacketDecision::Packet { seq, slot_idx });
        }

        None
    }

    /// Update push statistics when pulling a packet.
    fn update_push_stats(&mut self, sh: &Shared, seq: u16, timestamp: f64, now: f64) {
        let last_out_raw = sh.last_seq_out.load(Ordering::Acquire);
        if last_out_raw == SEQ_NONE {
            return;
        }
        let last_out = last_out_raw as u16;

        // Estimate previous packet timing (use wrapping arithmetic for u16)
        let pkts = seq.wrapping_sub(last_out.wrapping_add(1)) as usize;

        // Stamp check before trusting timestamp (WEB-53 Bug 4).
        let Some(last_payload) = sh.slots[slot_index(last_out)].read(last_out) else {
            return;
        };
        let prev_time = last_payload.timestamp + (pkts as f64 + 1.0) * sh.fpp_duration_ms();
        if prev_time < timestamp {
            let elapsed = timestamp - prev_time;
            let updated = self.push_stats.tick(elapsed, now);

            if updated && self.push_stats.long_term_count % WINDOW_DIVISOR == 0 {
                self.update_tolerance(sh, now);
            }
        }
    }

    /// Update auto-adaptive tolerance.
    fn update_tolerance(&mut self, sh: &Shared, now: f64) {
        if !self.auto_mode || now < AUTO_INIT_DURATION_MS {
            return;
        }

        let total_glitches = self.pull_stats.underruns + self.pull_stats.overruns;
        let new_glitches = total_glitches.saturating_sub(self.last_glitches);
        self.last_glitches = total_glitches;

        // Skip warmup period
        if now <= self.auto_headroom_start_time {
            self.last_max_latency  = 0.0;  // ignore during warmup
            self.stats_max_latency = 0.0;
            self.update_headroom(sh, 0);
        } else {
            self.last_max_latency = self.stats_max_latency;
            self.stats_max_latency = 0.0;
            self.update_headroom(sh, new_glitches);
        }
    }

    /// Update headroom based on glitch counts.
    fn update_headroom(&mut self, sh: &Shared, glitches: u64) {
        if self.auto_headroom < 0.0 {
            // Variable headroom mode
            let glitches_allowed =
                ((AUTO_HEADROOM_GLITCH_TOLERANCE * sh.sample_rate as f64) / sh.fpp as f64)
                    .ceil() as u64;
            let max_headroom = (self.push_stats.long_term_max * 3.0)
                .max(self.last_max_latency + 10.0);

            if glitches > glitches_allowed && self.current_headroom < max_headroom {
                if self.skip_auto_headroom {
                    self.skip_auto_headroom = false;
                } else {
                    self.skip_auto_headroom = true;
                    if self.last_max_latency > self.tolerance_ms + 3.0 {
                        // special case to grow headroom faster to catch up
                        let headroom_increase = ((self.last_max_latency - self.tolerance_ms) / 2.0).ceil();
                        self.current_headroom = (self.current_headroom + headroom_increase).min(max_headroom);
                    } else {
                        self.current_headroom += 1.0;
                    }
                }
            } else {
                self.skip_auto_headroom = true;
            }
        } else {
            self.current_headroom = self.auto_headroom;
        }

        // Calculate new tolerance
        let push_tol = self.push_stats.calc_auto();
        let pull_tol = self.pull_stats.calc_auto();
        let mut new_tolerance = (push_tol + self.current_headroom).max(pull_tol);

        new_tolerance = new_tolerance.clamp(sh.fpp_duration_ms(), AUTO_MAX_MS);
        self.tolerance_ms = new_tolerance;
        self.publish_stats(sh);
    }

    /// Process audio with Burg algorithm for PLC.
    fn process_burg(&mut self, fpp: usize, glitch: bool) {
        let primed = self.plc_packet_count > self.packets_in_past as u64;

        for channel in &mut self.channels[..self.num_channels] {
            // Copy real packet data
            for s in 0..fpp {
                channel.real_now_packet[s] = if !glitch { channel.tmp_buf[s] } else { 0.0 };
            }

            // If not a glitch, push to ring buffer
            if !glitch {
                channel.ring_buffer_push();
            }

            // Build real past from ring buffer
            if primed {
                let mut offset = 0;
                for i in 0..self.packets_in_past {
                    channel.ring_buffer_pull(self.packets_in_past - i);
                    for s in 0..fpp {
                        if offset + s < channel.prediction.len() {
                            channel.prediction[offset + s] = channel.tmp_buf[s];
                        }
                    }
                    offset += fpp;
                }
            }

            // Perform prediction on glitch
            if glitch {
                // Copy predicted past into prediction buffer
                for i in 0..self.packets_in_past {
                    for s in 0..fpp {
                        let idx = i * fpp + s;
                        if idx < channel.prediction.len() {
                            channel.prediction[idx] = channel.predicted_past[i][s];
                        }
                    }
                }

                // Train Burg model using pre-allocated buffer (no allocation!)
                channel.train_data[..self.up_to_now].copy_from_slice(&channel.prediction[..self.up_to_now]);
                self.burg.train(&channel.train_data, &mut channel.coeffs);

                // Predict future samples
                let tail_size = channel.prediction.len();
                self.burg.predict(&channel.coeffs, &mut channel.prediction, tail_size);

                // Extract predicted now packet
                for s in 0..fpp {
                    let idx = self.up_to_now + s;
                    channel.predicted_now_packet[s] =
                        channel.prediction.get(idx).copied().unwrap_or(0.0);
                }
            }

            // Generate output with crossfade
            for s in 0..fpp {
                channel.output_now_packet[s] = if glitch {
                    if primed {
                        channel.predicted_now_packet[s]
                    } else {
                        0.0
                    }
                } else if self.last_was_glitch {
                    // Crossfade from prediction to real
                    self.fade_down[s] * channel.future_predicted_packet[s]
                        + self.fade_up[s] * channel.real_now_packet[s]
                } else {
                    channel.real_now_packet[s]
                };
            }

            // Copy output to tmp_buf for consistency
            channel.tmp_buf.copy_from_slice(&channel.output_now_packet);

            // Shift predicted past (rotate left without allocation)
            // This moves all packets forward by one position in a single operation
            channel.predicted_past.rotate_left(1);
            // Now the last position is free, fill it with current output
            channel.predicted_past[self.packets_in_past - 1]
                .copy_from_slice(&channel.output_now_packet);

            // Store future prediction for next crossfade
            for s in 0..fpp {
                let idx = self.beyond_now + s;
                channel.future_predicted_packet[s] =
                    channel.prediction.get(idx).copied().unwrap_or(0.0);
            }
        }

        self.last_was_glitch = glitch;
        self.plc_packet_count += 1;
    }

    /// Reset the consumer's state. See [`Regulator::reset`] for the
    /// quiescence the caller must provide.
    fn reset(&mut self, sh: &Shared) {
        // A stash left over from the previous connection would otherwise be
        // returned on the first pop after reconnect, pinning `last_seq_out` to
        // a stale sequence number and triggering thousands of spurious PLCs
        // until the new stream's sequence numbers caught up.
        self.last_stashed = None;
        self.packet_count = 0;
        self.plc_packet_count = 0;
        self.last_glitches = 0;
        self.stats_glitches = 0;
        self.last_max_latency = 0.0;
        self.stats_max_latency = 0.0;
        self.last_was_glitch = false;
        self.tolerance_ms = if self.auto_mode {
            sh.fpp as f64 * AUTO_INIT_VAL_FACTOR
        } else {
            self.auto_headroom
        };
        self.current_headroom = if self.auto_headroom < 0.0 {
            0.0
        } else {
            self.auto_headroom
        };
        self.skip_auto_headroom = true;
        self.auto_headroom_start_time = 6000.0;

        // Reset timing
        self.last_pop_time_ms = 0.0;

        // Reset timing stats state
        self.pull_stats.reset();
        self.push_stats.reset();

        // Every channel, not just the ones in use: the next stream may adopt
        // a wider count, and `adopt_channel_count` relies on these being clean.
        for channel in &mut self.channels {
            channel.clear();
        }
        self.publish_stats(sh);
    }

    /// Publish the consumer-owned stats snapshot (WEB-53 Bug 6).
    fn publish_stats(&self, sh: &Shared) {
        let total_glitches = self.pull_stats.underruns + self.pull_stats.overruns;
        sh.pub_tolerance_bits
            .store(f64_to_bits(self.tolerance_ms), Ordering::Relaxed);
        sh.pub_headroom_bits
            .store(f64_to_bits(self.current_headroom), Ordering::Relaxed);
        sh.pub_max_latency_bits
            .store(f64_to_bits(self.last_max_latency), Ordering::Relaxed);
        sh.pub_glitches.store(
            total_glitches.saturating_sub(self.stats_glitches),
            Ordering::Relaxed,
        );
        sh.pub_packets.store(self.packet_count, Ordering::Relaxed);
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Plant a packet directly in the ring, bypassing `push`, so a test can
    /// lay out an arrival timeline (timestamps, gaps, out-of-order arrivals)
    /// without driving the wall clock. `samples` must be a full packet — the
    /// same contract `push` enforces.
    fn plant_packet(reg: &mut Regulator, seq: u16, timestamp: f64, samples: &[f32]) {
        let slot = &mut reg.shared.slots[slot_index(seq)];
        let payload = slot.payload.get_mut();
        payload.timestamp = timestamp;
        payload.data.copy_from_slice(samples);
        slot.publish_seq(seq);
    }

    /// Push a packet a test expects the regulator to store, asserting the
    /// outcome. Tests that drive `push_internal` for setup would otherwise
    /// discard the `#[must_use]` result, so a push that starts getting
    /// rejected (wrong size, unadopted channel count) would leave the
    /// regulator empty and surface as a confusing failure further down —
    /// or as no failure at all.
    fn push_stored(reg: &Regulator, seq: u16, channels: usize, samples: &[f32], now_ms: f64) {
        assert_eq!(
            reg.push_internal(seq, channels, samples, now_ms),
            PushOutcome::Stored,
            "setup push of seq {seq} ({channels}ch) must be stored"
        );
    }

    fn last_seq_out_opt(reg: &Regulator) -> Option<u16> {
        let v = reg.shared.last_seq_out.load(Ordering::Acquire);
        if v == SEQ_NONE {
            None
        } else {
            Some(v as u16)
        }
    }

    fn set_last_seq_out(reg: &Regulator, v: Option<u16>) {
        let raw = match v {
            Some(s) => s as i32,
            None => SEQ_NONE,
        };
        reg.shared.last_seq_out.store(raw, Ordering::Release);
    }

    /// The consumer's state, borrowed through `&mut Regulator` so the borrow
    /// checker — not the SPSC protocol — keeps test borrows from overlapping.
    fn consumer(reg: &mut Regulator) -> &mut ConsumerState {
        reg.consumer.get_mut()
    }

    fn update_tolerance(reg: &mut Regulator, now: f64) {
        reg.consumer.get_mut().update_tolerance(&reg.shared, now);
    }

    #[test]
    fn test_burg_training_and_prediction() {
        let mut burg = BurgAlgorithm::new(64);
        let mut coeffs = vec![0.0f32; 63];

        // Create a simple sinusoidal signal
        let input: Vec<f32> = (0..64)
            .map(|i| (i as f32 * 0.1).sin())
            .collect();

        burg.train(&input, &mut coeffs);

        // Prediction buffer: past samples + space for predictions
        let mut tail = vec![0.0f32; 128];
        tail[..64].copy_from_slice(&input);

        burg.predict(&coeffs, &mut tail, 128);

        // Check that prediction was made (non-zero values after input)
        let has_predictions = tail[64..].iter().any(|&x| x.abs() > 1e-10);
        assert!(has_predictions, "Burg should produce non-zero predictions");
    }

    #[test]
    fn test_channel_ring_buffer() {
        let mut channel = ChannelState::new(128, 256, 2);

        // Fill tmp_buf with test data
        for i in 0..128 {
            channel.tmp_buf[i] = i as f32;
        }

        // Push to ring
        channel.ring_buffer_push();

        // Clear tmp_buf
        channel.tmp_buf.fill(0.0);

        // Pull back
        channel.ring_buffer_pull(1);

        // Verify data
        for i in 0..128 {
            assert!((channel.tmp_buf[i] - i as f32).abs() < 1e-6);
        }
    }

    #[test]
    fn test_sequence_number_wraparound() {
        let reg = Regulator::with_params(1, 128, 48000, 50.0);

        // Create test samples
        let samples: Vec<f32> = (0..128).map(|i| (i as f32 * 0.01).sin()).collect();
        
        // Test near u16::MAX (65535) to verify wraparound works
        let near_max: u16 = u16::MAX - 2;
        
        // Push packets near wraparound boundary with proper timing
        push_stored(&reg, near_max, 1, &samples, 0.0);
        push_stored(&reg, near_max.wrapping_add(1), 1, &samples, 3.0);
        push_stored(&reg, near_max.wrapping_add(2), 1, &samples, 6.0); // This wraps to 0
        push_stored(&reg, 0, 1, &samples, 9.0); // Already wrapped
        push_stored(&reg, 1, 1, &samples, 12.0);
        
        let mut output = vec![0.0f32; 128];
        
        // Pop packets - should work smoothly across wraparound
        // Pop after tolerance is met
        let _result1 = reg.pop_internal(&mut output, 60.0);
        let _result2 = reg.pop_internal(&mut output, 63.0);
        let _result3 = reg.pop_internal(&mut output, 66.0);
        
        // Verify depth calculation works across wraparound
        let depth = reg.depth();
        assert!(depth < 100); // Should be a reasonable small number, not huge
        
        // Verify last_seq_out was set properly (should be Some value, not causing issues)
        assert!(last_seq_out_opt(&reg).is_some());
    }

    /// An inbound outage longer than the ring's horizon strands the read
    /// pointer: the peer keeps counting (sequence numbers on the wire only ever
    /// go up), so when packets resume the span from `last_seq_out` to
    /// `last_seq_in` is wider than NUM_SLOTS and every slot it names has been
    /// overwritten by a later packet.
    ///
    /// Nothing detects that on its own — the scan still finds *data* at those
    /// slot indices, just the wrong packets, aliases from before the outage.
    /// Without the seam check (commit 10a3b37) the buffer therefore keeps
    /// running while playing audio a full ring behind the live stream, and
    /// crawls forward far slower than the stream advances: measured at
    /// `last_seq_out` = 1612 after 600 packets of a stream at 5709, i.e. ~10.7 s
    /// of permanent added latency. The span check must drop the pointer instead
    /// and resynchronize onto the live stream within a couple of callbacks.
    #[test]
    fn test_loss_burst_wider_than_ring_resyncs_read_pointer() {
        let reg = Regulator::with_params(1, 128, 48_000, 5.0);
        let samples = vec![0.5f32; reg.fpp()];
        let dt = reg.shared.fpp_duration_ms();
        let mut out = vec![0.0f32; reg.fpp()];

        // A stream that is playing out normally.
        let mut t = 0.0;
        for i in 0..10u16 {
            push_stored(&reg, 100 + i, 1, &samples, t);
            t += dt;
        }
        let mut pop_t = 10.0;
        for _ in 0..10 {
            reg.pop_internal(&mut out, pop_t);
            pop_t += dt;
        }
        assert_eq!(last_seq_out_opt(&reg), Some(109), "stream should be playing out");

        // Nothing arrives for 5000 packets — 13.3 s at fpp=128, past the ring's
        // 4096-packet (10.9 s) horizon. An ordinary wifi roam, not an exotic
        // event. The peer never renumbers; it just keeps counting.
        let gap = 5_000u16;
        let resume_t = t + gap as f64 * dt;
        let first_resumed = 109 + gap + 1;

        let mut first_real_pkt = None;
        let mut tail_real = 0;
        for k in 0..600u16 {
            let now = resume_t + k as f64 * dt;
            push_stored(&reg, first_resumed + k, 1, &samples, now);
            let real = reg.pop_internal(&mut out, now + 1.0);
            if real && first_real_pkt.is_none() {
                first_real_pkt = Some(k);
            }
            if k >= 550 && real {
                tail_real += 1;
            }
        }

        let first_real_pkt =
            first_real_pkt.expect("real audio must resume after a loss burst wider than the ring");
        assert!(
            first_real_pkt <= 4,
            "the read pointer must resync within a couple of callbacks \
             (first real audio {first_real_pkt} packets in)"
        );
        assert!(
            tail_real >= 45,
            "playback must be sustained once resynced, not intermittent ({tail_real}/50)"
        );
        assert_eq!(last_seq_out_opt(&reg),
            Some(first_resumed + 599),
            "the read pointer must be on the live stream, not replaying slot aliases \
             from before the outage"
        );
        assert_eq!(
            reg.depth(),
            0,
            "resync must clear the backlog, not leave a ring's worth of latency behind"
        );
    }

    /// If a candidate sequence number's packet was actually lost (never
    /// arrived), but a slot from `NUM_SLOTS` sequence numbers earlier — which
    /// aliases onto the same slot index — is still sitting there
    /// unoverwritten, `find_best_packet` must not mistake that stale data for
    /// the candidate. The stale timestamp is deliberately kept close to the
    /// last-played one so the timestamp-proximity heuristic alone would not
    /// catch it — only a direct seq check can.
    #[test]
    fn test_stale_aliased_slot_is_not_played_as_missing_packet() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 10.0);
        let last_seq = 5u16;
        let candidate_seq = last_seq.wrapping_add(1); // 6 — genuinely lost, never planted
        let stale_seq = candidate_seq.wrapping_sub(NUM_SLOTS as u16); // aliases to same slot
        let last_packet = vec![0.1f32; reg.fpp()];
        let stale_packet = vec![0.9f32; reg.fpp()];
        set_last_seq_out(&reg, Some(last_seq));
        reg.shared.last_seq_in.store(candidate_seq as i32, Ordering::Release);

        plant_packet(&mut reg, last_seq, 40.0, &last_packet);
        plant_packet(&mut reg, stale_seq, 41.0, &stale_packet); // candidate_seq never planted

        let mut output = vec![0.0f32; reg.fpp()];
        let result = reg.pop_internal(&mut output, 50.0);

        assert!(
            !result,
            "a slot whose stored seq doesn't match the candidate must not be \
             played back as real audio"
        );
        assert_eq!(last_seq_out_opt(&reg),
            Some(last_seq),
            "read pointer must not advance onto a candidate whose slot \
             actually holds a stale, aliased packet"
        );
        assert_ne!(
            output, stale_packet,
            "the stale aliased packet's data must never be echoed back as \
             candidate_seq's audio"
        );
    }

    #[test]
    fn test_burg_priming_uses_plc_iterations() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 10.0);
        let fpp = reg.fpp();
        let c = consumer(&mut reg);
        let history_packets = c.packets_in_past;
        let history: Vec<f32> = (0..c.up_to_now)
            .map(|i| (i as f32 * 0.1).sin())
            .collect();

        for (i, packet) in c.channels[0].predicted_past.iter_mut().enumerate() {
            packet.copy_from_slice(&history[i * fpp..(i + 1) * fpp]);
        }

        c.process_burg(fpp, true);
        assert!(
            c.channels[0]
                .output_now_packet
                .iter()
                .all(|sample| sample.abs() < 1e-6),
            "PLC should stay muted before the predictor is primed"
        );

        c.plc_packet_count = history_packets as u64 + 1;
        c.process_burg(fpp, true);
        assert!(
            c.channels[0]
                .output_now_packet
                .iter()
                .any(|sample| sample.abs() > 1e-6),
            "PLC should emit predicted audio once the PLC iteration counter is primed"
        );
        assert_eq!(c.packet_count, 0);
        assert_eq!(c.plc_packet_count, history_packets as u64 + 2);
    }

    #[test]
    fn test_skipped_packet_outputs_concealment_before_stashed_real_packet() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 10.0);
        let fpp = reg.fpp();
        let fpp_duration_ms = reg.shared.fpp_duration_ms();
        let packet = vec![0.25f32; fpp];
        let last_packet = vec![0.1f32; fpp];
        let last_seq = 10u16;
        let good_seq = last_seq.wrapping_add(2);

        {
            let c = consumer(&mut reg);
            let history: Vec<f32> = (0..c.up_to_now)
                .map(|i| (i as f32 * 0.1).sin())
                .collect();
            for (i, predicted) in c.channels[0].predicted_past.iter_mut().enumerate() {
                predicted.copy_from_slice(&history[i * fpp..(i + 1) * fpp]);
            }
            c.plc_packet_count = c.packets_in_past as u64 + 1;
        }
        set_last_seq_out(&reg, Some(last_seq));
        reg.shared.last_seq_in.store(good_seq as i32, Ordering::Release);

        plant_packet(&mut reg, last_seq, 40.0, &last_packet);
        plant_packet(&mut reg, good_seq, 45.0, &packet);

        let mut concealment = vec![0.0f32; reg.fpp()];
        let concealment_result = reg.pop_internal(&mut concealment, 50.0);
        assert!(!concealment_result);
        assert_eq!(consumer(&mut reg).pull_stats.overruns, 1);
        assert_eq!(consumer(&mut reg).last_stashed.map(|(seq, _)| seq), Some(good_seq));
        assert_eq!(last_seq_out_opt(&reg), Some(last_seq));
        assert!(
            concealment.iter().any(|sample| sample.abs() > 1e-6),
            "skipped packets should trigger PLC output before the real packet is replayed"
        );

        let mut real_output = vec![0.0f32; reg.fpp()];
        let real_result = reg.pop_internal(&mut real_output, 50.0 + fpp_duration_ms);
        assert!(real_result);
        assert_eq!(consumer(&mut reg).last_stashed, None);
        assert_eq!(last_seq_out_opt(&reg), Some(good_seq));
        assert_eq!(consumer(&mut reg).packet_count, 1);
        assert!(
            real_output.iter().any(|sample| sample.abs() > 1e-6),
            "the stashed real packet should be rendered on the following callback"
        );
    }

    /// When exactly one good packet would be skipped and an older good packet
    /// is still buffered, `find_best_packet` defers the latency adjustment:
    /// it plays the stale packet instead of concealing, stashes the fresh
    /// packet for the next callback, and records no glitch. Latency
    /// adjustments only happen once they are at least 2 packets wide.
    #[test]
    fn test_single_skipped_packet_defers_latency_adjustment() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 10.0);
        let fpp_duration_ms = reg.shared.fpp_duration_ms();
        let stale_packet = vec![0.25f32; reg.fpp()];
        let fresh_packet = vec![0.5f32; reg.fpp()];
        let last_seq = 10u16;
        let stale_seq = last_seq.wrapping_add(1);
        let fresh_seq = last_seq.wrapping_add(2);
        set_last_seq_out(&reg, Some(last_seq));
        reg.shared.last_seq_in.store(fresh_seq as i32, Ordering::Release);

        // Previously-played packet, reference point for push-stats timing.
        let silent_packet = vec![0.0f32; reg.fpp()];
        plant_packet(&mut reg, last_seq, 40.0, &silent_packet);
        // Stale but valid packet: misses tolerance at now=60 (41 + 10 < 60),
        // so the scan records it as `first_good_skipped` instead of playing it.
        plant_packet(&mut reg, stale_seq, 41.0, &stale_packet);
        // Fresh packet within tolerance (55 + 10 >= 60); playing it would skip
        // exactly one good packet, which triggers the deferral.
        plant_packet(&mut reg, fresh_seq, 55.0, &fresh_packet);

        let mut output = vec![0.0f32; reg.fpp()];
        let first_result = reg.pop_internal(&mut output, 60.0);
        assert!(
            first_result,
            "deferral should play the stale packet as real audio, not concealment"
        );
        assert_eq!(last_seq_out_opt(&reg), Some(stale_seq));
        assert_eq!(consumer(&mut reg).last_stashed.map(|(seq, _)| seq), Some(fresh_seq));
        assert_eq!(consumer(&mut reg).pull_stats.overruns, 0, "no glitch may be recorded");
        assert_eq!(consumer(&mut reg).packet_count, 1);

        let second_result = reg.pop_internal(&mut output, 60.0 + fpp_duration_ms);
        assert!(second_result);
        assert_eq!(consumer(&mut reg).last_stashed, None);
        assert_eq!(last_seq_out_opt(&reg), Some(fresh_seq));
        assert_eq!(consumer(&mut reg).packet_count, 2);
        assert_eq!(consumer(&mut reg).pull_stats.overruns, 0);
    }

    /// After a full push/pop cycle that accumulates state (stats, sequence
    /// numbers, timing), `reset()` must scrub the regulator back to a
    /// freshly-constructed state. This covers the field-by-field cleanup
    /// required for safe stream reconnection.
    #[test]
    fn test_reset_clears_state_after_active_stream() {
        let mut reg = Regulator::with_params(2, 32, 48_000, 5.0);
        let samples = vec![0.25f32; reg.fpp() * reg.channels()];

        // Drive a small stream through the regulator.
        push_stored(&reg, 0, 2, &samples, 0.0);
        push_stored(&reg, 1, 2, &samples, 2.0);
        let mut out = vec![0.0f32; reg.fpp() * reg.channels()];
        let _ = reg.pop_internal(&mut out, 10.0);
        let _ = reg.pop_internal(&mut out, 12.0);
        let _ = reg.pop_internal(&mut out, 14.0); // forces underrun -> PLC

        assert!(reg.is_initialized());
        assert!(last_seq_out_opt(&reg).is_some());
        assert!(consumer(&mut reg).packet_count > 0);
        assert!(consumer(&mut reg).pull_stats.underruns > 0);

        reg.reset();

        assert!(!reg.is_initialized(), "last_seq_in should be cleared");
        assert_eq!(last_seq_out_opt(&reg), None);
        assert_eq!(consumer(&mut reg).last_stashed, None);
        assert_eq!(consumer(&mut reg).packet_count, 0);
        assert_eq!(consumer(&mut reg).plc_packet_count, 0);
        assert_eq!(consumer(&mut reg).last_glitches, 0);
        assert_eq!(consumer(&mut reg).stats_glitches, 0);
        assert_eq!(consumer(&mut reg).last_max_latency, 0.0);
        assert_eq!(consumer(&mut reg).stats_max_latency, 0.0);
        assert!(!consumer(&mut reg).last_was_glitch);
        assert_eq!(reg.shared.start_time_ms(), 0.0);
        assert_eq!(consumer(&mut reg).last_pop_time_ms, 0.0);
        assert_eq!(consumer(&mut reg).pull_stats.underruns, 0);
        assert_eq!(consumer(&mut reg).pull_stats.overruns, 0);
        assert_eq!(consumer(&mut reg).push_stats.long_term_count, 0);
        assert_eq!(reg.depth(), 0);

        // Channels should be zeroed and ring write pointer should be re-centered.
        for channel in &consumer(&mut reg).channels {
            assert!(channel.tmp_buf.iter().all(|s| *s == 0.0));
            assert!(channel.real_now_packet.iter().all(|s| *s == 0.0));
            assert!(channel.output_now_packet.iter().all(|s| *s == 0.0));
            assert_eq!(channel.ring_wptr, channel.ring_size / 2);
        }

        // A reconnect on a new stream with a different channel count must be
        // able to adopt again — reset() re-arms SEQ_NONE, which is the only
        // gate on adoption.
        let mono = vec![0.5f32; reg.fpp()];
        let outcome = reg.push_internal(0, 1, &mono, 0.0);
        assert_eq!(outcome, PushOutcome::Stored);
        assert_eq!(reg.channels(), 1, "reconnect must re-adopt the new peer's channel count");
        assert_eq!(reg.fpp() * reg.channels(), reg.fpp());
    }

    /// The 7def5fc race fix: a `last_stashed` slot left from a prior connection
    /// must be cleared by `reset()`. Otherwise the first pop after reconnect
    /// would replay a stale packet and pin `last_seq_out` to an old sequence
    /// number, triggering a storm of spurious PLC events while the new stream
    /// catches up.
    #[test]
    fn test_reset_clears_stashed_packet_to_avoid_stale_pop() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 5.0);
        let samples = vec![0.5f32; reg.fpp()];

        // Simulate a prior connection that landed with a stashed packet:
        // primer connection seq 7, stash for "next-good" seq 9.
        reg.shared.last_seq_in.store(9, Ordering::Release);
        set_last_seq_out(&reg, Some(7));
        consumer(&mut reg).last_stashed = Some((9, slot_index(9)));
        plant_packet(&mut reg, 9, 100.0, &samples);

        reg.reset();

        assert_eq!(
            consumer(&mut reg).last_stashed, None,
            "stashed packet from prior connection must be cleared"
        );
        assert!(!reg.is_initialized());
        assert_eq!(last_seq_out_opt(&reg), None);

        // After reset, the first pop on a fresh stream should return silence
        // (startup), not the stale stashed buffer.
        push_stored(&reg, 0, 1, &samples, 0.0);
        let mut out = vec![0.0f32; reg.fpp()];
        let result = reg.pop_internal(&mut out, 1.0); // still inside tolerance window
        assert!(!result, "should not replay the pre-reset stash");
        assert!(out.iter().all(|s| *s == 0.0));
    }

    /// Push two packets with a 2-packet gap, then pop. The regulator should
    /// detect the gap, emit a PLC concealment block, and bump the overrun
    /// counter by the number of skipped packets.
    #[test]
    fn test_overrun_counter_increments_when_packets_skipped() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 5.0);
        let samples = vec![0.5f32; reg.fpp()];

        push_stored(&reg, 0, 1, &samples, 0.0);
        let mut out = vec![0.0f32; reg.fpp()];
        let r1 = reg.pop_internal(&mut out, 10.0);
        assert!(r1);
        assert_eq!(last_seq_out_opt(&reg), Some(0));

        // Skip seq 1, 2 — push seq 3 directly. With `skipped = 2`, the
        // regulator should treat this as a glitch, conceal, stash, and bump
        // overruns.
        push_stored(&reg, 3, 1, &samples, 20.0);
        let r2 = reg.pop_internal(&mut out, 40.0);
        assert!(!r2, "skipped-gap path returns concealment (false)");
        assert_eq!(consumer(&mut reg).pull_stats.overruns, 2, "overrun counter tracks skip distance");
        // The good packet must have been stashed for replay on the next pop.
        assert_eq!(consumer(&mut reg).last_stashed.map(|(s, _)| s), Some(3));

        // Next pop should consume the stashed real packet, no new overruns.
        let overruns_before = consumer(&mut reg).pull_stats.overruns;
        let r3 = reg.pop_internal(&mut out, 42.0);
        assert!(r3, "stashed packet replay should be a real-packet pop");
        assert_eq!(last_seq_out_opt(&reg), Some(3));
        assert_eq!(consumer(&mut reg).pull_stats.overruns, overruns_before);
    }

    /// PLC must kick in after consecutive misses and disengage as soon as a
    /// real packet arrives. The result boolean should reflect this transition.
    #[test]
    fn test_plc_engages_on_underrun_and_stops_with_real_packets() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 5.0);
        let samples = vec![0.5f32; reg.fpp()];

        push_stored(&reg, 0, 1, &samples, 0.0);
        let mut out = vec![0.0f32; reg.fpp()];
        let r0 = reg.pop_internal(&mut out, 10.0);
        assert!(r0);
        let plc_after_first = consumer(&mut reg).plc_packet_count;
        let packets_after_first = consumer(&mut reg).packet_count;

        // Three consecutive underrun pops should each invoke PLC.
        for i in 0..3 {
            let t = 12.0 + i as f64 * 2.0;
            let result = reg.pop_internal(&mut out, t);
            assert!(!result, "missing packet at t={t} should produce PLC output");
        }
        assert_eq!(consumer(&mut reg).pull_stats.underruns, 3);
        assert_eq!(
            consumer(&mut reg).plc_packet_count,
            plc_after_first + 3,
            "process_burg should run for each concealed packet"
        );
        assert_eq!(
            consumer(&mut reg).packet_count, packets_after_first,
            "packet_count must not grow while concealing"
        );

        // Real packet arrives — next pop should be real audio again.
        push_stored(&reg, 1, 1, &samples, 25.0);
        let r_resume = reg.pop_internal(&mut out, 30.0);
        assert!(r_resume, "PLC must disengage once a real packet is available");
        assert_eq!(last_seq_out_opt(&reg), Some(1));
        assert_eq!(consumer(&mut reg).packet_count, packets_after_first + 1);
        let underruns_after = consumer(&mut reg).pull_stats.underruns;

        // No further underruns when consumption keeps pace.
        push_stored(&reg, 2, 1, &samples, 32.0);
        let r_next = reg.pop_internal(&mut out, 38.0);
        assert!(r_next);
        assert_eq!(consumer(&mut reg).pull_stats.underruns, underruns_after);
    }

    /// `depth()` and `latency_ms()` should report the gap between the newest
    /// pushed and most recently popped sequence numbers. Each push must grow
    /// depth by exactly one, and the latency formula must follow from depth.
    /// Pops drain depth monotonically until it returns to zero.
    #[test]
    fn test_depth_and_latency_reflect_buffered_packets() {
        let reg = Regulator::with_params(2, 64, 48_000, 5.0);
        assert_eq!(reg.depth(), 0, "depth starts at zero before first packet");
        assert_eq!(reg.latency_ms(), 0.0);

        let samples = vec![0.1f32; reg.fpp() * reg.channels()];

        // Before any pop, last_seq_out is None so depth treats read == write.
        push_stored(&reg, 0, 2, &samples, 0.0);
        push_stored(&reg, 1, 2, &samples, 2.0);
        assert_eq!(reg.depth(), 0, "with no pop, read==write so depth is zero");

        // First pop sets last_seq_out; the in-flight buffer is consumed.
        let mut out = vec![0.0f32; reg.fpp() * reg.channels()];
        let r = reg.pop_internal(&mut out, 10.0);
        assert!(r);
        let seq_after_first_pop = last_seq_out_opt(&reg).expect("pop must set last_seq_out");

        // Push more packets without popping — depth grows by one per push.
        for (i, t) in (1u16..=3).zip([12.0_f64, 14.0, 16.0]) {
            let seq = seq_after_first_pop.wrapping_add(i);
            push_stored(&reg, seq, 2, &samples, t);
            assert_eq!(
                reg.depth() as u16,
                i,
                "depth should equal pushes-since-last-pop ({i})"
            );
        }

        // Latency = depth * fpp / sample_rate (in ms) — frames, not
        // samples_per_packet, which also carries the channel count and would
        // double-count latency at 2ch.
        let depth = reg.depth();
        let expected_ms =
            (depth as f32 * reg.fpp() as f32 / reg.shared.sample_rate as f32) * 1000.0;
        assert!(
            (reg.latency_ms() - expected_ms).abs() < 1e-3,
            "latency_ms ({}) should match depth*fpp/sr ({expected_ms})",
            reg.latency_ms()
        );

        // Drain via repeated pops: depth must decrease monotonically to zero.
        let mut prev_depth = reg.depth();
        let mut t = 18.0_f64;
        for _ in 0..8 {
            if reg.depth() == 0 {
                break;
            }
            let _ = reg.pop_internal(&mut out, t);
            let new_depth = reg.depth();
            assert!(
                new_depth <= prev_depth,
                "depth must not grow during a pop ({prev_depth} -> {new_depth})"
            );
            prev_depth = new_depth;
            t += 2.0;
        }
        assert_eq!(reg.depth(), 0, "depth should reach zero after the buffer drains");
        assert_eq!(reg.latency_ms(), 0.0);
    }

    /// `stats()` should expose tolerance, headroom, counters, and last seq
    /// number consistently with internal state.
    #[test]
    fn test_stats_reflect_internal_counters_and_state() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 7.5);
        let samples = vec![0.0f32; reg.fpp()];
        push_stored(&reg, 42, 1, &samples, 0.0);
        push_stored(&reg, 43, 1, &samples, 2.0);

        let mut out = vec![0.0f32; reg.fpp()];
        let _ = reg.pop_internal(&mut out, 15.0);
        let _ = reg.pop_internal(&mut out, 17.0);
        let _ = reg.pop_internal(&mut out, 19.0); // underrun

        let s = reg.stats();
        assert_eq!(s.tolerance_ms, consumer(&mut reg).tolerance_ms);
        assert_eq!(s.headroom_ms, consumer(&mut reg).current_headroom);
        assert_eq!(s.max_latency_ms, consumer(&mut reg).last_max_latency);
        assert_eq!(s.packets_received, consumer(&mut reg).packet_count);
        assert_eq!(s.packets_played, consumer(&mut reg).packet_count);
        assert_eq!(
            s.last_seq_received, 43,
            "last_seq_received should match the highest seq pushed"
        );
        let expected_glitches =
            (consumer(&mut reg).pull_stats.underruns + consumer(&mut reg).pull_stats.overruns) - consumer(&mut reg).stats_glitches;
        assert_eq!(s.glitches, expected_glitches);

        // Stats on a fresh regulator: zeroed counters, default tolerance,
        // and last_seq_received clamped to 0 when uninitialized.
        let mut fresh = Regulator::with_params(1, 32, 48_000, -1.0);
        let fs = fresh.stats();
        assert_eq!(fs.packets_received, 0);
        assert_eq!(fs.packets_played, 0);
        assert_eq!(fs.glitches, 0);
        assert_eq!(fs.last_seq_received, 0);
        assert_eq!(fs.packets_rejected, 0);
        assert_eq!(fs.tolerance_ms, fresh.tolerance_ms());
        assert_eq!(fs.headroom_ms, consumer(&mut fresh).current_headroom);
    }

    /// Auto mode must not adjust tolerance until `AUTO_INIT_DURATION_MS` of
    /// relative time has elapsed, even if the underlying long-term stats look
    /// jittery enough to demand an adjustment.
    #[test]
    fn test_auto_tolerance_unchanged_during_init_duration() {
        let mut reg = Regulator::with_params(1, 32, 48_000, -1.0);
        let initial = consumer(&mut reg).tolerance_ms;
        assert!(consumer(&mut reg).auto_mode);

        // Inject long-term stats that would normally drive a tolerance bump.
        consumer(&mut reg).push_stats.long_term_max = 100.0;
        consumer(&mut reg).push_stats.long_term_std_dev = 25.0;
        consumer(&mut reg).pull_stats.long_term_max = 50.0;
        consumer(&mut reg).pull_stats.long_term_std_dev = 10.0;

        update_tolerance(&mut reg, 0.0);
        update_tolerance(&mut reg, 1500.0);
        update_tolerance(&mut reg, AUTO_INIT_DURATION_MS - 0.1);
        assert_eq!(
            consumer(&mut reg).tolerance_ms, initial,
            "tolerance must be unchanged before AUTO_INIT_DURATION_MS"
        );

        // Once we cross the threshold the same inputs cause an update.
        update_tolerance(&mut reg, AUTO_INIT_DURATION_MS + 1.0);
        assert_ne!(
            consumer(&mut reg).tolerance_ms, initial,
            "tolerance should react to long-term stats after init duration"
        );
    }

    /// Auto-mode tolerance must follow long-term stats: a noisy network should
    /// grow tolerance, a quiet network should shrink it, both clamped within
    /// `[fpp_duration_ms, AUTO_MAX_MS]`.
    #[test]
    fn test_auto_tolerance_recomputes_from_long_term_stats() {
        let mut reg = Regulator::with_params(1, 32, 48_000, -1.0);
        let initial = consumer(&mut reg).tolerance_ms;
        let fpp_duration_ms = reg.shared.fpp_duration_ms();

        // Phase 1: jittery push, calm pull — tolerance should rise sharply.
        consumer(&mut reg).push_stats.long_term_max = 80.0;
        consumer(&mut reg).push_stats.long_term_std_dev = 20.0;
        consumer(&mut reg).pull_stats.long_term_max = 10.0;
        consumer(&mut reg).pull_stats.long_term_std_dev = 2.0;
        update_tolerance(&mut reg, 7000.0); // past AUTO_INIT_DURATION_MS & headroom warmup
        let after_jitter = consumer(&mut reg).tolerance_ms;
        assert!(
            after_jitter > initial,
            "tolerance must grow with long-term jitter (init={initial}, after={after_jitter})"
        );
        assert!(after_jitter <= AUTO_MAX_MS);
        assert!(after_jitter >= fpp_duration_ms);

        // Phase 2: calm network — tolerance should fall back down.
        consumer(&mut reg).push_stats.long_term_max = 3.0;
        consumer(&mut reg).push_stats.long_term_std_dev = 1.0;
        consumer(&mut reg).pull_stats.long_term_max = 2.0;
        consumer(&mut reg).pull_stats.long_term_std_dev = 0.5;
        update_tolerance(&mut reg, 7100.0);
        let after_calm = consumer(&mut reg).tolerance_ms;
        assert!(
            after_calm < after_jitter,
            "tolerance must drop when jitter subsides (jitter={after_jitter}, calm={after_calm})"
        );
        assert!(after_calm >= fpp_duration_ms);

        // current_headroom should track the configured (positive) auto_headroom.
        assert!((consumer(&mut reg).current_headroom - consumer(&mut reg).auto_headroom).abs() < 1e-9);
    }

    /// Fixed (non-auto) tolerance mode must ignore the auto-tolerance machinery
    /// entirely, even when push/pull stats look noisy.
    #[test]
    fn test_fixed_tolerance_mode_does_not_auto_update() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 25.0);
        assert!(!consumer(&mut reg).auto_mode);
        assert_eq!(consumer(&mut reg).tolerance_ms, 25.0);

        consumer(&mut reg).push_stats.long_term_max = 200.0;
        consumer(&mut reg).push_stats.long_term_std_dev = 50.0;
        consumer(&mut reg).pull_stats.long_term_max = 100.0;
        consumer(&mut reg).pull_stats.long_term_std_dev = 20.0;

        update_tolerance(&mut reg, 10_000.0);
        assert_eq!(
            consumer(&mut reg).tolerance_ms, 25.0,
            "fixed-mode tolerance must remain at the configured value"
        );
    }

    /// `TimingStats::tick` should accumulate within a window without exposing
    /// long-term values, then promote them on window completion. The dedicated
    /// overrun/underrun counters must be independently mutable and reset.
    #[test]
    fn test_timing_stats_window_completion_records_long_term() {
        let mut stats = TimingStats::new(48_000, 32);
        assert!(stats.window > 0);

        // Drive partial window — long-term values stay zero.
        for _ in 0..(stats.window - 1) {
            let updated = stats.tick(5.0, 100.0);
            assert!(!updated, "tick before window completion must not promote");
        }
        assert_eq!(stats.long_term_count, 0);
        assert_eq!(stats.long_term_max, 0.0);

        // Final tick completes the window.
        let updated = stats.tick(5.0, 200.0);
        assert!(updated, "window-completing tick should return true");
        assert_eq!(stats.long_term_count, 1);
        assert!(stats.long_term_max >= 5.0);
        assert_eq!(stats.count, 0, "window state resets after promotion");

        // Out-of-range measurements should be dropped, not promoted.
        assert!(!stats.tick(0.0, 250.0));
        assert!(!stats.tick(-1.0, 251.0));
        assert!(!stats.tick(20_000.0, 252.0));

        // Independent overrun / underrun counters.
        stats.overruns += 3;
        stats.underruns += 2;

        let pre_reset_long_term = stats.long_term_count;
        assert!(pre_reset_long_term > 0);

        stats.reset();
        assert_eq!(stats.long_term_count, 0);
        assert_eq!(stats.long_term_max, 0.0);
        assert_eq!(stats.long_term_std_dev, 0.0);
        assert_eq!(stats.count, 0);
        assert_eq!(stats.overruns, 0);
        assert_eq!(stats.underruns, 0);
        assert_eq!(stats.last_max, 0.0);
    }

    /// `calc_auto()` returns the `AUTO_MAX_MS` ceiling whenever either long-term
    /// component is still zero, and otherwise `std_dev + max.min(AUTO_MAX_MS)`.
    /// The early-return guard and the clamped non-zero branch are exercised
    /// directly, since the auto-tolerance integration tests only ever feed it
    /// non-zero stats.
    #[test]
    fn test_timing_stats_calc_auto_zero_and_nonzero_branches() {
        let mut stats = TimingStats::new(48_000, 32);

        // Fresh stats: both long-term values are zero -> early-return ceiling.
        assert_eq!(stats.long_term_std_dev, 0.0);
        assert_eq!(stats.long_term_max, 0.0);
        assert_eq!(stats.calc_auto(), AUTO_MAX_MS);

        // Either component being zero alone still trips the guard.
        stats.long_term_std_dev = 0.0;
        stats.long_term_max = 40.0;
        assert_eq!(stats.calc_auto(), AUTO_MAX_MS);

        stats.long_term_std_dev = 10.0;
        stats.long_term_max = 0.0;
        assert_eq!(stats.calc_auto(), AUTO_MAX_MS);

        // Non-zero case returns std_dev + max (max below the cap is unchanged).
        stats.long_term_std_dev = 12.0;
        stats.long_term_max = 40.0;
        assert_eq!(stats.calc_auto(), 12.0 + 40.0);

        // A max above the cap is clamped to AUTO_MAX_MS before adding std_dev.
        stats.long_term_std_dev = 5.0;
        stats.long_term_max = AUTO_MAX_MS + 100.0;
        assert_eq!(stats.calc_auto(), 5.0 + AUTO_MAX_MS);
    }

    /// After the simple-average startup region ends
    /// (`long_term_count > WINDOW_DIVISOR * AUTO_HISTORY_WINDOW`), a completed
    /// window must fold its statistics into the long-term values via `ewma`
    /// rather than the running mean.
    #[test]
    fn test_timing_stats_long_term_uses_ewma_after_startup() {
        let mut stats = TimingStats::new(48_000, 32);
        let window = stats.window;

        // Jump past the startup region so the next completed window takes the
        // EWMA branch instead of the simple-average branch.
        let threshold = WINDOW_DIVISOR * AUTO_HISTORY_WINDOW as usize;
        stats.long_term_count = threshold + 1;
        let prev_std_dev = 7.0;
        let prev_max = 30.0;
        stats.long_term_std_dev = prev_std_dev;
        stats.long_term_max = prev_max;

        // Feed a full window of a constant elapsed value: the window's std_dev
        // is exactly 0 and its max equals that constant.
        let elapsed = 9.0;
        let mut completed = false;
        for i in 0..window {
            completed = stats.tick(elapsed, 100.0 + i as f64);
        }
        assert!(completed, "feeding `window` samples must complete a window");

        let expected_std_dev = TimingStats::ewma(prev_std_dev, 0.0);
        let expected_max = TimingStats::ewma(prev_max, elapsed);
        assert!(
            (stats.long_term_std_dev - expected_std_dev).abs() < 1e-9,
            "long_term_std_dev should follow EWMA ({}, expected {expected_std_dev})",
            stats.long_term_std_dev
        );
        assert!(
            (stats.long_term_max - expected_max).abs() < 1e-9,
            "long_term_max should follow EWMA ({}, expected {expected_max})",
            stats.long_term_max
        );
    }

    /// `depth()` uses `wrapping_sub`, so a read pointer near the top of the
    /// u16 range with a write pointer that has wrapped past `u16::MAX` must
    /// still report the small forward distance. `latency_ms()` then follows
    /// from that depth for a known packet format.
    #[test]
    fn test_depth_and_latency_handle_u16_wraparound() {
        let reg = Regulator::with_params(1, 128, 48_000, 5.0);

        // Write pointer wrapped around (now 2); read pointer near the top.
        let read: u16 = u16::MAX - 1; // 65534
        let write: u16 = 2;
        reg.shared.last_seq_in.store(write as i32, Ordering::Release);
        set_last_seq_out(&reg, Some(read));

        // 65534 -> 65535 -> 0 -> 1 -> 2 is a forward distance of 4 packets.
        assert_eq!(reg.depth(), write.wrapping_sub(read) as u32);
        assert_eq!(reg.depth(), 4);

        // latency = (depth * samples_per_packet / sample_rate) * 1000.
        // 4 * 128 / 48000 * 1000 ≈ 10.6667 ms.
        let expected_ms = (4.0_f32 * 128.0 / 48_000.0) * 1000.0;
        assert!(
            (reg.latency_ms() - expected_ms).abs() < 1e-3,
            "latency_ms ({}) should follow from the wrapped depth ({expected_ms})",
            reg.latency_ms()
        );
    }

    /// The trivial configuration getters must reflect the values the regulator
    /// was constructed with, including the auto-mode initial tolerance derived
    /// from `fpp * AUTO_INIT_VAL_FACTOR`.
    #[test]
    fn test_config_getters_reflect_constructed_values() {
        let fixed = Regulator::with_params(2, 256, 44_100, 18.0);
        assert_eq!(fixed.channels(), 2);
        assert_eq!(fixed.fpp(), 256);
        assert_eq!(fixed.tolerance_ms(), 18.0);

        let auto = Regulator::with_params(1, 64, 48_000, -1.0);
        assert_eq!(auto.channels(), 1);
        assert_eq!(auto.fpp(), 64);
        assert_eq!(auto.tolerance_ms(), 64.0 * AUTO_INIT_VAL_FACTOR);
    }

    /// The first packet of a stream adopts the peer's channel count rather
    /// than being validated against whatever this regulator happened to be
    /// constructed with. This only passes if the deinterleave stride, the
    /// `ChannelState` count, and the slot length all moved together — a
    /// partial adoption would either panic on a length mismatch or silently
    /// scramble channels.
    #[test]
    fn test_first_packet_adopts_peer_channel_count_and_plays_back_at_that_stride() {
        let reg = Regulator::with_params(2, 32, 48_000, 5.0);
        let original_fpp = reg.fpp();
        let original_tolerance = reg.tolerance_ms();

        // Peer sends 1 channel; this regulator was constructed for 2.
        let ramp: Vec<f32> = (0..reg.fpp()).map(|i| i as f32).collect();
        assert_eq!(reg.push_internal(0, 1, &ramp, 0.0), PushOutcome::Stored);
        assert_eq!(reg.channels(), 1, "first packet must adopt the peer's channel count");

        let mut out = vec![0.0f32; reg.fpp()];
        let real = reg.pop_internal(&mut out, 10.0); // past the 5ms tolerance
        assert!(real, "the adopted-stride packet must play back as real audio");
        assert_eq!(
            out, ramp,
            "deinterleave stride, ChannelState count, and slot length must all match the adopted count"
        );

        // Adoption is not a rebuild: fpp and the jitter policy are untouched.
        assert_eq!(reg.fpp(), original_fpp, "fpp must be unchanged by adoption");
        assert_eq!(
            reg.tolerance_ms(), original_tolerance,
            "adopt_channel_count must not touch the jitter policy"
        );
    }

    /// A connection fixes frames-per-packet and channel count when it is
    /// established, so a packet carrying any other number of samples cannot
    /// belong to this stream. `push` must reject it outright rather than fit
    /// it to the slot: a short packet padded or left partially written plays
    /// as a burst of garbage or silence inside otherwise good audio, and
    /// either size would still drag the write pointer forward onto a slot the
    /// read side then hands to the output.
    #[test]
    fn test_push_rejects_packets_that_are_not_exactly_one_packet_long() {
        let mut reg = Regulator::with_params(2, 64, 48_000, 5.0);
        let good = vec![0.5f32; reg.fpp() * reg.channels()];
        assert_eq!(reg.push_internal(10, 2, &good, 0.0), PushOutcome::Stored);
        assert_eq!(reg.shared.last_seq_in.load(Ordering::Acquire), 10);

        let wrong_sizes = [
            0,
            1,
            reg.fpp() * reg.channels() - reg.channels(), // one frame short
            reg.fpp() * reg.channels() - 1,                // one sample short
            reg.fpp() * reg.channels() + 1,                // one sample long
            reg.fpp() * reg.channels() * 2,                // double-length packet
        ];
        for len in wrong_sizes {
            let packet = vec![0.25f32; len];
            assert_eq!(
                reg.push_internal(11, 2, &packet, 1.0),
                PushOutcome::WrongPacketSize { expected: reg.fpp() * reg.channels(), got: len },
                "accepted a {len}-sample packet on a {}-sample stream",
                reg.fpp() * reg.channels()
            );
        }

        // Neither the ring nor the write pointer moved: the slot the rejected
        // packets targeted is still empty, and the newest sequence number is
        // still the last good packet's.
        assert_eq!(reg.shared.last_seq_in.load(Ordering::Acquire), 10);
        let slot = &mut reg.shared.slots[slot_index(11)];
        assert!(slot.read(11).is_none());
        let payload = slot.payload.get_mut();
        assert_eq!(payload.timestamp, 0.0);
        assert!(payload.data.iter().all(|&s| s == 0.0));
        assert_eq!(reg.stats().packets_rejected, wrong_sizes.len() as u64);

        // Channel count 0, or beyond MAX_CHANNELS, is rejected before the size
        // check even runs.
        let rejected_before = reg.stats().packets_rejected;
        assert_eq!(
            reg.push_internal(12, 0, &[], 2.0),
            PushOutcome::UnsupportedChannelCount { got: 0 }
        );
        assert_eq!(
            reg.push_internal(12, MAX_CHANNELS + 1, &[0.0], 2.0),
            PushOutcome::UnsupportedChannelCount { got: MAX_CHANNELS + 1 }
        );
        assert_eq!(reg.stats().packets_rejected, rejected_before + 2);

        // A packet claiming a channel count that differs from the count
        // already adopted for this stream is rejected outright, not
        // re-adopted — see `PushOutcome::ChannelCountChanged`.
        let mismatched = vec![0.5f32; reg.fpp()]; // 1ch-sized; this stream adopted 2ch
        assert_eq!(
            reg.push_internal(13, 1, &mismatched, 3.0),
            PushOutcome::ChannelCountChanged { adopted: 2, got: 1 }
        );
        assert_eq!(
            reg.shared.last_seq_in.load(Ordering::Acquire), 10,
            "a rejected packet must not move the write pointer"
        );
    }

    /// `push`'s explicit `channels` argument (rather than inferring it as
    /// `samples.len() / fpp`) exists to prevent exactly this ambiguity: 4
    /// channels at fpp=64 and 2 channels at fpp=128 both total 256 samples.
    /// With `fpp` fixed locally and `channels` read from the wire header, a
    /// packet claiming 4 channels on a 128-fpp stream must be rejected for
    /// its size — never silently reinterpreted as a valid 2-channel packet
    /// just because the sample count happens to match.
    #[test]
    fn test_push_channel_count_is_explicit_not_inferred_from_sample_count() {
        let reg = Regulator::with_params(2, 128, 48_000, 5.0);
        let first = vec![0.1f32; 256]; // 2ch * 128fpp
        assert_eq!(reg.push_internal(0, 2, &first, 0.0), PushOutcome::Stored);

        // Same total sample count as a valid 2ch*128fpp packet, but claiming
        // 4 channels — must be rejected for its size (4*128=512 != 256), not
        // accepted as if it were secretly the 2ch stride.
        let same_len_but_4ch = vec![0.2f32; 256];
        assert_eq!(
            reg.push_internal(1, 4, &same_len_but_4ch, 1.0),
            PushOutcome::WrongPacketSize { expected: 4 * 128, got: 256 }
        );
    }

    /// WEB-53 Bug 1: a reordered straggler must still publish its slot via the
    /// per-slot Release stamp, even though it must not advance `last_seq_in`.
    /// Without the stamp, the consumer would never observe the straggler's
    /// payload under the SPSC protocol.
    #[test]
    fn test_straggler_publishes_slot_without_advancing_write_pointer() {
        let reg = Regulator::with_params(1, 32, 48_000, 5.0);
        let samples_a = vec![0.25f32; 32];
        let samples_b = vec![0.75f32; 32];
        push_stored(&reg, 10, 1, &samples_a, 100.0);
        push_stored(&reg, 12, 1, &samples_a, 102.0);
        assert_eq!(reg.shared.last_seq_in.load(Ordering::Acquire), 12);

        // Straggler seq 11 — between last_seq_out (none) and last_seq_in.
        push_stored(&reg, 11, 1, &samples_b, 103.0);
        assert_eq!(
            reg.shared.last_seq_in.load(Ordering::Acquire),
            12,
            "straggler must not advance last_seq_in"
        );
        let payload = reg.shared.slots[slot_index(11)]
            .read(11)
            .expect("straggler must Release-publish its slot stamp");
        assert!(
            (payload.data[0] - 0.75).abs() < 1e-6,
            "straggler payload must be stored"
        );
    }

    /// PR #89 review: `push` and `pop` each used to create a `&mut` to the
    /// whole regulator, so a concurrent producer, consumer, and stats reader
    /// held overlapping references — undefined behavior even though every
    /// payload access was stamp-ordered. Natively that cannot fail a test;
    /// under Miri (`npm run test:miri`) it reports the aliasing violation or
    /// the data race it enables.
    #[test]
    #[cfg_attr(not(miri), ignore = "only meaningful under Miri: npm run test:miri")]
    fn test_concurrent_push_pop_and_stats_do_not_alias() {
        let reg = Regulator::with_params(2, 32, 48_000, 1.0);
        let fpp = reg.fpp();
        let packet = vec![0.5f32; fpp]; // mono: adoption runs concurrently with pop
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for seq in 0..40u16 {
                    push_stored(&reg, seq, 1, &packet, seq as f64);
                }
            });
            scope.spawn(|| {
                let mut out = vec![0.0f32; fpp * MAX_CHANNELS];
                for i in 0..40 {
                    reg.pop_internal(&mut out, 2.0 + i as f64);
                    let _ = reg.channels();
                }
            });
            scope.spawn(|| {
                for _ in 0..40 {
                    let _ = (reg.stats(), reg.depth(), reg.latency_ms(), reg.tolerance_ms());
                }
            });
        });
    }
}
