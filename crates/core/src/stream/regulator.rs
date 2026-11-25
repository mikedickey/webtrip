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
//! - **Slot space** is `num_slots` entries deep ([`slot_index`]), where
//!   [`num_slots`] is chosen per stream from its adopted fpp. It answers
//!   *storage*: where does this packet's audio live, and is it still there?
//!
//! The ring holds a constant *time horizon* rather than a constant packet
//! count: a budget of [`SLOT_FRAME_BUDGET`] frames (4096 packets at the
//! reference fpp of 128, ~10.9 s at 48 kHz). `num_slots(fpp)` is the largest
//! power of two whose slots fit that budget, capped at [`MAX_SLOTS`]:
//!
//! - **A power of two**, so `seq % num_slots` stays continuous across the
//!   `u16` wrap (a non-divisor of 65536 would jump at the wrap).
//! - **The cap** keeps the ring at or under a quarter of sequence space,
//!   inside [`seq_is_newer`]'s half-space window.
//!
//! Memory is therefore at most 16 MiB at 8 channels whatever the fpp, instead
//! of growing with it (128 MiB at fpp 1024 with a fixed 4096 slots). The
//! horizon is ~10.9 s at any fpp of 32 or more, and ~5.5 s at 16, where the cap
//! binds. The slot array itself is allocated once at [`MAX_SLOTS`] entries
//! with empty payloads; only the first `num_slots` are ever sized.
//!
//! Sequence space is at least 4x larger than the ring, so the seam between
//! them has two rules:
//!
//! - Every `num_slots`th sequence number aliases onto the same slot, so a slot
//!   holds only the most recent packet congruent to its index. Reading one is
//!   meaningful only for a sequence number already known to be within
//!   `num_slots` of the write pointer.
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
use std::sync::atomic::{fence, AtomicI32, AtomicU64, AtomicUsize, Ordering};

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
/// Frames the slot ring may hold, whatever the fpp: 4096 packets at the
/// reference fpp, which is ~10.9 s at 48 kHz. See the module header.
const SLOT_FRAME_BUDGET: usize = 4096 * HIST_FPP;
/// Most slots the ring ever has. The array is preallocated at this size; the
/// cap keeps the ring within a quarter of the `u16` sequence space.
const MAX_SLOTS: usize = 16384;
/// Fewest frames per packet a stream may carry. A local mirror of
/// `limits::MIN_FPP` (this module has no `crate::` imports); a test pins the two.
pub(crate) const MIN_FPP: usize = 16;
/// Most frames per packet a stream may carry. A local mirror of
/// `limits::MAX_FPP`; pinned by the same test as [`MIN_FPP`].
pub(crate) const MAX_FPP: usize = 1024;
/// Longest history the Burg model trains on, in samples: `packets_in_past *
/// fpp` never exceeds this for any fpp in `[MIN_FPP, MAX_FPP]`.
const UP_TO_NOW_MAX: usize = if 2 * 2 * HIST_FPP > MAX_FPP { 2 * 2 * HIST_FPP } else { MAX_FPP };
/// Maximum channels a single stream may claim. A local mirror of
/// `limits::MAX_WIRE_CHANNELS`, not an import of it: this module deliberately has
/// no `crate::` imports (see the module header) so a native host can reuse it
/// against a different wire format with its own limit.
pub(crate) const MAX_CHANNELS: usize = 8;
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

/// Slots in the ring for a stream of `fpp` frames per packet: the largest
/// power of two that fits [`SLOT_FRAME_BUDGET`], capped at [`MAX_SLOTS`].
fn num_slots(fpp: usize) -> usize {
    let fitting = (SLOT_FRAME_BUDGET / fpp.max(1)).max(1);
    // Round down to a power of two.
    let pow2 = 1usize << (usize::BITS - 1 - fitting.leading_zeros());
    pow2.min(MAX_SLOTS)
}

