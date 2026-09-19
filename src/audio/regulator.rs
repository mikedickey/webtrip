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
//!   see [`Regulator::find_best_packet`].
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

use std::sync::atomic::{AtomicI32, Ordering};

/// Sentinel value for "no sequence number" (None)
const SEQ_NONE: i32 = -1;

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

/// A slot for storing an incoming packet.
///
/// A JackTrip connection fixes frames-per-packet and channel count when it is
/// established, so every packet belonging to the stream carries exactly
/// [`Regulator::samples_per_packet`] samples. `data` is sized to that once, at
/// configure time, and is always full: a packet of any other size is rejected
/// before it reaches a slot ([`Regulator::push`]). That leaves no
/// partially-filled state to track and no allocation on the audio path.
struct PacketSlot {
    /// Arrival timestamp in milliseconds
    timestamp: f64,
    /// One packet of audio (interleaved channels), `samples_per_packet` long
    data: Vec<f32>,
    /// The sequence number whose data currently occupies this slot, or
    /// `None` if never written or just reset. Because every `NUM_SLOTS`th
    /// sequence number aliases onto the same slot (see the module header),
    /// `data` alone doesn't say *which* packet it belongs to — this is the
    /// identity check the read side verifies before trusting it.
    seq: Option<u16>,
}

impl PacketSlot {
    fn new(samples_per_packet: usize) -> Self {
        Self {
            timestamp: 0.0,
            data: vec![0.0; samples_per_packet],
            seq: None,
        }
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
    /// Number of skipped packets
    pub skipped: u64,
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
pub struct Regulator {
    // Configuration
    num_channels: usize,
    sample_rate: u32,
    fpp: usize,
    samples_per_packet: usize,

    // Burg algorithm state
    burg: BurgAlgorithm,
    packets_in_past: usize,
    up_to_now: usize,
    beyond_now: usize,

    // Per-channel state
    channels: Vec<ChannelState>,

    // Packet storage (circular buffer by sequence number)
    slots: Vec<Option<PacketSlot>>,

    // Sequence tracking
    /// Last sequence number received (SEQ_NONE = not initialized)
    /// Uses AtomicI32 for thread-safe access between push (writer) and pop (reader) threads
    last_seq_in: AtomicI32,
    last_seq_out: Option<u16>,
    last_stashed: Option<(u16, usize)>,

    // Timing (internal clock using performance.now() equivalent)
    start_time_ms: f64,
    last_pop_time_ms: f64,
    push_stats: TimingStats,
    pull_stats: TimingStats,

    // Auto-adaptive tolerance
    auto_mode: bool,
    tolerance_ms: f64,
    auto_headroom: f64,
    current_headroom: f64,
    skip_auto_headroom: bool,
    auto_headroom_start_time: f64,

    // Statistics
    packet_count: u64,
    plc_packet_count: u64,
    packets_rejected: u64,
    skipped: u64,
    last_skipped: u64,
    last_glitches: u64,
    stats_glitches: u64,
    last_max_latency: f64,
    stats_max_latency: f64,

    // Crossfade buffers
    fade_up: Vec<f32>,
    fade_down: Vec<f32>,

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
/// count from the peer's first packet ([`Regulator::adopt_channel_count`]).
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
    /// reading (see [`Regulator::adopt_channel_count`]'s concurrency note).
    ChannelCountChanged { adopted: usize, got: usize },
    /// `samples.len()` was not exactly `channels * fpp`.
    WrongPacketSize { expected: usize, got: usize },
}

impl Regulator {
    /// Get current time in milliseconds.
    fn now_ms() -> f64 {
        #[cfg(target_arch = "wasm32")]
        {
            // Use js_sys::Date::now() which returns milliseconds since epoch
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

    /// Create a new Regulator with default parameters.
    /// Use `configure()` to set the proper parameters.
    pub fn new() -> Self {
        // Start with defaults that will be overridden by configure()
        Self::with_params(1, 128, 48000, -1.0)
    }

    /// Create a new Regulator with specific parameters.
    ///
    /// # Arguments
    /// * `channels` - Number of audio channels
    /// * `fpp` - Frames (samples) per packet per channel
    /// * `sample_rate` - Sample rate in Hz
    /// * `tolerance_ms` - Initial tolerance in ms, or negative for auto mode
    pub fn with_params(channels: usize, fpp: usize, sample_rate: u32, tolerance_ms: f64) -> Self {
        let samples_per_packet = fpp * channels;

        // Calculate history based on FPP
        let packets_in_past = if fpp < HIST_FPP {
            HIST * (HIST_FPP / fpp)
        } else if fpp > HIST_FPP * 2 {
            1
        } else {
            HIST
        };

        let up_to_now = packets_in_past * fpp;
        let beyond_now = (packets_in_past + 1) * fpp;

        // Determine auto mode and initial tolerance
        let (auto_mode, auto_headroom, initial_tolerance) = if tolerance_ms <= 0.0 {
            let headroom = if tolerance_ms == -500.0 {
                -1.0 // Variable headroom
            } else {
                tolerance_ms.abs()
            };
            (true, headroom, fpp as f64 * AUTO_INIT_VAL_FACTOR)
        } else {
            (false, tolerance_ms, tolerance_ms)
        };

        // Create crossfade ramps
        let fade_up: Vec<f32> = (0..fpp)
            .map(|i| i as f32 / fpp as f32)
            .collect();
        let fade_down: Vec<f32> = fade_up.iter().map(|&x| 1.0 - x).collect();

        // Create channel states
        let channel_states: Vec<ChannelState> = (0..channels)
            .map(|_| ChannelState::new(fpp, up_to_now, packets_in_past))
            .collect();

        // Create packet slots (pre-allocated to avoid allocations in audio path)
        let mut slots = Vec::with_capacity(NUM_SLOTS);
        for _ in 0..NUM_SLOTS {
            slots.push(Some(PacketSlot::new(samples_per_packet)));
        }

        Self {
            num_channels: channels,
            sample_rate,
            fpp,
            samples_per_packet,

            burg: BurgAlgorithm::new(up_to_now),
            packets_in_past,
            up_to_now,
            beyond_now,

            channels: channel_states,
            slots,

            last_seq_in: AtomicI32::new(SEQ_NONE),
            last_seq_out: None,
            last_stashed: None,

            start_time_ms: 0.0,
            last_pop_time_ms: 0.0,
            push_stats: TimingStats::new(sample_rate, fpp),
            pull_stats: TimingStats::new(sample_rate, fpp),

            auto_mode,
            tolerance_ms: initial_tolerance,
            auto_headroom,
            current_headroom: if auto_headroom < 0.0 { 0.0 } else { auto_headroom },
            skip_auto_headroom: true,
            auto_headroom_start_time: 6000.0,

            packet_count: 0,
            plc_packet_count: 0,
            packets_rejected: 0,
            skipped: 0,
            last_skipped: 0,
            last_glitches: 0,
            stats_glitches: 0,
            last_max_latency: 0.0,
            stats_max_latency: 0.0,

            fade_up,
            fade_down,
            last_was_glitch: false,
        }
    }

    /// Configure the regulator parameters.
    ///
    /// # Arguments
    /// * `channels` - Number of audio channels
    /// * `fpp` - Frames (samples) per packet per channel
    /// * `sample_rate` - Sample rate in Hz
    /// * `tolerance_ms` - Initial tolerance in ms, or negative for auto mode
    pub fn configure(&mut self, channels: usize, fpp: usize, sample_rate: u32, tolerance_ms: f64) {
        *self = Self::with_params(channels, fpp, sample_rate, tolerance_ms);
    }

    /// Push a received packet into the buffer (with explicit timestamp).
    /// This method performs NO heap allocations on the `Stored` path — all
    /// buffers involved are pre-allocated. Adoption
    /// ([`adopt_channel_count`](Self::adopt_channel_count)) is the one
    /// exception, and it only ever runs once per stream, on the first packet.
    ///
    /// # Arguments
    /// * `seq_num` - Packet sequence number (u16 wraps at 65535)
    /// * `channels` - The channel count this packet claims to carry, from the
    ///   wire header. Taken explicitly rather than inferred from
    ///   `samples.len() / fpp`: e.g. 4ch×64 and 2ch×128 are both 256 samples,
    ///   and inference would silently adopt the wrong stride.
    /// * `samples` - Interleaved audio samples
    /// * `now_ms` - Current timestamp in milliseconds
    ///
    /// # Returns
    /// See [`PushOutcome`].
    fn push_internal(&mut self, seq_num: u16, channels: usize, samples: &[f32], now_ms: f64) -> PushOutcome {
        if channels == 0 || channels > MAX_CHANNELS {
            self.packets_rejected += 1;
            return PushOutcome::UnsupportedChannelCount { got: channels };
        }

        // Frames-per-packet is fixed locally for the life of this regulator;
        // channel count is instead adopted from the peer's first packet
        // (below), which is why this checks `channels * self.fpp` rather than
        // `self.samples_per_packet` — the latter may not reflect `channels`
        // yet on the very packet that is about to establish it. Either way,
        // every packet on the wire must carry exactly that many samples:
        // anything else did not come from this stream's configuration, and
        // there is no honest way to place it in the timeline. A short packet
        // would play as a dropout in the middle of good audio, and a long one
        // would be silently truncated. Reject it before it can occupy a slot
        // or move the write pointer.
        let expected = channels * self.fpp;
        if samples.len() != expected {
            self.packets_rejected += 1;
            return PushOutcome::WrongPacketSize { expected, got: samples.len() };
        }

        let current = self.last_seq_in.load(Ordering::Acquire);

        if current == SEQ_NONE {
            // First packet of a stream: adopt its channel count rather than
            // rejecting a mismatch against whatever this regulator happened
            // to be constructed with.
            if channels != self.num_channels {
                self.adopt_channel_count(channels);
            }
            self.start_time_ms = now_ms;
        } else if channels != self.num_channels {
            // A later packet disagreeing with the already-adopted count is a
            // peer bug or an unsignaled renegotiation, not something to
            // re-adopt into — see `PushOutcome::ChannelCountChanged`.
            self.packets_rejected += 1;
            return PushOutcome::ChannelCountChanged { adopted: self.num_channels, got: channels };
        }

        let relative_now = now_ms - self.start_time_ms;

        // Store the audio unconditionally, even for a packet the ordering check
        // below rejects: it is still the freshest thing that belongs in this
        // slot, and a straggler the read side has not passed yet is playable.
        if let Some(ref mut slot) = self.slots[slot_index(seq_num)] {
            slot.timestamp = relative_now;
            slot.data.copy_from_slice(samples);
            slot.seq = Some(seq_num);
        }

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

    /// Rebuild the channel-derived state to match a peer's first packet.
    ///
    /// Only ever called from [`push_internal`](Self::push_internal) while
    /// `last_seq_in == SEQ_NONE`, i.e. before this stream has stored anything.
    /// Rebuilds `num_channels`, `samples_per_packet`, the per-channel
    /// Burg/ring state (via `ChannelState::new`), and every slot's `data`
    /// buffer — resized **and zeroed**, for the same reason `reset()` zeroes
    /// slot data: a slot left over from a previous, differently-sized stream
    /// must not leak into this one as a burst of garbage.
    ///
    /// Left untouched: `burg`, `up_to_now`/`beyond_now` (these depend only on
    /// `fpp`, which does not change here), the fade ramps, `push_stats`/
    /// `pull_stats`, and the whole auto-tolerance/headroom policy. That is
    /// what distinguishes this from [`configure`](Self::configure), which is
    /// `*self = Self::with_params(...)` and would reset the jitter policy on
    /// every stream's first packet.
    ///
    /// # Concurrency
    ///
    /// This runs on the network thread, inside `push`, and resizes
    /// [`NUM_SLOTS`] slot `Vec`s and the per-channel state while the audio
    /// thread may concurrently be inside `pop`. It is sound only because it
    /// happens strictly before the `Release` store of `last_seq_in` in
    /// `push_internal`: `pop_internal`'s `SEQ_NONE` early return touches none
    /// of the state rebuilt here — it reads only `start_time_ms`/
    /// `tolerance_ms`, fills the output with zeros, and returns. Any code path
    /// past that early return is reached only after an `Acquire` load has
    /// observed this call's later `Release` store, so it always sees the
    /// fully rebuilt state, never a partial one.
    ///
    /// This does **not** cover a `pop` still running the *previous* stream's
    /// real (non-`SEQ_NONE`) path when `reset()` re-arms `SEQ_NONE` and a new
    /// peer then adopts a different channel count here — that `pop` could read
    /// a slot or channel buffer already resized out from under it.
    /// `WebTripSession::disconnect` orders `close().await` → `stop_capture()`
    /// → `reset()`, but the worklet's final render call is not synchronously
    /// joinable with that ordering, so this window is believed narrow but not
    /// proven closed. This is the same open concurrency question documented on
    /// `SharedPtr::as_mut`; it is tracked there, not claimed solved here.
    fn adopt_channel_count(&mut self, channels: usize) {
        self.num_channels = channels;
        self.samples_per_packet = self.fpp * channels;

        self.channels = (0..channels)
            .map(|_| ChannelState::new(self.fpp, self.up_to_now, self.packets_in_past))
            .collect();

        for slot in &mut self.slots {
            if let Some(ref mut s) = slot {
                s.data.clear();
                s.data.resize(self.samples_per_packet, 0.0);
            }
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
    pub fn push(&mut self, sequence: u16, channels: usize, samples: &[f32]) -> PushOutcome {
        let now_ms = Self::now_ms();
        self.push_internal(sequence, channels, samples, now_ms)
    }

    /// Pop samples for playback (internal with explicit timestamp).
    ///
    /// # Arguments
    /// * `output` - Buffer to write samples to (interleaved)
    /// * `now_ms` - Current timestamp in milliseconds
    ///
    /// # Returns
    /// `true` if real packet data was output, `false` if concealment was used
    fn pop_internal(&mut self, output: &mut [f32], now_ms: f64) -> bool {
        let relative_now = now_ms - self.start_time_ms;
        let last_seq_in_raw = self.last_seq_in.load(Ordering::Acquire);
        let last_seq_in = last_seq_in_raw as u16;

        // Return silence during startup
        if last_seq_in_raw == SEQ_NONE || relative_now < self.tolerance_ms {
            output.fill(0.0);
            return false;
        }

        // Check for underrun (no new packets)
        if let Some(last_out) = self.last_seq_out {
            if last_seq_in == last_out {
                return self.handle_underrun(output, relative_now);
            }
        }

        // Find best packet to output (NO allocations)
        let result = self.find_best_packet(last_seq_in, relative_now);

        match result {
            Some(PacketDecision::Packet { seq, slot_idx }) => {
                // Deinterleave from slot directly into channel tmp buffers (NO allocation)
                if let Some(ref slot) = self.slots[slot_idx] {
                    for (ch, channel) in self.channels.iter_mut().enumerate() {
                        for s in 0..self.fpp {
                            channel.tmp_buf[s] = slot.data[s * self.num_channels + ch];
                        }
                    }
                }

                // Process with Burg algorithm
                self.process_burg(false);
                self.interleave_output(output);

                self.last_seq_out = Some(seq);
                self.packet_count += 1;
                true
            }
            Some(PacketDecision::ConcealSkippedPacket { skipped }) => {
                self.pull_stats.overruns += skipped;
                self.output_concealment(output);
                false
            }
            None => {
                self.handle_underrun(output, relative_now)
            }
        }
    }

    /// Pop samples for playback.
    ///
    /// # Arguments
    /// * `output` - Buffer to write samples to (interleaved)
    ///
    /// # Returns
    /// `true` if real packet data was output, `false` if concealment was used
    pub fn pop(&mut self, output: &mut [f32]) -> bool {
        let now_ms = Self::now_ms();
        
        // Track time between pops for statistics
        if self.last_pop_time_ms > 0.0 {
            let elapsed = now_ms - self.last_pop_time_ms;
            self.pull_stats.tick(elapsed, now_ms - self.start_time_ms);
        }
        self.last_pop_time_ms = now_ms;
        
        self.pop_internal(output, now_ms)
    }

    /// Handle an underrun (missing packet).
    fn handle_underrun(&mut self, output: &mut [f32], now: f64) -> bool {
        self.pull_stats.underruns += 1;

        // Check for stuck client (no packets for a long time)
        if let Some(last_out) = self.last_seq_out {
            let slot_idx = slot_index(last_out);
            if let Some(ref slot) = self.slots[slot_idx] {
                if now - slot.timestamp > 10000.0 {
                    // Stuck - output silence
                    output.fill(0.0);
                    return false;
                }
            }
        }

        // Good underrun - use prediction
        self.output_concealment(output);
        false
    }

    fn output_concealment(&mut self, output: &mut [f32]) {
        for channel in &mut self.channels {
            channel.tmp_buf.fill(0.0);
        }
        self.process_burg(true);
        self.interleave_output(output);
    }

    fn interleave_output(&self, output: &mut [f32]) {
        for (ch, channel) in self.channels.iter().enumerate() {
            for s in 0..self.fpp {
                let idx = s * self.num_channels + ch;
                if idx < output.len() {
                    output[idx] = channel.output_now_packet[s];
                }
            }
        }
    }

    /// Find the best packet to output based on timing.
    fn find_best_packet(&mut self, last_seq_in: u16, now: f64) -> Option<PacketDecision> {
        if let Some((seq, slot_idx)) = self.last_stashed {
            self.last_stashed = None;
            return Some(PacketDecision::Packet { seq, slot_idx });
        }

        let start_seq = if let Some(last_out) = self.last_seq_out {
            last_out.wrapping_add(1)
        } else {
            last_seq_in
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
            self.last_seq_out = None;
            return None;
        }

        let mut skipped = 0u64;
        let mut first_good_skipped: Option<(u16, usize, f64)> = None;

        // Find the best candidate (NO allocations - just return indices)
        let mut best_candidate: Option<(u16, usize, f64)> = None;

        for i in (0..new_pkts).rev() {
            let seq = last_seq_in.wrapping_sub(i as u16);
            let slot_idx = slot_index(seq);

            let timestamp = match &self.slots[slot_idx] {
                Some(slot) if slot.seq == Some(seq) => slot.timestamp,
                _ => continue,
            };

            // Skip packets that arrived too early (out of order)
            if let Some(last_out) = self.last_seq_out {
                let last_out_idx = slot_index(last_out);
                if let Some(ref last_slot) = &self.slots[last_out_idx] {
                    if timestamp < last_slot.timestamp
                        && last_slot.timestamp - timestamp > self.tolerance_ms
                    {
                        continue;
                    }
                }
            }

            // Calculate skipped packet count (recalculate for each candidate, don't accumulate)
            if let Some(last_out) = self.last_seq_out {
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
                    self.skipped += skipped;
                    self.last_stashed = Some((seq, slot_idx));
                    self.update_push_stats(seq, timestamp, now);
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
            self.update_push_stats(seq, timestamp, now);
            return Some(PacketDecision::Packet { seq, slot_idx });
        }

        None
    }

    /// Update push statistics when pulling a packet.
    fn update_push_stats(&mut self, seq: u16, timestamp: f64, now: f64) {
        let Some(last_out) = self.last_seq_out else {
            return;
        };

        let fpp_duration_ms = 1000.0 * self.fpp as f64 / self.sample_rate as f64;

        // Estimate previous packet timing (use wrapping arithmetic for u16)
        let pkts = seq.wrapping_sub(last_out.wrapping_add(1)) as usize;
        let last_out_idx = slot_index(last_out);

        if let Some(ref last_slot) = &self.slots[last_out_idx] {
            let prev_time = last_slot.timestamp + (pkts as f64 + 1.0) * fpp_duration_ms;
            if prev_time < timestamp {
                let elapsed = timestamp - prev_time;
                let updated = self.push_stats.tick(elapsed, now);

                if updated && self.push_stats.long_term_count % WINDOW_DIVISOR == 0 {
                    self.update_tolerance(now);
                }
            }
        }
    }

    /// Update auto-adaptive tolerance.
    fn update_tolerance(&mut self, now: f64) {
        if !self.auto_mode || now < AUTO_INIT_DURATION_MS {
            return;
        }

        let total_glitches = self.pull_stats.underruns + self.pull_stats.overruns;
        let total_skipped = self.skipped;
        let new_glitches = total_glitches.saturating_sub(self.last_glitches);
        let new_skipped = total_skipped.saturating_sub(self.last_skipped);
        self.last_glitches = total_glitches;
        self.last_skipped = total_skipped;

        // Skip warmup period
        if now <= self.auto_headroom_start_time {
            self.last_max_latency  = 0.0;  // ignore during warmup
            self.stats_max_latency = 0.0;
            self.update_headroom(0, 0);
        } else {
            self.last_max_latency = self.stats_max_latency;
            self.stats_max_latency = 0.0;
            self.update_headroom(new_glitches, new_skipped);
        }
    }

    /// Update headroom based on glitch counts.
    fn update_headroom(&mut self, glitches: u64, _skipped: u64) {
        let fpp_duration_ms = 1000.0 * self.fpp as f64 / self.sample_rate as f64;

        if self.auto_headroom < 0.0 {
            // Variable headroom mode
            let glitches_allowed = 
                ((AUTO_HEADROOM_GLITCH_TOLERANCE * self.sample_rate as f64) / self.fpp as f64)
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

        new_tolerance = new_tolerance.clamp(fpp_duration_ms, AUTO_MAX_MS);
        self.tolerance_ms = new_tolerance;
    }

    /// Process audio with Burg algorithm for PLC.
    fn process_burg(&mut self, glitch: bool) {
        let primed = self.plc_packet_count > self.packets_in_past as u64;

        for channel in &mut self.channels {
            // Copy real packet data
            for s in 0..self.fpp {
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
                    for s in 0..self.fpp {
                        if offset + s < channel.prediction.len() {
                            channel.prediction[offset + s] = channel.tmp_buf[s];
                        }
                    }
                    offset += self.fpp;
                }
            }

            // Perform prediction on glitch
            if glitch {
                // Copy predicted past into prediction buffer
                for i in 0..self.packets_in_past {
                    for s in 0..self.fpp {
                        let idx = i * self.fpp + s;
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
                for s in 0..self.fpp {
                    let idx = self.up_to_now + s;
                    channel.predicted_now_packet[s] =
                        channel.prediction.get(idx).copied().unwrap_or(0.0);
                }
            }

            // Generate output with crossfade
            for s in 0..self.fpp {
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
            for s in 0..self.fpp {
                let idx = self.beyond_now + s;
                channel.future_predicted_packet[s] =
                    channel.prediction.get(idx).copied().unwrap_or(0.0);
            }
        }

        self.last_was_glitch = glitch;
        self.plc_packet_count += 1;
    }

    /// Get current statistics.
    pub fn stats(&self) -> RegulatorStats {
        let total_glitches = self.pull_stats.underruns + self.pull_stats.overruns;
        let last_seq_raw = self.last_seq_in.load(Ordering::Relaxed);
        let last_seq = if last_seq_raw == SEQ_NONE { 0 } else { last_seq_raw as u16 };
        RegulatorStats {
            tolerance_ms: self.tolerance_ms,
            headroom_ms: self.current_headroom,
            max_latency_ms: self.last_max_latency,
            glitches: total_glitches.saturating_sub(self.stats_glitches),
            skipped: self.skipped.saturating_sub(self.last_skipped),
            packets_received: self.packet_count, // Use packet_count for total packets received
            packets_played: self.packet_count,
            last_seq_received: last_seq,
            packets_rejected: self.packets_rejected,
        }
    }

    /// Reset the regulator state.
    pub fn reset(&mut self) {
        self.last_seq_in.store(SEQ_NONE, Ordering::Release);
        self.last_seq_out = None;
        // A stash left over from the previous connection would otherwise be
        // returned on the first pop after reconnect, pinning `last_seq_out` to
        // a stale sequence number and triggering thousands of spurious PLCs
        // until the new stream's sequence numbers caught up.
        self.last_stashed = None;
        self.packet_count = 0;
        self.plc_packet_count = 0;
        self.packets_rejected = 0;
        self.skipped = 0;
        self.last_skipped = 0;
        self.last_glitches = 0;
        self.stats_glitches = 0;
        self.last_max_latency = 0.0;
        self.stats_max_latency = 0.0;
        self.last_was_glitch = false;
        self.tolerance_ms = if self.auto_mode {
            self.fpp as f64 * AUTO_INIT_VAL_FACTOR
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
        self.start_time_ms = 0.0;
        self.last_pop_time_ms = 0.0;

        // Reset timing stats state
        self.pull_stats.reset();
        self.push_stats.reset();

        // Reset slots without deallocating. The audio is zeroed, not just the
        // timestamp: after a reset the next stream can jump the write pointer
        // over slots this one filled, and those slots are readable again as
        // soon as they fall inside the new stream's span. Silence there is a
        // dropout; the previous connection's audio is a burst of garbage.
        for slot in &mut self.slots {
            if let Some(ref mut s) = slot {
                s.timestamp = 0.0;
                s.data.fill(0.0);
                s.seq = None;
            }
        }

        for channel in &mut self.channels {
            channel.tmp_buf.fill(0.0);
            channel.real_now_packet.fill(0.0);
            channel.predicted_now_packet.fill(0.0);
            channel.output_now_packet.fill(0.0);
            channel.future_predicted_packet.fill(0.0);
            for ring_pkt in &mut channel.packet_ring {
                ring_pkt.fill(0.0);
            }
            for pred_pkt in &mut channel.predicted_past {
                pred_pkt.fill(0.0);
            }
            channel.prediction.fill(0.0);
            channel.coeffs.fill(0.0);
            channel.train_data.fill(0.0);
            channel.ring_wptr = channel.ring_size / 2;
        }
    }

    /// Get the current tolerance in milliseconds.
    pub fn tolerance_ms(&self) -> f64 {
        self.tolerance_ms
    }

    /// Get the frames per packet.
    pub fn fpp(&self) -> usize {
        self.fpp
    }

    /// Get the number of channels.
    pub fn channels(&self) -> usize {
        self.num_channels
    }

    /// Check if the regulator has been initialized (received first packet).
    pub fn is_initialized(&self) -> bool {
        self.last_seq_in.load(Ordering::Acquire) != SEQ_NONE
    }

    /// Get current buffer depth (number of packets buffered).
    pub fn depth(&self) -> u32 {
        let write_raw = self.last_seq_in.load(Ordering::Acquire);
        if write_raw == SEQ_NONE {
            return 0;
        }
        let write = write_raw as u16;
        let read = self.last_seq_out.unwrap_or(write);
        // Use wrapping arithmetic to handle sequence number wraparound (u16)
        write.wrapping_sub(read) as u32
    }

    /// Get approximate latency in milliseconds.
    pub fn latency_ms(&self) -> f32 {
        let depth = self.depth();
        // `depth` counts packets (sequence numbers), each `fpp` frames long —
        // not `samples_per_packet`, which also carries the channel count and
        // would double-count latency for anything wider than mono.
        let total_frames = depth * self.fpp as u32;
        (total_frames as f32 / self.sample_rate as f32) * 1000.0
    }
}

impl Default for Regulator {
    fn default() -> Self {
        Self::new()
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
        let slot = reg.slots[slot_index(seq)]
            .as_mut()
            .expect("slots are pre-allocated at configure time");
        slot.timestamp = timestamp;
        slot.data.copy_from_slice(samples);
        slot.seq = Some(seq);
    }

    /// Push a packet a test expects the regulator to store, asserting the
    /// outcome. Tests that drive `push_internal` for setup would otherwise
    /// discard the `#[must_use]` result, so a push that starts getting
    /// rejected (wrong size, unadopted channel count) would leave the
    /// regulator empty and surface as a confusing failure further down —
    /// or as no failure at all.
    fn push_stored(reg: &mut Regulator, seq: u16, channels: usize, samples: &[f32], now_ms: f64) {
        assert_eq!(
            reg.push_internal(seq, channels, samples, now_ms),
            PushOutcome::Stored,
            "setup push of seq {seq} ({channels}ch) must be stored"
        );
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
        let mut reg = Regulator::with_params(1, 128, 48000, 50.0);

        // Create test samples
        let samples: Vec<f32> = (0..128).map(|i| (i as f32 * 0.01).sin()).collect();
        
        // Test near u16::MAX (65535) to verify wraparound works
        let near_max: u16 = u16::MAX - 2;
        
        // Push packets near wraparound boundary with proper timing
        push_stored(&mut reg, near_max, 1, &samples, 0.0);
        push_stored(&mut reg, near_max.wrapping_add(1), 1, &samples, 3.0);
        push_stored(&mut reg, near_max.wrapping_add(2), 1, &samples, 6.0); // This wraps to 0
        push_stored(&mut reg, 0, 1, &samples, 9.0); // Already wrapped
        push_stored(&mut reg, 1, 1, &samples, 12.0);
        
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
        assert!(reg.last_seq_out.is_some());
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
        let mut reg = Regulator::with_params(1, 128, 48_000, 5.0);
        let samples = vec![0.5f32; reg.fpp];
        let dt = 1000.0 * reg.fpp as f64 / reg.sample_rate as f64;
        let mut out = vec![0.0f32; reg.fpp];

        // A stream that is playing out normally.
        let mut t = 0.0;
        for i in 0..10u16 {
            push_stored(&mut reg, 100 + i, 1, &samples, t);
            t += dt;
        }
        let mut pop_t = 10.0;
        for _ in 0..10 {
            reg.pop_internal(&mut out, pop_t);
            pop_t += dt;
        }
        assert_eq!(reg.last_seq_out, Some(109), "stream should be playing out");

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
            push_stored(&mut reg, first_resumed + k, 1, &samples, now);
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
        assert_eq!(
            reg.last_seq_out,
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
        let last_packet = vec![0.1f32; reg.fpp];
        let stale_packet = vec![0.9f32; reg.fpp];

        reg.start_time_ms = 0.0;
        reg.last_seq_out = Some(last_seq);
        reg.last_seq_in.store(candidate_seq as i32, Ordering::Release);

        plant_packet(&mut reg, last_seq, 40.0, &last_packet);
        plant_packet(&mut reg, stale_seq, 41.0, &stale_packet); // candidate_seq never planted

        let mut output = vec![0.0f32; reg.fpp];
        let result = reg.pop_internal(&mut output, 50.0);

        assert!(
            !result,
            "a slot whose stored seq doesn't match the candidate must not be \
             played back as real audio"
        );
        assert_eq!(
            reg.last_seq_out,
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
        let history_packets = reg.packets_in_past;
        let history: Vec<f32> = (0..reg.up_to_now)
            .map(|i| (i as f32 * 0.1).sin())
            .collect();

        {
            let channel = &mut reg.channels[0];
            for (i, packet) in channel.predicted_past.iter_mut().enumerate() {
                let start = i * reg.fpp;
                let end = start + reg.fpp;
                packet.copy_from_slice(&history[start..end]);
            }
        }

        reg.process_burg(true);
        assert!(
            reg.channels[0]
                .output_now_packet
                .iter()
                .all(|sample| sample.abs() < 1e-6),
            "PLC should stay muted before the predictor is primed"
        );

        reg.plc_packet_count = history_packets as u64 + 1;
        reg.process_burg(true);
        assert!(
            reg.channels[0]
                .output_now_packet
                .iter()
                .any(|sample| sample.abs() > 1e-6),
            "PLC should emit predicted audio once the PLC iteration counter is primed"
        );
        assert_eq!(reg.packet_count, 0);
        assert_eq!(reg.plc_packet_count, history_packets as u64 + 2);
    }

    #[test]
    fn test_skipped_packet_outputs_concealment_before_stashed_real_packet() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 10.0);
        let history_packets = reg.packets_in_past;
        let fpp_duration_ms = 1000.0 * reg.fpp as f64 / reg.sample_rate as f64;
        let history: Vec<f32> = (0..reg.up_to_now)
            .map(|i| (i as f32 * 0.1).sin())
            .collect();
        let packet = vec![0.25f32; reg.fpp];
        let last_packet = vec![0.1f32; reg.fpp];
        let last_seq = 10u16;
        let good_seq = last_seq.wrapping_add(2);

        {
            let channel = &mut reg.channels[0];
            for (i, predicted) in channel.predicted_past.iter_mut().enumerate() {
                let start = i * reg.fpp;
                let end = start + reg.fpp;
                predicted.copy_from_slice(&history[start..end]);
            }
        }
        reg.plc_packet_count = history_packets as u64 + 1;

        reg.start_time_ms = 0.0;
        reg.last_seq_out = Some(last_seq);
        reg.last_seq_in.store(good_seq as i32, Ordering::Release);

        plant_packet(&mut reg, last_seq, 40.0, &last_packet);
        plant_packet(&mut reg, good_seq, 45.0, &packet);

        let mut concealment = vec![0.0f32; reg.fpp];
        let concealment_result = reg.pop_internal(&mut concealment, 50.0);
        assert!(!concealment_result);
        assert_eq!(reg.pull_stats.overruns, 1);
        assert_eq!(reg.last_stashed.map(|(seq, _)| seq), Some(good_seq));
        assert_eq!(reg.last_seq_out, Some(last_seq));
        assert!(
            concealment.iter().any(|sample| sample.abs() > 1e-6),
            "skipped packets should trigger PLC output before the real packet is replayed"
        );

        let mut real_output = vec![0.0f32; reg.fpp];
        let real_result = reg.pop_internal(&mut real_output, 50.0 + fpp_duration_ms);
        assert!(real_result);
        assert_eq!(reg.last_stashed, None);
        assert_eq!(reg.last_seq_out, Some(good_seq));
        assert_eq!(reg.packet_count, 1);
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
        let fpp_duration_ms = 1000.0 * reg.fpp as f64 / reg.sample_rate as f64;
        let stale_packet = vec![0.25f32; reg.fpp];
        let fresh_packet = vec![0.5f32; reg.fpp];
        let last_seq = 10u16;
        let stale_seq = last_seq.wrapping_add(1);
        let fresh_seq = last_seq.wrapping_add(2);

        reg.start_time_ms = 0.0;
        reg.last_seq_out = Some(last_seq);
        reg.last_seq_in.store(fresh_seq as i32, Ordering::Release);

        // Previously-played packet, reference point for the out-of-order check.
        let silent_packet = vec![0.0f32; reg.fpp];
        plant_packet(&mut reg, last_seq, 40.0, &silent_packet);
        // Stale but valid packet: misses tolerance at now=60 (41 + 10 < 60),
        // so the scan records it as `first_good_skipped` instead of playing it.
        plant_packet(&mut reg, stale_seq, 41.0, &stale_packet);
        // Fresh packet within tolerance (55 + 10 >= 60); playing it would skip
        // exactly one good packet, which triggers the deferral.
        plant_packet(&mut reg, fresh_seq, 55.0, &fresh_packet);

        let mut output = vec![0.0f32; reg.fpp];
        let first_result = reg.pop_internal(&mut output, 60.0);
        assert!(
            first_result,
            "deferral should play the stale packet as real audio, not concealment"
        );
        assert_eq!(reg.last_seq_out, Some(stale_seq));
        assert_eq!(reg.last_stashed.map(|(seq, _)| seq), Some(fresh_seq));
        assert_eq!(reg.skipped, 0, "no packet may be counted as skipped");
        assert_eq!(reg.pull_stats.overruns, 0, "no glitch may be recorded");
        assert_eq!(reg.packet_count, 1);

        let second_result = reg.pop_internal(&mut output, 60.0 + fpp_duration_ms);
        assert!(second_result);
        assert_eq!(reg.last_stashed, None);
        assert_eq!(reg.last_seq_out, Some(fresh_seq));
        assert_eq!(reg.packet_count, 2);
        assert_eq!(reg.skipped, 0);
        assert_eq!(reg.pull_stats.overruns, 0);
    }

    /// After a full push/pop cycle that accumulates state (stats, sequence
    /// numbers, timing), `reset()` must scrub the regulator back to a
    /// freshly-constructed state. This covers the field-by-field cleanup
    /// required for safe stream reconnection.
    #[test]
    fn test_reset_clears_state_after_active_stream() {
        let mut reg = Regulator::with_params(2, 32, 48_000, 5.0);
        let samples = vec![0.25f32; reg.samples_per_packet];

        // Drive a small stream through the regulator.
        push_stored(&mut reg, 0, 2, &samples, 0.0);
        push_stored(&mut reg, 1, 2, &samples, 2.0);
        let mut out = vec![0.0f32; reg.samples_per_packet];
        let _ = reg.pop_internal(&mut out, 10.0);
        let _ = reg.pop_internal(&mut out, 12.0);
        let _ = reg.pop_internal(&mut out, 14.0); // forces underrun -> PLC

        assert!(reg.is_initialized());
        assert!(reg.last_seq_out.is_some());
        assert!(reg.packet_count > 0);
        assert!(reg.pull_stats.underruns > 0);

        reg.reset();

        assert!(!reg.is_initialized(), "last_seq_in should be cleared");
        assert_eq!(reg.last_seq_out, None);
        assert_eq!(reg.last_stashed, None);
        assert_eq!(reg.packet_count, 0);
        assert_eq!(reg.plc_packet_count, 0);
        assert_eq!(reg.skipped, 0);
        assert_eq!(reg.last_skipped, 0);
        assert_eq!(reg.last_glitches, 0);
        assert_eq!(reg.stats_glitches, 0);
        assert_eq!(reg.last_max_latency, 0.0);
        assert_eq!(reg.stats_max_latency, 0.0);
        assert!(!reg.last_was_glitch);
        assert_eq!(reg.start_time_ms, 0.0);
        assert_eq!(reg.last_pop_time_ms, 0.0);
        assert_eq!(reg.pull_stats.underruns, 0);
        assert_eq!(reg.pull_stats.overruns, 0);
        assert_eq!(reg.push_stats.long_term_count, 0);
        assert_eq!(reg.depth(), 0);

        // Channels should be zeroed and ring write pointer should be re-centered.
        for channel in &reg.channels {
            assert!(channel.tmp_buf.iter().all(|s| *s == 0.0));
            assert!(channel.real_now_packet.iter().all(|s| *s == 0.0));
            assert!(channel.output_now_packet.iter().all(|s| *s == 0.0));
            assert_eq!(channel.ring_wptr, channel.ring_size / 2);
        }

        // A reconnect on a new stream with a different channel count must be
        // able to adopt again — reset() re-arms SEQ_NONE, which is the only
        // gate on adoption.
        let mono = vec![0.5f32; reg.fpp];
        let outcome = reg.push_internal(0, 1, &mono, 0.0);
        assert_eq!(outcome, PushOutcome::Stored);
        assert_eq!(reg.channels(), 1, "reconnect must re-adopt the new peer's channel count");
        assert_eq!(reg.samples_per_packet, reg.fpp);
    }

    /// The 7def5fc race fix: a `last_stashed` slot left from a prior connection
    /// must be cleared by `reset()`. Otherwise the first pop after reconnect
    /// would replay a stale packet and pin `last_seq_out` to an old sequence
    /// number, triggering a storm of spurious PLC events while the new stream
    /// catches up.
    #[test]
    fn test_reset_clears_stashed_packet_to_avoid_stale_pop() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 5.0);
        let samples = vec![0.5f32; reg.fpp];

        // Simulate a prior connection that landed with a stashed packet:
        // primer connection seq 7, stash for "next-good" seq 9.
        reg.last_seq_in.store(9, Ordering::Release);
        reg.last_seq_out = Some(7);
        reg.last_stashed = Some((9, slot_index(9)));
        plant_packet(&mut reg, 9, 100.0, &samples);

        reg.reset();

        assert_eq!(
            reg.last_stashed, None,
            "stashed packet from prior connection must be cleared"
        );
        assert!(!reg.is_initialized());
        assert_eq!(reg.last_seq_out, None);

        // After reset, the first pop on a fresh stream should return silence
        // (startup), not the stale stashed buffer.
        push_stored(&mut reg, 0, 1, &samples, 0.0);
        let mut out = vec![0.0f32; reg.fpp];
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
        let samples = vec![0.5f32; reg.fpp];

        push_stored(&mut reg, 0, 1, &samples, 0.0);
        let mut out = vec![0.0f32; reg.fpp];
        let r1 = reg.pop_internal(&mut out, 10.0);
        assert!(r1);
        assert_eq!(reg.last_seq_out, Some(0));

        // Skip seq 1, 2 — push seq 3 directly. With `skipped = 2`, the
        // regulator should treat this as a glitch, conceal, stash, and bump
        // overruns.
        push_stored(&mut reg, 3, 1, &samples, 20.0);
        let r2 = reg.pop_internal(&mut out, 40.0);
        assert!(!r2, "skipped-gap path returns concealment (false)");
        assert_eq!(reg.pull_stats.overruns, 2, "overrun counter tracks skip distance");
        assert_eq!(reg.skipped, 2, "skipped counter tracks skip distance");
        // The good packet must have been stashed for replay on the next pop.
        assert_eq!(reg.last_stashed.map(|(s, _)| s), Some(3));

        // Next pop should consume the stashed real packet, no new overruns.
        let overruns_before = reg.pull_stats.overruns;
        let r3 = reg.pop_internal(&mut out, 42.0);
        assert!(r3, "stashed packet replay should be a real-packet pop");
        assert_eq!(reg.last_seq_out, Some(3));
        assert_eq!(reg.pull_stats.overruns, overruns_before);
    }

    /// PLC must kick in after consecutive misses and disengage as soon as a
    /// real packet arrives. The result boolean should reflect this transition.
    #[test]
    fn test_plc_engages_on_underrun_and_stops_with_real_packets() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 5.0);
        let samples = vec![0.5f32; reg.fpp];

        push_stored(&mut reg, 0, 1, &samples, 0.0);
        let mut out = vec![0.0f32; reg.fpp];
        let r0 = reg.pop_internal(&mut out, 10.0);
        assert!(r0);
        let plc_after_first = reg.plc_packet_count;
        let packets_after_first = reg.packet_count;

        // Three consecutive underrun pops should each invoke PLC.
        for i in 0..3 {
            let t = 12.0 + i as f64 * 2.0;
            let result = reg.pop_internal(&mut out, t);
            assert!(!result, "missing packet at t={t} should produce PLC output");
        }
        assert_eq!(reg.pull_stats.underruns, 3);
        assert_eq!(
            reg.plc_packet_count,
            plc_after_first + 3,
            "process_burg should run for each concealed packet"
        );
        assert_eq!(
            reg.packet_count, packets_after_first,
            "packet_count must not grow while concealing"
        );

        // Real packet arrives — next pop should be real audio again.
        push_stored(&mut reg, 1, 1, &samples, 25.0);
        let r_resume = reg.pop_internal(&mut out, 30.0);
        assert!(r_resume, "PLC must disengage once a real packet is available");
        assert_eq!(reg.last_seq_out, Some(1));
        assert_eq!(reg.packet_count, packets_after_first + 1);
        let underruns_after = reg.pull_stats.underruns;

        // No further underruns when consumption keeps pace.
        push_stored(&mut reg, 2, 1, &samples, 32.0);
        let r_next = reg.pop_internal(&mut out, 38.0);
        assert!(r_next);
        assert_eq!(reg.pull_stats.underruns, underruns_after);
    }

    /// `depth()` and `latency_ms()` should report the gap between the newest
    /// pushed and most recently popped sequence numbers. Each push must grow
    /// depth by exactly one, and the latency formula must follow from depth.
    /// Pops drain depth monotonically until it returns to zero.
    #[test]
    fn test_depth_and_latency_reflect_buffered_packets() {
        let mut reg = Regulator::with_params(2, 64, 48_000, 5.0);
        assert_eq!(reg.depth(), 0, "depth starts at zero before first packet");
        assert_eq!(reg.latency_ms(), 0.0);

        let samples = vec![0.1f32; reg.samples_per_packet];

        // Before any pop, last_seq_out is None so depth treats read == write.
        push_stored(&mut reg, 0, 2, &samples, 0.0);
        push_stored(&mut reg, 1, 2, &samples, 2.0);
        assert_eq!(reg.depth(), 0, "with no pop, read==write so depth is zero");

        // First pop sets last_seq_out; the in-flight buffer is consumed.
        let mut out = vec![0.0f32; reg.samples_per_packet];
        let r = reg.pop_internal(&mut out, 10.0);
        assert!(r);
        let seq_after_first_pop = reg.last_seq_out.expect("pop must set last_seq_out");

        // Push more packets without popping — depth grows by one per push.
        for (i, t) in (1u16..=3).zip([12.0_f64, 14.0, 16.0]) {
            let seq = seq_after_first_pop.wrapping_add(i);
            push_stored(&mut reg, seq, 2, &samples, t);
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
            (depth as f32 * reg.fpp as f32 / reg.sample_rate as f32) * 1000.0;
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
        let samples = vec![0.0f32; reg.fpp];
        push_stored(&mut reg, 42, 1, &samples, 0.0);
        push_stored(&mut reg, 43, 1, &samples, 2.0);

        let mut out = vec![0.0f32; reg.fpp];
        let _ = reg.pop_internal(&mut out, 15.0);
        let _ = reg.pop_internal(&mut out, 17.0);
        let _ = reg.pop_internal(&mut out, 19.0); // underrun

        let s = reg.stats();
        assert_eq!(s.tolerance_ms, reg.tolerance_ms);
        assert_eq!(s.headroom_ms, reg.current_headroom);
        assert_eq!(s.max_latency_ms, reg.last_max_latency);
        assert_eq!(s.packets_received, reg.packet_count);
        assert_eq!(s.packets_played, reg.packet_count);
        assert_eq!(
            s.last_seq_received, 43,
            "last_seq_received should match the highest seq pushed"
        );
        let expected_glitches =
            (reg.pull_stats.underruns + reg.pull_stats.overruns) - reg.stats_glitches;
        assert_eq!(s.glitches, expected_glitches);

        // Stats on a fresh regulator: zeroed counters, default tolerance,
        // and last_seq_received clamped to 0 when uninitialized.
        let fresh = Regulator::with_params(1, 32, 48_000, -1.0);
        let fs = fresh.stats();
        assert_eq!(fs.packets_received, 0);
        assert_eq!(fs.packets_played, 0);
        assert_eq!(fs.glitches, 0);
        assert_eq!(fs.skipped, 0);
        assert_eq!(fs.last_seq_received, 0);
        assert_eq!(fs.packets_rejected, 0);
        assert_eq!(fs.tolerance_ms, fresh.tolerance_ms);
        assert_eq!(fs.headroom_ms, fresh.current_headroom);
    }

    /// Auto mode must not adjust tolerance until `AUTO_INIT_DURATION_MS` of
    /// relative time has elapsed, even if the underlying long-term stats look
    /// jittery enough to demand an adjustment.
    #[test]
    fn test_auto_tolerance_unchanged_during_init_duration() {
        let mut reg = Regulator::with_params(1, 32, 48_000, -1.0);
        let initial = reg.tolerance_ms;
        assert!(reg.auto_mode);

        // Inject long-term stats that would normally drive a tolerance bump.
        reg.push_stats.long_term_max = 100.0;
        reg.push_stats.long_term_std_dev = 25.0;
        reg.pull_stats.long_term_max = 50.0;
        reg.pull_stats.long_term_std_dev = 10.0;

        reg.update_tolerance(0.0);
        reg.update_tolerance(1500.0);
        reg.update_tolerance(AUTO_INIT_DURATION_MS - 0.1);
        assert_eq!(
            reg.tolerance_ms, initial,
            "tolerance must be unchanged before AUTO_INIT_DURATION_MS"
        );

        // Once we cross the threshold the same inputs cause an update.
        reg.update_tolerance(AUTO_INIT_DURATION_MS + 1.0);
        assert_ne!(
            reg.tolerance_ms, initial,
            "tolerance should react to long-term stats after init duration"
        );
    }

    /// Auto-mode tolerance must follow long-term stats: a noisy network should
    /// grow tolerance, a quiet network should shrink it, both clamped within
    /// `[fpp_duration_ms, AUTO_MAX_MS]`.
    #[test]
    fn test_auto_tolerance_recomputes_from_long_term_stats() {
        let mut reg = Regulator::with_params(1, 32, 48_000, -1.0);
        let initial = reg.tolerance_ms;
        let fpp_duration_ms = 1000.0 * reg.fpp as f64 / reg.sample_rate as f64;

        // Phase 1: jittery push, calm pull — tolerance should rise sharply.
        reg.push_stats.long_term_max = 80.0;
        reg.push_stats.long_term_std_dev = 20.0;
        reg.pull_stats.long_term_max = 10.0;
        reg.pull_stats.long_term_std_dev = 2.0;
        reg.update_tolerance(7000.0); // past AUTO_INIT_DURATION_MS & headroom warmup
        let after_jitter = reg.tolerance_ms;
        assert!(
            after_jitter > initial,
            "tolerance must grow with long-term jitter (init={initial}, after={after_jitter})"
        );
        assert!(after_jitter <= AUTO_MAX_MS);
        assert!(after_jitter >= fpp_duration_ms);

        // Phase 2: calm network — tolerance should fall back down.
        reg.push_stats.long_term_max = 3.0;
        reg.push_stats.long_term_std_dev = 1.0;
        reg.pull_stats.long_term_max = 2.0;
        reg.pull_stats.long_term_std_dev = 0.5;
        reg.update_tolerance(7100.0);
        let after_calm = reg.tolerance_ms;
        assert!(
            after_calm < after_jitter,
            "tolerance must drop when jitter subsides (jitter={after_jitter}, calm={after_calm})"
        );
        assert!(after_calm >= fpp_duration_ms);

        // current_headroom should track the configured (positive) auto_headroom.
        assert!((reg.current_headroom - reg.auto_headroom).abs() < 1e-9);
    }

    /// Fixed (non-auto) tolerance mode must ignore the auto-tolerance machinery
    /// entirely, even when push/pull stats look noisy.
    #[test]
    fn test_fixed_tolerance_mode_does_not_auto_update() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 25.0);
        assert!(!reg.auto_mode);
        assert_eq!(reg.tolerance_ms, 25.0);

        reg.push_stats.long_term_max = 200.0;
        reg.push_stats.long_term_std_dev = 50.0;
        reg.pull_stats.long_term_max = 100.0;
        reg.pull_stats.long_term_std_dev = 20.0;

        reg.update_tolerance(10_000.0);
        assert_eq!(
            reg.tolerance_ms, 25.0,
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
        let mut reg = Regulator::with_params(1, 128, 48_000, 5.0);

        // Write pointer wrapped around (now 2); read pointer near the top.
        let read: u16 = u16::MAX - 1; // 65534
        let write: u16 = 2;
        reg.last_seq_in.store(write as i32, Ordering::Release);
        reg.last_seq_out = Some(read);

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
        let mut reg = Regulator::with_params(2, 32, 48_000, 5.0);
        let original_fpp = reg.fpp();
        let original_tolerance = reg.tolerance_ms();

        // Peer sends 1 channel; this regulator was constructed for 2.
        let ramp: Vec<f32> = (0..reg.fpp).map(|i| i as f32).collect();
        assert_eq!(reg.push_internal(0, 1, &ramp, 0.0), PushOutcome::Stored);
        assert_eq!(reg.channels(), 1, "first packet must adopt the peer's channel count");

        let mut out = vec![0.0f32; reg.fpp];
        let real = reg.pop_internal(&mut out, 10.0); // past the 5ms tolerance
        assert!(real, "the adopted-stride packet must play back as real audio");
        assert_eq!(
            out, ramp,
            "deinterleave stride, ChannelState count, and slot length must all match the adopted count"
        );

        // Adoption is not `configure()`: fpp and the jitter policy are untouched.
        assert_eq!(reg.fpp(), original_fpp, "fpp must be unchanged by adoption");
        assert_eq!(
            reg.tolerance_ms(), original_tolerance,
            "adopt_channel_count must not touch the jitter policy, unlike configure()"
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
        let good = vec![0.5f32; reg.samples_per_packet];
        assert_eq!(reg.push_internal(10, 2, &good, 0.0), PushOutcome::Stored);
        assert_eq!(reg.last_seq_in.load(Ordering::Acquire), 10);

        let wrong_sizes = [
            0,
            1,
            reg.samples_per_packet - reg.num_channels, // one frame short
            reg.samples_per_packet - 1,                // one sample short
            reg.samples_per_packet + 1,                // one sample long
            reg.samples_per_packet * 2,                // double-length packet
        ];
        for len in wrong_sizes {
            let packet = vec![0.25f32; len];
            assert_eq!(
                reg.push_internal(11, 2, &packet, 1.0),
                PushOutcome::WrongPacketSize { expected: reg.samples_per_packet, got: len },
                "accepted a {len}-sample packet on a {}-sample stream",
                reg.samples_per_packet
            );
        }

        // Neither the ring nor the write pointer moved: the slot the rejected
        // packets targeted is still empty, and the newest sequence number is
        // still the last good packet's.
        assert_eq!(reg.last_seq_in.load(Ordering::Acquire), 10);
        let slot = reg.slots[slot_index(11)]
            .as_ref()
            .expect("slots are pre-allocated at configure time");
        assert_eq!(slot.timestamp, 0.0);
        assert!(slot.data.iter().all(|&s| s == 0.0));
        assert_eq!(reg.packets_rejected, wrong_sizes.len() as u64);

        // Channel count 0, or beyond MAX_CHANNELS, is rejected before the size
        // check even runs.
        let rejected_before = reg.packets_rejected;
        assert_eq!(
            reg.push_internal(12, 0, &[], 2.0),
            PushOutcome::UnsupportedChannelCount { got: 0 }
        );
        assert_eq!(
            reg.push_internal(12, MAX_CHANNELS + 1, &[0.0], 2.0),
            PushOutcome::UnsupportedChannelCount { got: MAX_CHANNELS + 1 }
        );
        assert_eq!(reg.packets_rejected, rejected_before + 2);

        // A packet claiming a channel count that differs from the count
        // already adopted for this stream is rejected outright, not
        // re-adopted — see `PushOutcome::ChannelCountChanged`.
        let mismatched = vec![0.5f32; reg.fpp]; // 1ch-sized; this stream adopted 2ch
        assert_eq!(
            reg.push_internal(13, 1, &mismatched, 3.0),
            PushOutcome::ChannelCountChanged { adopted: 2, got: 1 }
        );
        assert_eq!(
            reg.last_seq_in.load(Ordering::Acquire), 10,
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
        let mut reg = Regulator::with_params(2, 128, 48_000, 5.0);
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
}