/// Map a sequence number onto its slot in a ring of `num_slots` slots.
///
/// Lossy by construction: every `num_slots`th sequence number lands on the same
/// slot, so the packet found there is the one asked for only when that sequence
/// number is within `num_slots` of the write pointer. Callers establish that
/// first — see the module header.
#[inline]
fn slot_index(seq: u16, num_slots: usize) -> usize {
    seq as usize % num_slots
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

/// Past packets the Burg model trains on, so that its history is about
/// `HIST * HIST_FPP` samples whatever the packet size. Whole packets only: a
/// fpp that does not divide `HIST_FPP` truncates, which merely shortens the
/// history (power-of-two fpps divide exactly, which is why they are the ones
/// a sender picks).
fn packets_in_past(fpp: usize) -> usize {
    if fpp < HIST_FPP {
        HIST * (HIST_FPP / fpp.max(1))
    } else if fpp > HIST_FPP * 2 {
        1
    } else {
        HIST
    }
}

/// Fill the crossfade ramps for packets of `fpp` frames.
fn fill_fades(fade_up: &mut [f32], fade_down: &mut [f32], fpp: usize) {
    for (i, (up, down)) in fade_up.iter_mut().zip(fade_down.iter_mut()).enumerate().take(fpp) {
        *up = i as f32 / fpp as f32;
        *down = 1.0 - *up;
    }
}

/// Samples the frame FIFO needs to serve callbacks of `callback_frames` when
/// a peer may announce packets of up to `max(fpp, MAX_FPP)` frames: one
/// callback's worth plus the packet pulled to top it up, at the widest stream.
fn fifo_samples(callback_frames: usize, fpp: usize) -> usize {
    (callback_frames + fpp.max(MAX_FPP)) * MAX_CHANNELS
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
    /// Reset state for AR coefficients (only the first `input_size` entries are meaningful)
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
        Self::with_capacity(size, size)
    }

    /// Create an instance for `size` input samples whose buffers can later be
    /// resized up to `capacity` with [`set_size`](Self::set_size), without
    /// allocating.
    pub fn with_capacity(capacity: usize, size: usize) -> Self {
        let capacity = capacity.max(size);
        let mut burg = Self {
            order: 0,
            input_size: 0,
            ak: vec![0.0; capacity],
            ak_reset: vec![0.0; capacity],
            f: vec![0.0; capacity],
            b: vec![0.0; capacity],
        };
        burg.set_size(size);
        burg
    }

    /// Re-target the model at `size` input samples, within the capacity it was
    /// built with: sets the order and re-initializes the `ak_reset` prefix.
    /// Never allocates.
    pub fn set_size(&mut self, size: usize) {
        debug_assert!(size <= self.ak.len(), "Burg size {size} exceeds capacity {}", self.ak.len());
        let size = size.min(self.ak.len());
        self.order = size.saturating_sub(1);
        self.input_size = size;
        self.ak_reset[..size].fill(0.0);
        if size > 0 {
            self.ak_reset[0] = 1.0;
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
        self.ak[..self.input_size].copy_from_slice(&self.ak_reset[..self.input_size]);

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
///
/// Every buffer is sized once, for the largest packet a peer may announce
/// ([`MAX_FPP`]) and the longest history ([`UP_TO_NOW_MAX`]), so adopting a
/// peer's fpp ([`ConsumerState::configure_fpp`]) never allocates. A stream
/// uses a prefix of each: per-packet buffers `[..fpp]`, history buffers
/// `[..up_to_now]` (`packets_in_past` chunks of `fpp`).
struct ChannelState {
    /// Temporary buffer for incoming packet samples
    tmp_buf: Vec<f32>,
    /// Ring buffer of past packets, as `ring_size` consecutive chunks of `fpp`
    packet_ring: Vec<f32>,
    /// Ring buffer write pointer, in chunks
    ring_wptr: usize,
    /// Ring buffer size, in chunks
    ring_size: usize,
    /// Real samples from current packet
    real_now_packet: Vec<f32>,
    /// Predicted samples for current packet (when missing)
    predicted_now_packet: Vec<f32>,
    /// Output samples (after blending)
    output_now_packet: Vec<f32>,
    /// Future predicted packet (for crossfade)
    future_predicted_packet: Vec<f32>,
    /// History of predicted packets, oldest first, `ring_size` chunks of `fpp`
    predicted_past: Vec<f32>,
    /// Buffer for prediction (past + future)
    prediction: Vec<f32>,
    /// AR coefficients
    coeffs: Vec<f32>,
    /// Pre-allocated buffer for Burg training (avoids allocation in audio path)
    train_data: Vec<f32>,
}

impl ChannelState {
    /// Create channel state able to hold any stream within the fpp range, at
    /// least up to `fpp` frames per packet. Starts configured for `fpp` with
    /// `packets_in_past` chunks of history.
    fn new(fpp: usize, packets_in_past: usize) -> Self {
        let packet_cap = fpp.max(MAX_FPP);
        let history_cap = UP_TO_NOW_MAX.max(packets_in_past * fpp);

        Self {
            tmp_buf: vec![0.0; packet_cap],
            packet_ring: vec![0.0; history_cap],
            ring_wptr: packets_in_past / 2,
            ring_size: packets_in_past,
            real_now_packet: vec![0.0; packet_cap],
            predicted_now_packet: vec![0.0; packet_cap],
            output_now_packet: vec![0.0; packet_cap],
            future_predicted_packet: vec![0.0; packet_cap],
            predicted_past: vec![0.0; history_cap],
            prediction: vec![0.0; history_cap + 2 * packet_cap],
            coeffs: vec![0.0; history_cap.saturating_sub(1)],
            train_data: vec![0.0; history_cap],
        }
    }

    /// Adopt a new history geometry. The buffers are untouched: this is called
    /// only on a stream's first packet, when they are fresh or were cleared.
    fn configure(&mut self, packets_in_past: usize) {
        self.ring_wptr = packets_in_past / 2;
        self.ring_size = packets_in_past;
    }

    /// Push the first `fpp` samples of `tmp_buf` to the ring buffer.
    fn ring_buffer_push(&mut self, fpp: usize) {
        let at = self.ring_wptr * fpp;
        self.packet_ring[at..at + fpp].copy_from_slice(&self.tmp_buf[..fpp]);
        self.ring_wptr = (self.ring_wptr + 1) % self.ring_size;
    }

    /// Lay the real-packet history into the front of `prediction`, oldest
    /// first. The oldest packet is the one `ring_wptr` will overwrite next.
    fn real_past_into_prediction(&mut self, fpp: usize) {
        let split = self.ring_wptr * fpp;
        let end = self.ring_size * fpp;
        let older = end - split;
        self.prediction[..older].copy_from_slice(&self.packet_ring[split..end]);
        self.prediction[older..end].copy_from_slice(&self.packet_ring[..split]);
    }
}

/// Copy channel `ch` out of `interleaved` (an `n`-channel frame sequence) into
/// `dst`, stopping at whichever runs out first.
fn deinterleave_channel(interleaved: &[f32], n: usize, ch: usize, dst: &mut [f32]) {
    if n == 1 {
        let len = dst.len().min(interleaved.len());
        dst[..len].copy_from_slice(&interleaved[..len]);
        return;
    }
    for (d, &s) in dst.iter_mut().zip(interleaved.iter().skip(ch).step_by(n)) {
        *d = s;
    }
}

/// Copy `src` into channel `ch` of `interleaved` (an `n`-channel frame
/// sequence), stopping at whichever runs out first.
fn interleave_channel(src: &[f32], n: usize, ch: usize, interleaved: &mut [f32]) {
    if n == 1 {
        let len = src.len().min(interleaved.len());
        interleaved[..len].copy_from_slice(&src[..len]);
        return;
    }
    for (d, &s) in interleaved.iter_mut().skip(ch).step_by(n).zip(src) {
        *d = s;
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
    /// Samples in one statistics window at `fpp`: an eighth of a second of packets.
    fn window_for(sample_rate: u32, fpp: usize) -> usize {
        (((sample_rate as usize) / fpp.max(1)) / WINDOW_DIVISOR).max(1)
    }

    /// `data` is preallocated for the smallest packet a peer may announce
    /// ([`MIN_FPP`], the longest window), so [`set_window`](Self::set_window)
    /// never allocates.
    fn new(sample_rate: u32, fpp: usize) -> Self {
        let window = Self::window_for(sample_rate, fpp);
        Self {
            window,
            count: 0,
            accumulator: 0.0,
            min: f64::MAX,
            max: f64::MIN,
            data: vec![0.0; window.max(Self::window_for(sample_rate, MIN_FPP))],
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

    /// Re-size the window for packets of `fpp` frames and restart the
    /// statistics, within the capacity `new` allocated.
    fn set_window(&mut self, sample_rate: u32, fpp: usize) {
        self.reset();
        self.window = Self::window_for(sample_rate, fpp).min(self.data.len());
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
/// slot ([`Regulator::push_at`]). That leaves no partially-filled state to track
/// and no allocation on the audio path.
///
/// # Publication
///
/// `seq` is the per-slot publication stamp, used as a seqlock. The producer
/// ([`store`](Self::store)) clears `seq`, writes the payload, then stores the
/// new `seq` with [`Ordering::Release`]. The consumer ([`read`](Self::read))
/// loads `seq` with [`Ordering::Acquire`], copies, and loads `seq` again: the
/// copy counts only if both loads show the `seq` it expects. This is
/// independent of the stream-wide [`Shared::last_seq_in`] pointer: a reordered
/// straggler must still publish its slot without advancing that pointer.
///
/// # Tolerated race
///
/// The payload is plain data, so a producer that laps the ring onto the slot
/// the consumer is copying — a whole ring (`num_slots` packets, ~10.9 s at
/// fpp >= 32/48 kHz) ahead of a copy in progress — races that copy. This is a
/// deliberate exception to the crate's no-data-race rule, taken because the
/// lap is out of reach of today's transports and callback timing, and the
/// alternative (atomic samples) costs code for no practical gain:
///
/// - On every target we build for, aligned `f32`/`f64` accesses do not tear,
///   so the overlapping copy yields a mix of whole old and new samples, and
///   the stamp reload in `read` discards it. The consumer conceals instead.
/// - The formal hazard: because the race is undefined behavior, the compiler
///   may assume the payload is not written during the copy, and could in
///   principle sink the copy's loads past the stamp reload (an `Acquire`
///   fence keeps later loads from moving earlier, not earlier loads from
///   moving later), defeating the check. LLVM treats fences as full barriers
///   and does not do this today.
///
/// Making the samples `AtomicU32` (with `Relaxed` accesses) would remove the
/// race entirely if either assumption stops holding.
struct PacketSlot {
    payload: UnsafeCell<SlotPayload>,
    /// Sequence number occupying this slot, or [`SEQ_NONE`] if empty or
    /// mid-write. Because every `num_slots`th sequence number aliases onto the
    /// same slot (see the module header), `data` alone doesn't say *which*
    /// packet it belongs to — this is the identity check the read side
    /// verifies before and after trusting it.
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
    /// Only the producer may call this, and no reference from an earlier call
    /// may still be live. The consumer must not be reading this slot, other than in the
    /// tolerated race documented on [`PacketSlot`].
    #[allow(clippy::mut_from_ref)]
    unsafe fn payload_mut(&self) -> &mut SlotPayload {
        &mut *self.payload.get()
    }

    /// Producer side: write a packet into this slot and publish it as `seq`.
    fn store(&self, seq: u16, timestamp: f64, samples: &[f32]) {
        // Invalidate first, so a reader whose copy overlaps these writes sees
        // the stamp change. The fence orders the invalidation before the
        // payload writes for any reader that observes one of them.
        self.seq.store(SEQ_NONE, Ordering::Relaxed);
        fence(Ordering::Release);
        {
            // SAFETY: this is the producer, and the borrow ends before the
            // Release below publishes the slot.
            let payload = unsafe { self.payload_mut() };
            payload.timestamp = timestamp;
            payload.data.copy_from_slice(samples);
        }
        // Publish last: Release-store of seq synchronizes-with the consumer's
        // Acquire identity check.
        self.seq.store(seq as i32, Ordering::Release);
    }

    /// Consumer side: run `copy` on the payload if this slot holds `seq`
    /// both before and after it. `None` means the slot held something else,
    /// or the producer overwrote it while `copy` ran — in which case whatever
    /// `copy` produced is torn and must be discarded.
    ///
    /// An overwrite goes undetected only if it leaves the same `seq` behind:
    /// the transport delivering one packet twice (both copies carry the same
    /// audio) or the producer getting 65536 packets ahead mid-copy.
    fn read<R>(&self, seq: u16, copy: impl FnOnce(&SlotPayload) -> R) -> Option<R> {
        if self.seq.load(Ordering::Acquire) != seq as i32 {
            return None;
        }
        // SAFETY: the Acquire above synchronizes-with the producer's Release
        // in `store`, so every payload write for `seq` happened-before this
        // read. A concurrent overwrite is the tolerated race on `PacketSlot`.
        let result = copy(unsafe { &*self.payload.get() });
        // Pairs with the fence in `store`: if `copy` saw any write made after
        // an invalidation, the reload below sees that invalidation or later.
        fence(Ordering::Acquire);
        (self.seq.load(Ordering::Relaxed) == seq as i32).then_some(result)
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
    /// channel count that changed mid-stream) for the life of this regulator.
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
/// One network-side producer calls [`push_at`](Self::push_at); one audio-side
/// consumer calls [`pop_frames_at`](Self::pop_frames_at) (or [`pop_at`](Self::pop_at)). Main-thread [`stats`](Self::stats) /
/// [`depth`](Self::depth) / [`latency_ms`](Self::latency_ms) read only atomics.
/// Every method takes `&self`, and state is split by owner so that no thread
/// ever creates a `&mut` overlapping memory another thread can reach:
///
/// - [`Shared`] is only ever borrowed `&`. It holds the configuration that is
///   fixed at construction, the atomics (the `last_seq_in` / `last_seq_out`
///   pointers, the stream's adopted channel count and start time, and the
///   published stats snapshot), and the slot ring.
/// - Each slot's payload is guarded by the slot's `seq` stamp as a
///   seqlock: the producer invalidates the stamp, writes, then `Release`s the
///   new `seq`; the consumer trusts a copy only if an `Acquire` load of the
///   stamp before it and a reload after it both show the `seq` it expects
///   ([`PacketSlot::read`]). A producer that laps the ring onto the slot the
///   consumer is copying — a whole ring (`num_slots` packets, ~10.9 s at
///   fpp >= 32/48 kHz) ahead — therefore turns that copy into a concealed
///   packet rather than torn audio. The copy itself races the overwrite —
///   the one data race this type tolerates; see [`PacketSlot`].
/// - [`ConsumerState`] — tolerance, timing stats, Burg/channel state — sits
///   behind one [`UnsafeCell`] that only `pop` borrows mutably. The producer has no non-atomic state of its own, and
///   takes a `&mut` only to resize slot payloads before a stream starts
///   ([`Shared::adopt_stream_shape`]).
///
/// Calling `push` from two threads at once, or `pop` from two threads at once,
/// breaks this and is undefined behavior.
///
/// See WEB-53. Do not weaken this to `&mut self` + concurrent `SharedPtr::as_mut`.
pub struct Regulator {
    shared: Shared,
    consumer: UnsafeCell<ConsumerState>,
}

// SAFETY: `Regulator` is shared across the network producer, audio consumer, and
// main-thread stats readers. The only non-`Sync` parts are the `UnsafeCell`s:
// `consumer` is borrowed mutably only by the single consumer, and each slot
// payload only while no stream is running (`Shared::adopt_stream_shape`).
// See the struct docs.
unsafe impl Sync for Regulator {}

/// Regulator state readable from every thread. Only ever borrowed `&`.
struct Shared {
    // Configuration, fixed at construction
    sample_rate: u32,
    /// The most frames one `pop_frames_at` call may ask for. Written only by
    /// `with_callback_capacity`, before the regulator is shared; read-only
    /// after, so any thread (e.g. a host sizing a new processor while the old
    /// one is still popping) may read it through `&Shared`.
    max_callback_frames: usize,

    /// Packet storage (circular buffer by sequence number). Allocated at
    /// [`MAX_SLOTS`] entries; only the first `num_slots` carry a payload.
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
    /// Frames per packet adopted from the peer's first packet.
    fpp: AtomicUsize,
    /// Slots in use: [`num_slots`] of the adopted fpp.
    num_slots: AtomicUsize,
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
    /// Frames pulled from the slots into the frame FIFO but not yet served.
    pub_queued_frames: AtomicU64,
}

/// State owned by the consumer (`pop`). Reached only through
/// [`Regulator::consumer_mut`].
struct ConsumerState {
    // Stream geometry: the construction fpp until the peer's first packet
    // announces its own ([`ConsumerState::configure_fpp`])
    /// Frames per packet this consumer is rendering.
    fpp: usize,
    packets_in_past: usize,
    up_to_now: usize,
    beyond_now: usize,
    // Configuration, fixed at construction
    auto_mode: bool,
    auto_headroom: f64,
    /// Crossfade ramps, `MAX_FPP` long, valid for `[..fpp]`.
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

    // Frame-granular output. Packets are pulled whole into `fifo`, and
    // `pop_frames_at` serves exact frame counts from it. Preallocated
    // (`with_callback_capacity`), so the render thread never allocates.
    /// Pulled, not-yet-served samples live at `fifo[fifo_head..fifo_head + fifo_len]`.
    fifo: Vec<f32>,
    fifo_head: usize,
    fifo_len: usize,
    /// Stride of the FIFO's contents.
    fifo_channels: usize,

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
/// A JackTrip stream *adopts* its channel count and frames-per-packet from the
/// peer's first packet ([`Shared::adopt_stream_shape`]).
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
    /// reading (see [`Shared::adopt_stream_shape`]'s concurrency note).
    ChannelCountChanged { adopted: usize, got: usize },
    /// A stream's first packet announced a frames-per-packet outside
    /// `[MIN_FPP, MAX_FPP]`, so the stream could not be adopted.
    UnsupportedFpp { got: usize },
    /// `fpp` differs from the value adopted from an earlier packet in this
    /// stream. Rejected rather than re-adopted, for the same reason as
    /// [`ChannelCountChanged`](Self::ChannelCountChanged).
    FppChanged { adopted: usize, got: usize },
    /// `samples.len()` was not exactly `channels * fpp`.
    WrongPacketSize { expected: usize, got: usize },
}


impl Regulator {
    /// Exclusive access to the consumer's state.
    ///
    /// # Safety
    ///
    /// Only the single consumer thread may call this, and no reference from
    /// an earlier call may still be live.
    #[allow(clippy::mut_from_ref)]
    unsafe fn consumer_mut(&self) -> &mut ConsumerState {
        &mut *self.consumer.get()
    }

    /// Create a new Regulator with default parameters.
    pub fn new() -> Self {
        Self::with_params(1, 128, 48000, -1.0)
    }

    /// Create a new Regulator with specific parameters.
    ///
    /// `channels` and `fpp` describe the stream until the peer's first packet
    /// announces its own, which replaces them ([`Shared::adopt_stream_shape`]).
    pub fn with_params(channels: usize, fpp: usize, sample_rate: u32, tolerance_ms: f64) -> Self {
        let samples_per_packet = fpp * channels;

        let packets_in_past = packets_in_past(fpp);
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

        let ramp_len = fpp.max(MAX_FPP);
        let mut fade_up = vec![0.0; ramp_len];
        let mut fade_down = vec![0.0; ramp_len];
        fill_fades(&mut fade_up, &mut fade_down, fpp);

        let channel_states: Vec<ChannelState> = (0..channels.max(MAX_CHANNELS))
            .map(|_| ChannelState::new(fpp, packets_in_past))
            .collect();

        let initial_slots = num_slots(fpp);
        let slots = (0..MAX_SLOTS)
            .map(|i| PacketSlot::new(if i < initial_slots { samples_per_packet } else { 0 }))
            .collect();

        let current_headroom = if auto_headroom < 0.0 { 0.0 } else { auto_headroom };
        let shared = Shared {
            sample_rate,
            max_callback_frames: fpp,
            slots,
            last_seq_in: AtomicI32::new(SEQ_NONE),
            last_seq_out: AtomicI32::new(SEQ_NONE),
            num_channels: AtomicUsize::new(channels),
            fpp: AtomicUsize::new(fpp),
            num_slots: AtomicUsize::new(initial_slots),
            start_time_bits: AtomicU64::new(f64_to_bits(0.0)),
            packets_rejected: AtomicU64::new(0),
            pub_tolerance_bits: AtomicU64::new(f64_to_bits(initial_tolerance)),
            pub_headroom_bits: AtomicU64::new(f64_to_bits(current_headroom)),
            pub_max_latency_bits: AtomicU64::new(0),
            pub_glitches: AtomicU64::new(0),
            pub_packets: AtomicU64::new(0),
            pub_queued_frames: AtomicU64::new(0),
        };
        let consumer = ConsumerState {
            fpp,
            packets_in_past,
            up_to_now,
            beyond_now,
            auto_mode,
            auto_headroom,
            fade_up,
            fade_down,
            burg: BurgAlgorithm::with_capacity(UP_TO_NOW_MAX.max(up_to_now), up_to_now),
            channels: channel_states,
            num_channels: channels,
            last_stashed: None,
            fifo: vec![0.0; fifo_samples(fpp, fpp)],
            fifo_head: 0,
            fifo_len: 0,
            fifo_channels: channels,
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
    /// * `fpp` - The frames per packet it claims, from the wire header
    /// * `samples` - Interleaved audio samples
    /// * `now_ms` - Wall-clock time of arrival in milliseconds. The regulator
    ///   never reads a clock itself; every host supplies one (and must use the
    ///   same epoch for `push_at` and `pop_at`).
    ///
    /// # Returns
    /// See [`PushOutcome`].
    pub fn push_at(&self, sequence: u16, channels: usize, fpp: usize, samples: &[f32], now_ms: f64) -> PushOutcome {
        self.shared.push_internal(sequence, channels, fpp, samples, now_ms)
    }

    /// Size the frame FIFO for callbacks of up to `frames` frames (default:
    /// one packet), with room for the largest packet a peer may announce
    /// (`MAX_FPP`) so an adopted fpp never reallocates. Call before the
    /// regulator is shared; this is the only place the FIFO allocates.
    pub fn with_callback_capacity(mut self, frames: usize) -> Self {
        self.shared.max_callback_frames = frames;
        let consumer = self.consumer.get_mut();
        consumer.fifo = vec![0.0; fifo_samples(frames, consumer.fpp)];
        self
    }

    /// The rate this regulator's packets are at: the wire rate, never the
    /// audio device's.
    pub fn sample_rate(&self) -> u32 {
        self.shared.sample_rate
    }

    /// The most frames one [`pop_frames_at`](Self::pop_frames_at) call may ask
    /// for, as set by [`with_callback_capacity`](Self::with_callback_capacity).
    /// A host that owns a callback size checks it against this at setup.
    pub fn callback_capacity(&self) -> usize {
        self.shared.max_callback_frames
    }

    /// Pop exactly `frames` frames for one audio callback, whatever the packet
    /// size. Writes `frames * channels` interleaved samples to `output` and
    /// returns `channels` — the peer's width, synchronized by this call's
    /// Acquire (WEB-53 Bug 3) — or `0` (output untouched) if `frames` exceeds
    /// the [`with_callback_capacity`](Self::with_callback_capacity) or the
    /// stream has no channels.
    ///
    /// One call is one tick of the pull clock: the jitter statistics see the
    /// true interval between callbacks, however many packets this call pulls
    /// from the slots (none, when the FIFO already holds enough). Packets are
    /// pulled as of `now_ms`, which is the same instant for all of them.
    ///
    /// Use either this or [`pop_at`](Self::pop_at) on a given regulator, not both.
    pub fn pop_frames_at(&self, output: &mut [f32], frames: usize, now_ms: f64) -> usize {
        // SAFETY: single consumer thread (struct docs); borrow ends on return.
        let consumer = unsafe { self.consumer_mut() };
        consumer.pop_frames(&self.shared, output, frames, now_ms)
    }

    /// Pop exactly one packet (`fpp * channels` samples), for a host whose
    /// callback is one packet long. Prefer [`pop_frames_at`](Self::pop_frames_at).
    ///
    /// Returns `true` if real packet data was output, `false` if concealment
    /// was used. After returning, [`channels`](Self::channels) reflects the
    /// peer width synchronized by this pop's Acquire (WEB-53 Bug 3).
    ///
    /// `now_ms` is the caller's clock, on the same epoch as [`push_at`](Self::push_at).
    pub fn pop_at(&self, output: &mut [f32], now_ms: f64) -> bool {
        // SAFETY: `pop` is only called from the single consumer thread
        // (struct docs), and the borrow ends before this returns.
        let consumer = unsafe { self.consumer_mut() };
        consumer.pop_internal(&self.shared, output, now_ms)
    }

    /// Get current statistics (lock-free; safe from any thread).
    pub fn stats(&self) -> RegulatorStats {
        self.shared.stats()
    }

    /// Get the current tolerance in milliseconds.
    pub fn tolerance_ms(&self) -> f64 {
        f64_from_bits(self.shared.pub_tolerance_bits.load(Ordering::Relaxed))
    }

    /// Get the frames per packet: the peer's, once its first packet has been
    /// adopted, the construction value before.
    pub fn fpp(&self) -> usize {
        self.shared.fpp()
    }

    /// Wall-clock duration of `frames` frames at the wire sample rate.
    pub fn frames_to_ms(&self, frames: usize) -> f64 {
        frames_to_ms(frames, self.shared.sample_rate)
    }

    /// Get the number of channels (peer-adopted).
    ///
    /// After a [`pop_frames_at`](Self::pop_frames_at) or [`pop_at`](Self::pop_at)
    /// that rendered a started stream, this matches the width that pop used —
    /// the Acquire inside the pop orders the adoption.
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


/// Wall-clock duration of `frames` frames at `sample_rate`.
fn frames_to_ms(frames: usize, sample_rate: u32) -> f64 {
    1000.0 * frames as f64 / sample_rate as f64
}

impl Shared {
    fn channels(&self) -> usize {
        self.num_channels.load(Ordering::Relaxed)
    }

    fn start_time_ms(&self) -> f64 {
        f64_from_bits(self.start_time_bits.load(Ordering::Relaxed))
    }

    /// Frames per packet: the construction value until the peer's first
    /// packet is adopted. Read it on the consumer only after the `Acquire` of
    /// a started stream; elsewhere it is a display value.
    fn fpp(&self) -> usize {
        self.fpp.load(Ordering::Relaxed)
    }

    /// The slot a sequence number lives in, for the adopted ring depth.
    fn slot_index(&self, seq: u16) -> usize {
        slot_index(seq, self.num_slots.load(Ordering::Relaxed))
    }

    fn reject(&self) {
        self.packets_rejected.fetch_add(1, Ordering::Relaxed);
    }

    fn push_internal(&self, seq_num: u16, channels: usize, fpp: usize, samples: &[f32], now_ms: f64) -> PushOutcome {
        if channels == 0 || channels > MAX_CHANNELS {
            self.reject();
            return PushOutcome::UnsupportedChannelCount { got: channels };
        }

        let current = self.last_seq_in.load(Ordering::Acquire);

        // Channel count and frames-per-packet are both adopted from the peer's
        // first packet (below); an fpp this regulator cannot render at all
        // must not be adopted.
        if current == SEQ_NONE && !(MIN_FPP..=MAX_FPP).contains(&fpp) {
            self.reject();
            return PushOutcome::UnsupportedFpp { got: fpp };
        }

        // Every packet on the wire must carry exactly `channels * fpp` samples
        // — what its own header says. Anything else did not come from a
        // well-formed stream, and there is no honest way to place it in the
        // timeline. A short packet would play as a dropout in the middle of
        // good audio, and a long one would be silently truncated. Reject it
        // before it can occupy a slot or move the write pointer. (The header
        // parser already guarantees this; this is the defensive check.)
        let expected = channels * fpp;
        if samples.len() != expected {
            self.reject();
            return PushOutcome::WrongPacketSize { expected, got: samples.len() };
        }

        if current == SEQ_NONE {
            // First packet of a stream: adopt its shape rather than rejecting
            // a mismatch against whatever this regulator happened to be
            // constructed with.
            if channels != self.channels() || fpp != self.fpp() {
                self.adopt_stream_shape(channels, fpp);
            }
            self.start_time_bits.store(f64_to_bits(now_ms), Ordering::Relaxed);
        } else {
            // A later packet disagreeing with the already-adopted shape is a
            // peer bug or an unsignaled renegotiation, not something to
            // re-adopt into — see `PushOutcome::ChannelCountChanged`.
            let adopted_channels = self.channels();
            if channels != adopted_channels {
                self.reject();
                return PushOutcome::ChannelCountChanged { adopted: adopted_channels, got: channels };
            }
            let adopted_fpp = self.fpp();
            if fpp != adopted_fpp {
                self.reject();
                return PushOutcome::FppChanged { adopted: adopted_fpp, got: fpp };
            }
        }

        let relative_now = now_ms - self.start_time_ms();

        // Store the audio unconditionally, even for a packet the ordering check
        // below rejects: it is still the freshest thing that belongs in this
        // slot, and a straggler the read side has not passed yet is playable.
        // Publishing the slot is independent of the conditional last_seq_in
        // advance below — stragglers must still publish their slots.
        self.slots[self.slot_index(seq_num)].store(seq_num, relative_now, samples);

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
    /// Stores the new `num_channels`, `fpp` and `num_slots` and resizes the
    /// first `num_slots` slots' `data` buffers, zeroed. Slots past the new
    /// depth are released.
    ///
    /// Nothing the consumer owns is touched. Its per-channel Burg/ring state
    /// is preallocated for [`MAX_CHANNELS`] and [`MAX_FPP`] and is already
    /// clean — a regulator is never reused, so adoption runs once, against a
    /// freshly constructed one — so the consumer picks up the new width by
    /// reading `num_channels` after its `Acquire`, and reconfigures itself for
    /// the new fpp ([`ConsumerState::observe_stream`]). The jitter policy is
    /// untouched: only the tolerance configuration survives from construction.
    ///
    /// # Concurrency
    ///
    /// This runs on the network thread, inside `push`, and resizes up to
    /// [`MAX_SLOTS`] slot `Vec`s while the audio thread may concurrently be
    /// inside `pop`. It is sound only because it happens strictly before the
    /// `Release` store of `last_seq_in` in `push_internal`: `pop_internal`'s
    /// `SEQ_NONE` early return touches no slot. Any code path past that early
    /// return is reached only after an `Acquire` load has observed this call's
    /// later `Release` store, so it always sees the fully resized slots, never
    /// a partial one. The relaxed `fpp`/`num_slots` stores are covered the same
    /// way.
    ///
    /// No other adoption can race a running `pop`: the stream shape is fixed
    /// for the life of a regulator, and a reconnect builds a fresh regulator
    /// (a *connection's* buffers are retired, never reset) instead of
    /// re-arming this one. The session frees a retired regulator only after
    /// the render thread has confirmed it has let go of it.
    fn adopt_stream_shape(&self, channels: usize, fpp: usize) {
        let samples_per_packet = fpp * channels;
        let new_slots = num_slots(fpp);
        let old_slots = self.num_slots.load(Ordering::Relaxed);
        for (i, slot) in self.slots[..new_slots.max(old_slots)].iter().enumerate() {
            // SAFETY: this is the producer, and no consumer reads a slot
            // before `last_seq_in` leaves `SEQ_NONE` (see above).
            let payload = unsafe { slot.payload_mut() };
            if i < new_slots {
                payload.data.clear();
                payload.data.resize(samples_per_packet, 0.0);
            } else {
                payload.data = Vec::new();
            }
        }
        self.num_channels.store(channels, Ordering::Relaxed);
        self.fpp.store(fpp, Ordering::Relaxed);
        self.num_slots.store(new_slots, Ordering::Relaxed);
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
        let queued = self.pub_queued_frames.load(Ordering::Relaxed) as f32;
        let total_frames = (depth * self.fpp() as u32) as f32 + queued;
        (total_frames / self.sample_rate as f32) * 1000.0
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
        self.tick_pull(sh, now_ms);
        self.pull_packet(sh, output, now_ms)
    }

    /// Advance the pull clock: record the interval since the previous tick.
    /// Intervals of zero or less are discarded by `TimingStats::tick`.
    fn tick_pull(&mut self, sh: &Shared, now_ms: f64) {
        // Acquire first so `relative_now` is coherent with the stream's start.
        sh.last_seq_in.load(Ordering::Acquire);
        let relative_now = now_ms - sh.start_time_ms();
        if self.last_pop_time_ms > 0.0 {
            let elapsed = now_ms - self.last_pop_time_ms;
            self.pull_stats.tick(elapsed, relative_now);
        }
        self.last_pop_time_ms = now_ms;
    }

    /// Whether the stream is still in its startup silence: nothing has
    /// arrived, or the opening tolerance window has not yet elapsed.
    fn in_startup(&self, last_seq_in_raw: i32, relative_now: f64) -> bool {
        last_seq_in_raw == SEQ_NONE || relative_now < self.tolerance_ms
    }

    /// Pull one packet's worth of output (no pull-clock tick). `output` must
    /// be exactly `fpp * channels` long, at the stream's adopted fpp.
    fn pull_packet(&mut self, sh: &Shared, output: &mut [f32], now_ms: f64) -> bool {
        // Acquire FIRST so the stream shape and slots written before the
        // matching Release are visible (WEB-53 Bug 2).
        let last_seq_in_raw = sh.last_seq_in.load(Ordering::Acquire);
        let last_seq_in = last_seq_in_raw as u16;
        let relative_now = now_ms - sh.start_time_ms();
        if last_seq_in_raw != SEQ_NONE {
            self.observe_stream(sh);
        }

        // Return silence during startup
        if self.in_startup(last_seq_in_raw, relative_now) {
            output.fill(0.0);
            self.publish_stats(sh);
            return false;
        }

        // A started stream's width is fixed for the regulator's life, and the
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
                //
                // Deinterleave from slot directly into channel tmp buffers (NO
                // allocation). If the producer overwrote the slot mid-copy,
                // `read` returns `None` and the torn tmp buffers are discarded
                // by the concealment path, which zeroes them first.
                let n = self.num_channels;
                let channels = &mut self.channels[..n];
                let copied = sh.slots[slot_idx].read(seq, |payload| {
                    for (ch, channel) in channels.iter_mut().enumerate() {
                        deinterleave_channel(&payload.data, n, ch, &mut channel.tmp_buf);
                    }
                });
                if copied.is_none() {
                    // Stamp mismatch: conceal rather than play torn audio.
                    self.pull_stats.underruns += 1;
                    self.output_concealment(output);
                    self.publish_stats(sh);
                    return false;
                }

                // Process with Burg algorithm
                self.process_burg(false);
                self.interleave_output(output);

                sh.last_seq_out.store(seq as i32, Ordering::Release);
                self.packet_count += 1;
                self.publish_stats(sh);
                true
            }
            Some(PacketDecision::ConcealSkippedPacket { skipped }) => {
                self.pull_stats.overruns += skipped;
                self.output_concealment(output);
                self.publish_stats(sh);
                false
            }
            None => {
                self.handle_underrun(sh, output, relative_now)
            }
        }
    }

    /// See [`Regulator::pop_frames_at`].
    fn pop_frames(&mut self, sh: &Shared, output: &mut [f32], frames: usize, now_ms: f64) -> usize {
        debug_assert!(frames <= sh.max_callback_frames, "callback of {frames} frames exceeds the FIFO capacity");
        if frames > sh.max_callback_frames {
            return 0;
        }
        self.tick_pull(sh, now_ms);

        // One Acquire for the whole call: whether the stream has started, and
        // therefore its width, is fixed for every packet pulled below.
        let last_seq_in_raw = sh.last_seq_in.load(Ordering::Acquire);
        if last_seq_in_raw != SEQ_NONE {
            self.observe_stream(sh);
        }
        let in_startup = self.in_startup(last_seq_in_raw, now_ms - sh.start_time_ms());
        let channels = sh.channels();
        if channels != self.fifo_channels {
            // The FIFO is at the old stride (the stream just adopted its width).
            self.fifo_head = 0;
            self.fifo_len = 0;
            self.fifo_channels = channels;
        }
        if channels == 0 {
            return 0;
        }

        let fpp = self.fpp;
        while self.fifo_len < frames * channels {
            if in_startup {
                // Startup silence is queued only as far as this callback needs:
                // a whole packet would leave a silent tail ahead of the first
                // real audio, delaying it for the rest of the stream.
                let shortfall = frames * channels - self.fifo_len;
                self.fifo_append(shortfall).fill(0.0);
                continue;
            }
            // Pull straight into the FIFO tail. The FIFO is moved out so the
            // pull can borrow `self` (nothing on the pull path reads it); no
            // allocation, no staging copy.
            let len = fpp * channels;
            self.fifo_reserve(len);
            let mut fifo = std::mem::take(&mut self.fifo);
            let end = self.fifo_head + self.fifo_len;
            self.pull_packet(sh, &mut fifo[end..end + len], now_ms);
            self.fifo = fifo;
            self.fifo_len += len;
        }

        let want = frames * channels;
        output[..want].copy_from_slice(&self.fifo[self.fifo_head..self.fifo_head + want]);
        self.fifo_head += want;
        self.fifo_len -= want;
        if self.fifo_len == 0 {
            self.fifo_head = 0;
        }
        self.publish_stats(sh);
        channels
    }

    /// Compact the FIFO if needed so `len` more samples fit after the queued ones.
    fn fifo_reserve(&mut self, len: usize) {
        if self.fifo_head + self.fifo_len + len > self.fifo.len() {
            self.fifo.copy_within(self.fifo_head..self.fifo_head + self.fifo_len, 0);
            self.fifo_head = 0;
        }
    }

    /// Extend the FIFO by `len` samples and return them for the caller to fill.
    fn fifo_append(&mut self, len: usize) -> &mut [f32] {
        self.fifo_reserve(len);
        let end = self.fifo_head + self.fifo_len;
        self.fifo_len += len;
        &mut self.fifo[end..end + len]
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
            if let Some(timestamp) = sh.slots[sh.slot_index(last_out)].read(last_out, |p| p.timestamp) {
                if now - timestamp > 10000.0 {
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
        let fpp = self.fpp;
        for channel in &mut self.channels[..self.num_channels] {
            channel.tmp_buf[..fpp].fill(0.0);
        }
        self.process_burg(true);
        self.interleave_output(output);
    }

    fn interleave_output(&self, output: &mut [f32]) {
        let n = self.num_channels;
        // Exactly one packet: a shorter buffer would silently drop the tail
        // (`interleave_channel` zips), a longer one would keep stale audio in
        // it. Every caller hands over an exactly-sized buffer.
        debug_assert_eq!(output.len(), self.fpp * n, "pop output must be exactly one packet");
        for (ch, channel) in self.channels[..n].iter().enumerate() {
            interleave_channel(&channel.output_now_packet[..self.fpp], n, ch, output);
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
        if new_pkts > sh.num_slots.load(Ordering::Relaxed) {
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
            let slot_idx = sh.slot_index(seq);

            let Some(timestamp) = sh.slots[slot_idx].read(seq, |p| p.timestamp) else {
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
        let Some(last_timestamp) = sh.slots[sh.slot_index(last_out)].read(last_out, |p| p.timestamp) else {
            return;
        };
        let prev_time = last_timestamp + (pkts as f64 + 1.0) * self.fpp_duration_ms(sh);
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
                ((AUTO_HEADROOM_GLITCH_TOLERANCE * sh.sample_rate as f64) / self.fpp as f64)
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

        new_tolerance = new_tolerance.clamp(self.fpp_duration_ms(sh), AUTO_MAX_MS);
        self.tolerance_ms = new_tolerance;
        self.publish_stats(sh);
    }

    /// Process audio with Burg algorithm for PLC.
    fn process_burg(&mut self, glitch: bool) {
        let fpp = self.fpp;
        let up_to_now = self.up_to_now;
        let beyond_now = self.beyond_now;
        let primed = self.plc_packet_count > self.packets_in_past as u64;

        for channel in &mut self.channels[..self.num_channels] {
            // Copy real packet data, and if not a glitch push it to the ring
            if glitch {
                channel.real_now_packet[..fpp].fill(0.0);
            } else {
                channel.real_now_packet[..fpp].copy_from_slice(&channel.tmp_buf[..fpp]);
                channel.ring_buffer_push(fpp);
            }

            // Build real past from ring buffer
            if primed {
                channel.real_past_into_prediction(fpp);
            }

            // Perform prediction on glitch
            if glitch {
                // Copy predicted past into prediction buffer
                channel.prediction[..up_to_now].copy_from_slice(&channel.predicted_past[..up_to_now]);

                // Train Burg model using pre-allocated buffer (no allocation!)
                channel.train_data[..up_to_now].copy_from_slice(&channel.prediction[..up_to_now]);
                self.burg.train(&channel.train_data[..up_to_now], &mut channel.coeffs);

                // Predict future samples
                let tail_size = up_to_now + 2 * fpp;
                self.burg.predict(&channel.coeffs, &mut channel.prediction[..tail_size], tail_size);

                // Extract predicted now packet
                channel.predicted_now_packet[..fpp]
                    .copy_from_slice(&channel.prediction[up_to_now..up_to_now + fpp]);
            }

            // Generate output with crossfade
            let output = &mut channel.output_now_packet[..fpp];
            if glitch {
                if primed {
                    output.copy_from_slice(&channel.predicted_now_packet[..fpp]);
                } else {
                    output.fill(0.0);
                }
            } else if self.last_was_glitch {
                // Crossfade from prediction to real
                let fades = self.fade_down[..fpp].iter().zip(&self.fade_up[..fpp]);
                let sources = channel.future_predicted_packet[..fpp].iter().zip(&channel.real_now_packet[..fpp]);
                for (out, ((&down, &up), (&predicted, &real))) in output.iter_mut().zip(fades.zip(sources)) {
                    *out = down * predicted + up * real;
                }
            } else {
                output.copy_from_slice(&channel.real_now_packet[..fpp]);
            }

            // Copy output to tmp_buf for consistency
            channel.tmp_buf[..fpp].copy_from_slice(&channel.output_now_packet[..fpp]);

            // Shift predicted past (rotate left without allocation): this
            // moves every packet forward by one position in a single operation.
            channel.predicted_past[..up_to_now].rotate_left(fpp);
            // Now the last position is free, fill it with current output
            channel.predicted_past[up_to_now - fpp..up_to_now]
                .copy_from_slice(&channel.output_now_packet[..fpp]);

            // Store future prediction for next crossfade
            channel.future_predicted_packet[..fpp]
                .copy_from_slice(&channel.prediction[beyond_now..beyond_now + fpp]);
        }

        self.last_was_glitch = glitch;
        self.plc_packet_count += 1;
    }

    /// Wall-clock duration of one packet at the stream's fpp.
    fn fpp_duration_ms(&self, sh: &Shared) -> f64 {
        frames_to_ms(self.fpp, sh.sample_rate)
    }

    /// The tolerance a stream opens with: a fraction of a packet in auto mode,
    /// the configured value otherwise.
    fn initial_tolerance_ms(&self) -> f64 {
        if self.auto_mode {
            self.fpp as f64 * AUTO_INIT_VAL_FACTOR
        } else {
            self.auto_headroom
        }
    }

    /// Follow the stream's adopted frames-per-packet. Called right after the
    /// `Acquire` that shows a started stream, so `sh.fpp()` is the value the
    /// producer stored before the matching `Release`. Idempotent.
    fn observe_stream(&mut self, sh: &Shared) {
        let fpp = sh.fpp();
        if fpp != self.fpp {
            self.configure_fpp(sh.sample_rate, fpp);
        }
    }

    /// Re-derive everything that depends on the packet size, for a stream
    /// whose first packet announced `fpp`. Stays within capacity preallocated
    /// at construction, so it never allocates. Nothing has run `process_burg`
    /// yet for this stream — adoption happens before the first packet is
    /// played — so there is no history to migrate, and the channel buffers
    /// are already clean.
    fn configure_fpp(&mut self, sample_rate: u32, fpp: usize) {
        debug_assert!((MIN_FPP..=MAX_FPP).contains(&fpp), "adopted fpp {fpp} out of range");
        self.fpp = fpp;
        self.packets_in_past = packets_in_past(fpp);
        self.up_to_now = self.packets_in_past * fpp;
        self.beyond_now = (self.packets_in_past + 1) * fpp;
        fill_fades(&mut self.fade_up, &mut self.fade_down, fpp);
        self.burg.set_size(self.up_to_now);
        for channel in &mut self.channels {
            channel.configure(self.packets_in_past);
        }
        self.push_stats.set_window(sample_rate, fpp);
        self.pull_stats.set_window(sample_rate, fpp);
        if self.auto_mode {
            self.tolerance_ms = self.initial_tolerance_ms();
        }
        // Frames pulled at the old packet size.
        self.fifo_head = 0;
        self.fifo_len = 0;
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
        sh.pub_queued_frames
            .store((self.fifo_len / self.fifo_channels.max(1)) as u64, Ordering::Relaxed);
    }
}

// ============================================================================
// Tests
// ============================================================================

/// Test drivers shared with other modules' tests (the processor's), so a
/// "peer streams, host pulls in callbacks" loop is written once.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// Push a packet a test expects the regulator to store, asserting the
    /// outcome. Tests that drive `push_internal` for setup would otherwise
    /// discard the `#[must_use]` result, so a push that starts getting
    /// rejected (wrong size, unadopted channel count) would leave the
    /// regulator empty and surface as a confusing failure further down —
    /// or as no failure at all.
    pub fn push_stored(reg: &Regulator, seq: u16, channels: usize, samples: &[f32], now_ms: f64) {
        assert_eq!(
            reg.push_at(seq, channels, samples.len() / channels, samples, now_ms),
            PushOutcome::Stored,
            "setup push of seq {seq} ({channels}ch) must be stored"
        );
    }

    /// Sample for `frame`/`ch` of the test stream: distinct per frame and
    /// channel, never 0 (0 is startup silence).
    pub fn ramp(frame: usize, ch: usize) -> f32 {
        (frame * 16 + ch + 1) as f32
    }

    pub fn ramp_packet(seq: usize, fpp: usize, channels: usize) -> Vec<f32> {
        (0..fpp * channels).map(|i| ramp(seq * fpp + i / channels, i % channels)).collect()
    }

    /// Callbacks allowed to hold the startup transient: a stream starts on
    /// the *newest* packet, so the rest of that callback's pulls find nothing
    /// newer and conceal. Exactness is asserted once the stream has settled.
    pub const SETTLE_CALLBACKS: usize = 40;

    /// Drive `callbacks` callbacks of `frames` frames against a peer sending
    /// one `fpp` packet (`packet(seq)`) per packet period (everything that has arrived by
    /// a callback is pushed before it). `pull(k, now_ms)` performs callback
    /// `k`'s pop — directly on the regulator, or through whatever wraps it.
    pub fn drive_callbacks(
        reg: &Regulator,
        (fpp, frames, channels): (usize, usize, usize),
        callbacks: usize,
        packet: impl Fn(usize) -> Vec<f32>,
        mut pull: impl FnMut(usize, f64),
    ) {
        let period = reg.frames_to_ms(fpp);
        let cb_period = reg.frames_to_ms(frames);
        let mut pushed = 0usize;
        for k in 0..callbacks {
            let now = 21.0 + k as f64 * cb_period;
            while 1.0 + pushed as f64 * period <= now {
                push_stored(reg, pushed as u16, channels, &packet(pushed), 1.0 + pushed as f64 * period);
                pushed += 1;
            }
            pull(k, now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    /// Plant a packet directly in the ring, bypassing `push`, so a test can
    /// lay out an arrival timeline (timestamps, gaps, out-of-order arrivals)
    /// without driving the wall clock. `samples` must be a full packet — the
    /// same contract `push` enforces.
    fn plant_packet(reg: &Regulator, seq: u16, timestamp: f64, samples: &[f32]) {
        reg.shared.slots[reg.shared.slot_index(seq)].store(seq, timestamp, samples);
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
    fn real_past_lands_oldest_first_across_ring_wrap() {
        // 3 packets of 4 samples; 4 pushes into a 3-packet ring, so the write
        // pointer has wrapped and the oldest surviving packet is mid-ring.
        let mut channel = ChannelState::new(4, 3);
        for packet in 1..=4 {
            channel.tmp_buf.fill(packet as f32);
            channel.ring_buffer_push(4);
        }

        channel.real_past_into_prediction(4);

        let expected: Vec<f32> = [2.0, 3.0, 4.0].iter().flat_map(|&p| [p; 4]).collect();
        assert_eq!(channel.prediction[..12], expected[..]);
        // The future region past the history is left alone.
        assert!(channel.prediction[12..].iter().all(|&s| s == 0.0));
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
        let _result1 = reg.pop_at(&mut output, 60.0);
        let _result2 = reg.pop_at(&mut output, 63.0);
        let _result3 = reg.pop_at(&mut output, 66.0);
        
        // Verify depth calculation works across wraparound
        let depth = reg.depth();
        assert!(depth < 100); // Should be a reasonable small number, not huge
        
        // Verify last_seq_out was set properly (should be Some value, not causing issues)
        assert!(last_seq_out_opt(&reg).is_some());
    }

    /// An inbound outage longer than the ring's horizon strands the read
    /// pointer: the peer keeps counting (sequence numbers on the wire only ever
    /// go up), so when packets resume the span from `last_seq_out` to
    /// `last_seq_in` is wider than `num_slots` and every slot it names has been
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
        // The ring's depth follows the adopted fpp, so the burst that outruns
        // it does too: 16 hits the slot cap, 1024 the smallest ring. The
        // regulator is built for 128, as the session builds it, and adopts.
        for fpp in [MIN_FPP, 128, MAX_FPP] {
            loss_burst_wider_than_ring_resyncs_read_pointer(fpp);
        }
    }

    fn loss_burst_wider_than_ring_resyncs_read_pointer(fpp: usize) {
        let reg = Regulator::with_params(1, 128, 48_000, 5.0);
        let samples = vec![0.5f32; fpp];
        let dt = reg.frames_to_ms(fpp);
        let mut out = vec![0.0f32; fpp];

        // A stream that is playing out normally.
        let mut t = 0.0;
        for i in 0..10u16 {
            push_stored(&reg, 100 + i, 1, &samples, t);
            t += dt;
        }
        let mut pop_t = 10.0;
        for _ in 0..10 {
            reg.pop_at(&mut out, pop_t);
            pop_t += dt;
        }
        assert_eq!(last_seq_out_opt(&reg), Some(109), "stream should be playing out");

        // Nothing arrives for a fifth more packets than the ring holds — 13.3 s
        // at fpp=128, past its 10.9 s horizon. An ordinary wifi roam, not an
        // exotic event. The peer never renumbers; it just keeps counting.
        let ring = reg.shared.num_slots.load(Ordering::Relaxed);
        assert_eq!(ring, num_slots(fpp), "fpp={fpp}: the stream adopted its ring depth");
        let gap = (ring + ring / 5) as u16;
        let resume_t = t + gap as f64 * dt;
        let first_resumed = 109 + gap + 1;

        let mut first_real_pkt = None;
        let mut tail_real = 0;
        for k in 0..600u16 {
            let now = resume_t + k as f64 * dt;
            push_stored(&reg, first_resumed + k, 1, &samples, now);
            let real = reg.pop_at(&mut out, now + 1.0);
            if real && first_real_pkt.is_none() {
                first_real_pkt = Some(k);
            }
            if k >= 550 && real {
                tail_real += 1;
            }
        }

        let first_real_pkt = first_real_pkt
            .unwrap_or_else(|| panic!("fpp={fpp}: real audio must resume after a loss burst wider than the ring"));
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
    /// arrived), but a slot from `num_slots` sequence numbers earlier — which
    /// aliases onto the same slot index — is still sitting there
    /// unoverwritten, `find_best_packet` must not mistake that stale data for
    /// the candidate. The stale timestamp is deliberately kept close to the
    /// last-played one so the timestamp-proximity heuristic alone would not
    /// catch it — only a direct seq check can.
    #[test]
    fn test_stale_aliased_slot_is_not_played_as_missing_packet() {
        let reg = Regulator::with_params(1, 32, 48_000, 10.0);
        let last_seq = 5u16;
        let candidate_seq = last_seq.wrapping_add(1); // 6 — genuinely lost, never planted
        let stale_seq = candidate_seq.wrapping_sub(reg.shared.num_slots.load(Ordering::Relaxed) as u16); // aliases to same slot
        let last_packet = vec![0.1f32; reg.fpp()];
        let stale_packet = vec![0.9f32; reg.fpp()];
        set_last_seq_out(&reg, Some(last_seq));
        reg.shared.last_seq_in.store(candidate_seq as i32, Ordering::Release);

        plant_packet(&reg, last_seq, 40.0, &last_packet);
        plant_packet(&reg, stale_seq, 41.0, &stale_packet); // candidate_seq never planted

        let mut output = vec![0.0f32; reg.fpp()];
        let result = reg.pop_at(&mut output, 50.0);

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
        let c = consumer(&mut reg);
        let history_packets = c.packets_in_past;
        let history: Vec<f32> = (0..c.up_to_now)
            .map(|i| (i as f32 * 0.1).sin())
            .collect();

        c.channels[0].predicted_past[..c.up_to_now].copy_from_slice(&history);

        c.process_burg(true);
        assert!(
            c.channels[0]
                .output_now_packet
                .iter()
                .all(|sample| sample.abs() < 1e-6),
            "PLC should stay muted before the predictor is primed"
        );

        c.plc_packet_count = history_packets as u64 + 1;
        c.process_burg(true);
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
        let fpp_duration_ms = reg.frames_to_ms(reg.fpp());
        let packet = vec![0.25f32; fpp];
        let last_packet = vec![0.1f32; fpp];
        let last_seq = 10u16;
        let good_seq = last_seq.wrapping_add(2);

        {
            let c = consumer(&mut reg);
            let history: Vec<f32> = (0..c.up_to_now)
                .map(|i| (i as f32 * 0.1).sin())
                .collect();
            c.channels[0].predicted_past[..c.up_to_now].copy_from_slice(&history);
            c.plc_packet_count = c.packets_in_past as u64 + 1;
        }
        set_last_seq_out(&reg, Some(last_seq));
        reg.shared.last_seq_in.store(good_seq as i32, Ordering::Release);

        plant_packet(&reg, last_seq, 40.0, &last_packet);
        plant_packet(&reg, good_seq, 45.0, &packet);

        let mut concealment = vec![0.0f32; reg.fpp()];
        let concealment_result = reg.pop_at(&mut concealment, 50.0);
        assert!(!concealment_result);
        assert_eq!(consumer(&mut reg).pull_stats.overruns, 1);
        assert_eq!(consumer(&mut reg).last_stashed.map(|(seq, _)| seq), Some(good_seq));
        assert_eq!(last_seq_out_opt(&reg), Some(last_seq));
        assert!(
            concealment.iter().any(|sample| sample.abs() > 1e-6),
            "skipped packets should trigger PLC output before the real packet is replayed"
        );

        let mut real_output = vec![0.0f32; reg.fpp()];
        let real_result = reg.pop_at(&mut real_output, 50.0 + fpp_duration_ms);
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
        let fpp_duration_ms = reg.frames_to_ms(reg.fpp());
        let stale_packet = vec![0.25f32; reg.fpp()];
        let fresh_packet = vec![0.5f32; reg.fpp()];
        let last_seq = 10u16;
        let stale_seq = last_seq.wrapping_add(1);
        let fresh_seq = last_seq.wrapping_add(2);
        set_last_seq_out(&reg, Some(last_seq));
        reg.shared.last_seq_in.store(fresh_seq as i32, Ordering::Release);

        // Previously-played packet, reference point for push-stats timing.
        let silent_packet = vec![0.0f32; reg.fpp()];
        plant_packet(&reg, last_seq, 40.0, &silent_packet);
        // Stale but valid packet: misses tolerance at now=60 (41 + 10 < 60),
        // so the scan records it as `first_good_skipped` instead of playing it.
        plant_packet(&reg, stale_seq, 41.0, &stale_packet);
        // Fresh packet within tolerance (55 + 10 >= 60); playing it would skip
        // exactly one good packet, which triggers the deferral.
        plant_packet(&reg, fresh_seq, 55.0, &fresh_packet);

        let mut output = vec![0.0f32; reg.fpp()];
        let first_result = reg.pop_at(&mut output, 60.0);
        assert!(
            first_result,
            "deferral should play the stale packet as real audio, not concealment"
        );
        assert_eq!(last_seq_out_opt(&reg), Some(stale_seq));
        assert_eq!(consumer(&mut reg).last_stashed.map(|(seq, _)| seq), Some(fresh_seq));
        assert_eq!(consumer(&mut reg).pull_stats.overruns, 0, "no glitch may be recorded");
        assert_eq!(consumer(&mut reg).packet_count, 1);

        let second_result = reg.pop_at(&mut output, 60.0 + fpp_duration_ms);
        assert!(second_result);
        assert_eq!(consumer(&mut reg).last_stashed, None);
        assert_eq!(last_seq_out_opt(&reg), Some(fresh_seq));
        assert_eq!(consumer(&mut reg).packet_count, 2);
        assert_eq!(consumer(&mut reg).pull_stats.overruns, 0);
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
        let r1 = reg.pop_at(&mut out, 10.0);
        assert!(r1);
        assert_eq!(last_seq_out_opt(&reg), Some(0));

        // Skip seq 1, 2 — push seq 3 directly. With `skipped = 2`, the
        // regulator should treat this as a glitch, conceal, stash, and bump
        // overruns.
        push_stored(&reg, 3, 1, &samples, 20.0);
        let r2 = reg.pop_at(&mut out, 40.0);
        assert!(!r2, "skipped-gap path returns concealment (false)");
        assert_eq!(consumer(&mut reg).pull_stats.overruns, 2, "overrun counter tracks skip distance");
        // The good packet must have been stashed for replay on the next pop.
        assert_eq!(consumer(&mut reg).last_stashed.map(|(s, _)| s), Some(3));

        // Next pop should consume the stashed real packet, no new overruns.
        let overruns_before = consumer(&mut reg).pull_stats.overruns;
        let r3 = reg.pop_at(&mut out, 42.0);
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
        let r0 = reg.pop_at(&mut out, 10.0);
        assert!(r0);
        let plc_after_first = consumer(&mut reg).plc_packet_count;
        let packets_after_first = consumer(&mut reg).packet_count;

        // Three consecutive underrun pops should each invoke PLC.
        for i in 0..3 {
            let t = 12.0 + i as f64 * 2.0;
            let result = reg.pop_at(&mut out, t);
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
        let r_resume = reg.pop_at(&mut out, 30.0);
        assert!(r_resume, "PLC must disengage once a real packet is available");
        assert_eq!(last_seq_out_opt(&reg), Some(1));
        assert_eq!(consumer(&mut reg).packet_count, packets_after_first + 1);
        let underruns_after = consumer(&mut reg).pull_stats.underruns;

        // No further underruns when consumption keeps pace.
        push_stored(&reg, 2, 1, &samples, 32.0);
        let r_next = reg.pop_at(&mut out, 38.0);
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
        let r = reg.pop_at(&mut out, 10.0);
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
            let _ = reg.pop_at(&mut out, t);
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
        let _ = reg.pop_at(&mut out, 15.0);
        let _ = reg.pop_at(&mut out, 17.0);
        let _ = reg.pop_at(&mut out, 19.0); // underrun

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
        let fpp_duration_ms = reg.frames_to_ms(reg.fpp());

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
        assert_eq!(reg.push_at(0, 1, reg.fpp(), &ramp, 0.0), PushOutcome::Stored);
        assert_eq!(reg.channels(), 1, "first packet must adopt the peer's channel count");

        let mut out = vec![0.0f32; reg.fpp()];
        let real = reg.pop_at(&mut out, 10.0); // past the 5ms tolerance
        assert!(real, "the adopted-stride packet must play back as real audio");
        assert_eq!(
            out, ramp,
            "deinterleave stride, ChannelState count, and slot length must all match the adopted count"
        );

        // Adoption is not a rebuild: fpp and the jitter policy are untouched.
        assert_eq!(reg.fpp(), original_fpp, "fpp must be unchanged by adoption");
        assert_eq!(
            reg.tolerance_ms(), original_tolerance,
            "adoption must not touch the jitter policy"
        );
    }

    /// A packet's header declares its channel count and frames-per-packet, so a
    /// packet carrying any other number of samples cannot be well-formed. `push` must reject it outright rather than fit
    /// it to the slot: a short packet padded or left partially written plays
    /// as a burst of garbage or silence inside otherwise good audio, and
    /// either size would still drag the write pointer forward onto a slot the
    /// read side then hands to the output.
    #[test]
    fn test_push_rejects_packets_that_are_not_exactly_one_packet_long() {
        let mut reg = Regulator::with_params(2, 64, 48_000, 5.0);
        let good = vec![0.5f32; reg.fpp() * reg.channels()];
        assert_eq!(reg.push_at(10, 2, reg.fpp(), &good, 0.0), PushOutcome::Stored);
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
                reg.push_at(11, 2, reg.fpp(), &packet, 1.0),
                PushOutcome::WrongPacketSize { expected: reg.fpp() * reg.channels(), got: len },
                "accepted a {len}-sample packet on a {}-sample stream",
                reg.fpp() * reg.channels()
            );
        }

        // Neither the ring nor the write pointer moved: the slot the rejected
        // packets targeted is still empty, and the newest sequence number is
        // still the last good packet's.
        assert_eq!(reg.shared.last_seq_in.load(Ordering::Acquire), 10);
        let slot = &mut reg.shared.slots[reg.shared.slot_index(11)];
        assert!(slot.read(11, |_| ()).is_none());
        let payload = slot.payload.get_mut();
        assert_eq!(payload.timestamp, 0.0);
        assert!(payload.data.iter().all(|&s| s == 0.0));
        assert_eq!(reg.stats().packets_rejected, wrong_sizes.len() as u64);

        // Channel count 0, or beyond MAX_CHANNELS, is rejected before the size
        // check even runs.
        let rejected_before = reg.stats().packets_rejected;
        assert_eq!(
            reg.push_at(12, 0, reg.fpp(), &[], 2.0),
            PushOutcome::UnsupportedChannelCount { got: 0 }
        );
        assert_eq!(
            reg.push_at(12, MAX_CHANNELS + 1, reg.fpp(), &[0.0], 2.0),
            PushOutcome::UnsupportedChannelCount { got: MAX_CHANNELS + 1 }
        );
        assert_eq!(reg.stats().packets_rejected, rejected_before + 2);

        // A packet claiming a channel count that differs from the count
        // already adopted for this stream is rejected outright, not
        // re-adopted — see `PushOutcome::ChannelCountChanged`.
        let mismatched = vec![0.5f32; reg.fpp()]; // 1ch-sized; this stream adopted 2ch
        assert_eq!(
            reg.push_at(13, 1, reg.fpp(), &mismatched, 3.0),
            PushOutcome::ChannelCountChanged { adopted: 2, got: 1 }
        );
        assert_eq!(
            reg.shared.last_seq_in.load(Ordering::Acquire), 10,
            "a rejected packet must not move the write pointer"
        );

        // Likewise a well-formed packet at a different fpp than the stream
        // adopted (64): rejected, not re-adopted.
        let other_fpp = vec![0.5f32; 128 * 2];
        assert_eq!(
            reg.push_at(14, 2, 128, &other_fpp, 4.0),
            PushOutcome::FppChanged { adopted: 64, got: 128 }
        );
        assert_eq!(reg.fpp(), 64, "a rejected packet must not change the adopted fpp");
        assert_eq!(reg.shared.last_seq_in.load(Ordering::Acquire), 10);
    }

    /// `push`'s explicit `channels` and `fpp` arguments (rather than inferring
    /// one from the sample count) exist to prevent exactly this ambiguity: 4
    /// channels at fpp=64 and 2 channels at fpp=128 both total 256 samples.
    /// Both come from the wire header, so a packet claiming 4 channels at
    /// fpp=128 must be rejected for its size — never silently reinterpreted as
    /// a valid 2-channel packet just because the sample count happens to match.
    #[test]
    fn test_push_channel_count_is_explicit_not_inferred_from_sample_count() {
        let reg = Regulator::with_params(2, 128, 48_000, 5.0);
        let first = vec![0.1f32; 256]; // 2ch * 128fpp
        assert_eq!(reg.push_at(0, 2, 128, &first, 0.0), PushOutcome::Stored);

        // Same total sample count as a valid 2ch*128fpp packet, but claiming
        // 4 channels — must be rejected for its size (4*128=512 != 256), not
        // accepted as if it were secretly the 2ch stride.
        let same_len_but_4ch = vec![0.2f32; 256];
        assert_eq!(
            reg.push_at(1, 4, 128, &same_len_but_4ch, 1.0),
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
        let first_sample = reg.shared.slots[reg.shared.slot_index(11)]
            .read(11, |payload| payload.data[0])
            .expect("straggler must Release-publish its slot stamp");
        assert!(
            (first_sample - 0.75).abs() < 1e-6,
            "straggler payload must be stored"
        );
    }

    /// A producer that laps the ring onto the slot the consumer is copying
    /// must make that copy fail, not pass off a mix of two packets as the one
    /// the consumer asked for. Without the post-copy stamp reload, `read`
    /// returns `Some`. Skipped under Miri: the overwrite is the tolerated race
    /// documented on `PacketSlot`.
    #[test]
    #[cfg_attr(miri, ignore = "exercises the tolerated race documented on PacketSlot")]
    fn test_slot_read_rejects_copy_overlapping_a_producer_write() {
        let slot = PacketSlot::new(4);
        let seq = 7u16;
        let lapped = seq.wrapping_add(num_slots(HIST_FPP) as u16);

        slot.store(seq, 1.0, &[0.25; 4]);
        assert_eq!(slot.read(seq, |p| p.data[0]), Some(0.25));

        let torn = slot.read(seq, |p| {
            let before = p.data[0];
            slot.store(lapped, 2.0, &[0.75; 4]);
            (before, p.data[3])
        });
        assert_eq!(torn, None, "a copy spanning an overwrite must be discarded");
        assert_eq!(slot.read(lapped, |p| p.data[3]), Some(0.75));
    }

    /// The other half of the seqlock: a copy that ends while the producer is
    /// still mid-write sees the old stamp on reload unless `store` invalidates
    /// it before writing. Every copy `read` accepts must be one whole packet.
    /// Timing-dependent: a missing invalidation is caught on some runs, not
    /// all. Skipped under Miri, which correctly reports the overlapping copy
    /// as the data race `PacketSlot` documents as tolerated.
    #[test]
    #[cfg_attr(miri, ignore = "exercises the tolerated race documented on PacketSlot")]
    fn test_slot_read_never_accepts_a_torn_packet() {
        const LEN: usize = 16;
        let packets: u16 = 20_000;
        // Through a `Regulator`, which is what carries the `Sync` assertion.
        let reg = &Regulator::with_params(1, LEN, 48_000, 5.0);
        let done = &AtomicI32::new(0);
        std::thread::scope(|scope| {
            scope.spawn(move || {
                let slot = &reg.shared.slots[0];
                for seq in 1..=packets {
                    slot.store(seq, seq as f64, &[seq as f32; LEN]);
                }
                done.store(1, Ordering::Release);
            });
            scope.spawn(move || {
                let slot = &reg.shared.slots[0];
                while done.load(Ordering::Acquire) == 0 {
                    let seq = slot.seq.load(Ordering::Relaxed);
                    if seq == SEQ_NONE {
                        continue;
                    }
                    let copy = slot.read(seq as u16, |p| {
                        (p.timestamp, p.data.clone())
                    });
                    if let Some((timestamp, samples)) = copy {
                        assert_eq!(timestamp, seq as f64, "accepted a torn timestamp");
                        assert!(
                            samples.iter().all(|&s| s == seq as f32),
                            "accepted a torn packet for seq {seq}: {samples:?}"
                        );
                    }
                }
            });
        });
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
                // `pop_at` takes exactly one packet: `fpp` samples per channel, mono here.
                let mut out = vec![0.0f32; fpp];
                for i in 0..40 {
                    reg.pop_at(&mut out, 2.0 + i as f64);
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

    // ------------------------------------------------------------------
    // Peer-fpp adoption
    // ------------------------------------------------------------------

    /// The first packet of a stream adopts the peer's frames-per-packet as it
    /// adopts its channel count: every fpp-derived quantity follows, the
    /// packet plays back whole at the adopted size, and `fpp()` / `latency_ms()`
    /// report the adopted value. The regulator is built for 128, as the
    /// session builds it, so every other fpp here is a genuine change.
    #[test]
    fn test_first_packet_adopts_peer_fpp_across_the_range() {
        for fpp in [MIN_FPP, 64, 128, 256, MAX_FPP] {
            for channels in [1usize, 2, MAX_CHANNELS] {
                let ctx = format!("fpp={fpp} ch={channels}");
                let mut reg = Regulator::with_params(2, 128, 48_000, 5.0);
                let packets: Vec<Vec<f32>> = (0..3).map(|seq| ramp_packet(seq, fpp, channels)).collect();
                push_stored(&reg, 0, channels, &packets[0], 0.0);
                assert_eq!(reg.fpp(), fpp, "{ctx}");
                assert_eq!(reg.channels(), channels, "{ctx}");
                assert_eq!(reg.shared.num_slots.load(Ordering::Relaxed), num_slots(fpp), "{ctx}");

                // Past the 5 ms tolerance: the first pop plays packet 0, whole.
                let mut out = vec![0.0f32; fpp * channels];
                assert!(reg.pop_at(&mut out, 10.0), "{ctx}: adopted packet must play as real audio");
                assert_eq!(out, packets[0], "{ctx}");

                // Two more arrive and wait, at the adopted size.
                push_stored(&reg, 1, channels, &packets[1], 11.0);
                push_stored(&reg, 2, channels, &packets[2], 12.0);
                let expected_ms = reg.frames_to_ms(2 * fpp) as f32;
                assert!((reg.latency_ms() - expected_ms).abs() < 1e-3, "{ctx}: latency {}", reg.latency_ms());

                // The consumer re-derived its geometry from the adopted fpp.
                let c = consumer(&mut reg);
                assert_eq!(c.fpp, fpp, "{ctx}");
                assert_eq!(c.up_to_now, c.packets_in_past * fpp, "{ctx}");
                assert!(c.up_to_now <= UP_TO_NOW_MAX, "{ctx}: history outgrew its preallocation");
                assert_eq!(c.tolerance_ms, 5.0, "{ctx}: a fixed tolerance is not an fpp-derived value");
            }
        }
    }

    /// An fpp outside the supported range cannot be adopted: it is refused on
    /// both sides of each edge, the regulator keeps its construction geometry,
    /// and — because the stream never started — a valid first packet can still
    /// adopt afterwards.
    #[test]
    fn test_first_packet_fpp_outside_range_is_refused() {
        let reg = Regulator::with_params(1, 128, 48_000, 5.0);
        for (i, fpp) in [0, MIN_FPP - 1, MAX_FPP + 1].into_iter().enumerate() {
            assert_eq!(
                reg.push_at(0, 1, fpp, &vec![0.5; fpp], 0.0),
                PushOutcome::UnsupportedFpp { got: fpp }
            );
            assert_eq!(reg.stats().packets_rejected, i as u64 + 1);
        }
        assert!(!reg.is_initialized(), "a refused first packet must not start the stream");
        assert_eq!(reg.fpp(), 128);

        push_stored(&reg, 0, 1, &vec![0.5; MAX_FPP], 0.0);
        assert_eq!(reg.fpp(), MAX_FPP);
    }

    /// The ring's depth is a constant time horizon, not a constant packet
    /// count: the largest power of two that fits the frame budget, capped. The
    /// power of two keeps `seq % num_slots` continuous across the `u16` wrap,
    /// and the cap keeps the ring inside the half-space window of
    /// `seq_is_newer`.
    #[test]
    fn test_num_slots_is_a_constant_horizon_power_of_two_below_the_cap() {
        for fpp in MIN_FPP..=MAX_FPP {
            let n = num_slots(fpp);
            assert!(n.is_power_of_two(), "fpp={fpp}: {n} slots");
            assert!(n <= MAX_SLOTS, "fpp={fpp}: {n} slots exceeds the cap");
            // Over the supported fpp range the cap never binds above the budget, so
            // the budget is a hard ceiling (a round-up bug would breach it).
            assert!(n * fpp <= SLOT_FRAME_BUDGET, "fpp={fpp}: {n} slots overshoots the frame budget");
            assert!(
                n == MAX_SLOTS || 2 * n * fpp > SLOT_FRAME_BUDGET,
                "fpp={fpp}: {n} slots leaves room for twice as many"
            );
            assert!(65_536 % n == 0 && n * 4 <= 65_536, "fpp={fpp}: {n} slots");
        }
        // Memory, the reason for the budget: never more than the old 4096-slot ring at 128.
        assert!(MAX_FPP * num_slots(MAX_FPP) <= 4096 * HIST_FPP);
    }

    /// Sequence numbers wrap at 65536 while the ring wraps at its own depth;
    /// for a depth other than 4096 the stream must still play in order across
    /// the seam. 64 frames adopts 8192 slots.
    #[test]
    fn test_sequence_wraparound_at_an_adopted_ring_depth() {
        const FPP: usize = 64;
        let reg = Regulator::with_params(1, 128, 48_000, 5.0);
        let first = u16::MAX - 2;
        let packets: Vec<Vec<f32>> = (0..6).map(|i| ramp_packet(i, FPP, 1)).collect();
        let mut out = vec![0.0f32; FPP];
        for (i, packet) in packets.iter().enumerate() {
            let seq = first.wrapping_add(i as u16);
            // A stream starts on its newest packet, so play each as it arrives.
            let now = 10.0 * (i + 1) as f64;
            // Popped past the opening 5 ms tolerance window.
            push_stored(&reg, seq, 1, packet, now - 6.0);
            assert!(reg.pop_at(&mut out, now), "packet {i} (seq {seq}) must play as real audio");
            assert_eq!(&out, packet, "packet {i} (seq {seq}) must play in order across the sequence wrap");
        }
        assert_ne!(reg.shared.num_slots.load(Ordering::Relaxed), 4096);
        assert_eq!(last_seq_out_opt(&reg), Some(first.wrapping_add(5)));
    }

    /// Concealment after adoption uses the reconfigured model: the Burg
    /// training window, the prediction tail and the crossfade ramps are all
    /// sized from the *adopted* fpp. A stale `up_to_now` or `beyond_now` (the
    /// construction fpp's) slices the wrong prefix. A regulator that adopts
    /// must therefore conceal exactly as one built for that fpp does — and the
    /// concealment must actually continue the tone, not play silence.
    #[test]
    fn test_loss_burst_at_adopted_fpp_conceals_like_a_regulator_built_for_it() {
        let concealed = |built_fpp: usize, fpp: usize| -> Vec<f32> {
            let tone = |n: usize| (n as f32 * 0.07).sin() * 0.5;
            let reg = Regulator::with_params(1, built_fpp, 48_000, 5.0);
            // Enough real packets to prime the predictor at any history length.
            let played = 3 * packets_in_past(fpp) + 4;
            let mut out = vec![0.0f32; fpp];
            for seq in 0..played {
                let t = seq as f64 * reg.frames_to_ms(fpp);
                let packet: Vec<f32> = (0..fpp).map(|i| tone(seq * fpp + i)).collect();
                push_stored(&reg, seq as u16, 1, &packet, t);
                reg.pop_at(&mut out, t + 1.0);
            }
            // The next packet never arrives.
            let real = reg.pop_at(&mut out, played as f64 * reg.frames_to_ms(fpp) + 1.0);
            assert!(!real, "fpp={fpp}: nothing new arrived, so this is concealment");
            out
        };
        for fpp in [MIN_FPP, 32, 256, MAX_FPP] {
            let adopted = concealed(128, fpp);
            assert_eq!(adopted, concealed(fpp, fpp), "fpp={fpp}: adoption must reconfigure the model fully");
            let energy: f32 = adopted.iter().map(|s| s * s).sum();
            assert!(energy > 0.1, "fpp={fpp}: concealment is silent (energy {energy})");
        }
    }

    // ------------------------------------------------------------------
    // Frame-granular pop (`pop_frames_at`)
    // ------------------------------------------------------------------

    struct CallbackRun {
        /// Output served after the settling window, interleaved.
        served: Vec<f32>,
        /// Glitches and packets played after the settling window.
        glitches: u64,
        played: u64,
        reg: Regulator,
    }

    /// Sample rate shared by `run_callbacks` and `callbacks_for_ms`, which is
    /// called before any `Regulator` exists to ask it.
    const CALLBACK_RUN_RATE: u32 = 48_000;

    /// `built_fpp` is the geometry the regulator is constructed with; the peer
    /// sends `fpp`, which is adopted from its first packet when they differ.
    fn run_callbacks(
        built_fpp: usize,
        fpp: usize,
        frames: usize,
        channels: usize,
        callbacks: usize,
        tolerance_ms: f64,
    ) -> CallbackRun {
        let reg = Regulator::with_params(channels, built_fpp, CALLBACK_RUN_RATE, tolerance_ms).with_callback_capacity(frames);
        let mut served = Vec::new();
        let mut settled = (0, 0);
        let mut out = vec![0.0f32; frames * MAX_CHANNELS];
        drive_callbacks(&reg, (fpp, frames, channels), callbacks, |seq| ramp_packet(seq, fpp, channels), |k, now| {
            if k == SETTLE_CALLBACKS {
                let stats = reg.stats();
                settled = (stats.glitches, stats.packets_played);
            }
            let c = reg.pop_frames_at(&mut out, frames, now);
            assert_eq!(c, channels);
            if k >= SETTLE_CALLBACKS {
                served.extend_from_slice(&out[..frames * c]);
            }
        });
        let stats = reg.stats();
        CallbackRun { served, glitches: stats.glitches - settled.0, played: stats.packets_played - settled.1, reg }
    }

    /// `callbacks` covering `ms` of wall clock at `frames` per callback.
    fn callbacks_for_ms(ms: f64, frames: usize) -> usize {
        (ms / frames_to_ms(frames, CALLBACK_RUN_RATE)) as usize
    }

    /// Callback size and packet size are independent: whatever the ratio, the
    /// peer's stream arrives sample-exact — no gap, repeat or drop at a
    /// packet/callback seam — with no concealment, and exactly one packet is
    /// consumed per packet of output.
    #[test]
    fn frame_pop_is_sample_exact_across_fpp_and_callback_ratios() {
        // (fpp, callback frames): callback < packet, ==, >, and a non-divisor.
        // The peer's fpp is adopted from its first packet, so every ratio also
        // runs against a regulator built for a different fpp, as the session
        // builds one (128): including 1024 against a 128-frame callback.
        for &(fpp, frames) in &[
            (16, 128),
            (32, 128),
            (64, 128),
            (128, 128),
            (256, 128),
            (128, 64),
            (512, 128),
            (1024, 128),
            (128, 100),
            (100, 128),
        ] {
          for built_fpp in [fpp, 128] {
            for channels in [1usize, 2, 5] {
                let ctx = format!("built={built_fpp} fpp={fpp} frames={frames} ch={channels}");
                let run = run_callbacks(built_fpp, fpp, frames, channels, 400, 5.0);
                let total = run.served.len() / channels;
                let start_frame = (run.served[0] as usize - 1) / 16;
                for i in 0..total {
                    for ch in 0..channels {
                        assert_eq!(run.served[i * channels + ch], ramp(start_frame + i, ch), "{ctx}: frame {i} ch {ch}");
                    }
                }
                assert_eq!(run.glitches, 0, "{ctx}: spurious concealment");
                // At most one packet is in flight at each end of the window.
                let consumed = (total / fpp) as u64;
                assert!(
                    (consumed..=consumed + 2).contains(&run.played),
                    "{ctx}: {} packets played for {consumed} packets of output",
                    run.played
                );
            }
          }
        }
    }

    /// The pull clock is the audio callback: one tick per call at the true
    /// callback interval, however many packets that call consumes — including
    /// none (callback smaller than the packet). Spreading a callback's packets
    /// over artificial intervals would erase real callback jitter from the
    /// statistics that set the auto tolerance's floor; zero-length intervals
    /// between same-callback packets must not tick at all.
    #[test]
    fn pull_clock_ticks_once_per_callback_at_the_callback_interval() {
        for &(fpp, frames) in &[(32, 128), (64, 128), (128, 128), (256, 128), (128, 64), (512, 128), (256, 64), (128, 100)] {
            // Past AUTO_INIT_DURATION_MS and the headroom warmup.
            let mut run = run_callbacks(fpp, fpp, frames, 2, callbacks_for_ms(12_000.0, frames), -1.0);
            let cb_period = run.reg.frames_to_ms(frames);
            let packet_period = run.reg.frames_to_ms(fpp);
            let pull = &consumer(&mut run.reg).pull_stats;
            assert!(
                (pull.last_mean - cb_period).abs() < 0.01 && pull.last_std_dev < 0.01,
                "fpp={fpp} frames={frames}: pull interval {:.3}±{:.3} ms, expected the {cb_period:.3} ms callback interval",
                pull.last_mean,
                pull.last_std_dev
            );
            // A steady peer settles tolerance near the slower of the two clocks.
            let tolerance = run.reg.stats().tolerance_ms;
            assert!(
                tolerance <= 1.6 * cb_period.max(packet_period),
                "fpp={fpp} frames={frames}: tolerance {tolerance:.2} ms for a steady peer"
            );
            assert_eq!(run.glitches, 0, "fpp={fpp} frames={frames}");
        }
    }

    /// Frames already pulled from the jitter buffer but not yet played are
    /// latency too.
    #[test]
    fn latency_includes_pulled_unplayed_frames() {
        const FPP: usize = 256;
        const FRAMES: usize = 128;
        let run = run_callbacks(FPP, FPP, FRAMES, 1, 100, 5.0);
        let reg = &run.reg;
        // A packet is twice the callback, so every other callback leaves one
        // callback's worth of the packet queued. Find that callback.
        let mut out = vec![0.0f32; FRAMES];
        let mut now = 1000.0;
        let mut queued_ms = 0.0;
        for _ in 0..2 {
            now += reg.frames_to_ms(FRAMES);
            reg.pop_frames_at(&mut out, FRAMES, now);
            queued_ms = reg.latency_ms() as f64 - reg.frames_to_ms(reg.depth() as usize * FPP);
            if queued_ms > 0.0 {
                break;
            }
        }
        assert!(
            (queued_ms - reg.frames_to_ms(FRAMES)).abs() < 1e-3,
            "queued frames must reach latency_ms: {queued_ms} ms"
        );
    }

    /// Pops before the peer starts must not delay the first real audio: startup
    /// silence is queued per callback, not per packet, so no silent packet tail
    /// is left ahead of the stream (PR #113 review).
    #[test]
    fn prestart_pops_do_not_delay_first_real_audio() {
        for &(fpp, frames) in &[(512usize, 100usize), (512, 128), (256, 128), (128, 100)] {
            // Frames from the peer's first packet to the first real sample.
            let first_real_frame = |prestart: usize| {
                let reg = Regulator::with_params(2, fpp, CALLBACK_RUN_RATE, 5.0).with_callback_capacity(frames);
                let mut out = vec![0.0f32; frames * MAX_CHANNELS];
                let mut now = 21.0;
                for _ in 0..prestart {
                    reg.pop_frames_at(&mut out, frames, now);
                    // Queued startup silence would be reported as jitter-buffer latency.
                    assert_eq!(reg.latency_ms(), 0.0, "fpp={fpp} frames={frames}: pre-start silence counted as latency");
                    now += reg.frames_to_ms(frames);
                }
                let (start, period) = (now, reg.frames_to_ms(fpp));
                let mut pushed = 0usize;
                for k in prestart.. {
                    while start + pushed as f64 * period <= now {
                        push_stored(&reg, pushed as u16, 2, &ramp_packet(pushed, fpp, 2), start + pushed as f64 * period);
                        pushed += 1;
                    }
                    let c = reg.pop_frames_at(&mut out, frames, now);
                    if let Some(f) = (0..frames).find(|f| out[f * c] != 0.0) {
                        return (k - prestart) * frames + f;
                    }
                    now += reg.frames_to_ms(frames);
                }
                unreachable!()
            };
            let baseline = first_real_frame(0);
            for prestart in 1..5 {
                assert_eq!(first_real_frame(prestart), baseline, "fpp={fpp} frames={frames} prestart={prestart}");
            }
        }
    }
}
