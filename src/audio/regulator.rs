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
/// Maximum number of slots for packet storage
const NUM_SLOTS: usize = 4096;
/// Maximum samples per packet (8 channels * 1024 frames)
const MAX_PACKET_SAMPLES: usize = 8192;
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

/// A slot for storing an incoming packet (pre-allocated, no heap allocations)
struct PacketSlot {
    /// Arrival timestamp in milliseconds
    timestamp: f64,
    /// Number of valid samples in the data array
    sample_count: usize,
    /// Audio data (interleaved channels) - pre-allocated fixed size
    data: [f32; MAX_PACKET_SAMPLES],
}

impl PacketSlot {
    fn new() -> Self {
        Self {
            timestamp: 0.0,
            sample_count: 0,
            data: [0.0; MAX_PACKET_SAMPLES],
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
            slots.push(Some(PacketSlot::new()));
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
    /// This method performs NO heap allocations - all buffers are pre-allocated.
    ///
    /// # Arguments
    /// * `seq_num` - Packet sequence number (u16 wraps at 65535)
    /// * `samples` - Interleaved audio samples
    /// * `now_ms` - Current timestamp in milliseconds
    fn push_internal(&mut self, seq_num: u16, samples: &[f32], now_ms: f64) {
        if samples.len() > MAX_PACKET_SAMPLES {
            return;
        }

        let current = self.last_seq_in.load(Ordering::Acquire);

        // Initialize on first packet
        if current == SEQ_NONE {
            self.start_time_ms = now_ms;
        }

        let slot_idx = (seq_num as usize) % NUM_SLOTS;

        // Copy packet data into pre-allocated slot (NO allocation)
        if let Some(ref mut slot) = self.slots[slot_idx] {
            let sample_count = samples.len().min(self.samples_per_packet).min(MAX_PACKET_SAMPLES);
            slot.timestamp = now_ms - self.start_time_ms;
            slot.sample_count = sample_count;
            slot.data[..sample_count].copy_from_slice(&samples[..sample_count]);
        }

        // Update last sequence number using wrapping comparison
        // A packet is "newer" if the wrapping distance forward is less than half the space
        let should_update = if current == SEQ_NONE {
            true
        } else {
            let delta = seq_num.wrapping_sub(current as u16);
            delta < u16::MAX / 2
        };
        if should_update {
            self.last_seq_in.store(seq_num as i32, Ordering::Release);
        }
    }

    /// Push a received packet into the buffer.
    ///
    /// # Arguments
    /// * `sequence` - Packet sequence number (u16 wraps at 65535)
    /// * `samples` - Interleaved audio samples
    ///
    /// # Returns
    /// `true` if packet was accepted, `false` otherwise
    pub fn push(&mut self, sequence: u16, samples: &[f32]) -> bool {
        let now_ms = Self::now_ms();
        self.push_internal(sequence, samples, now_ms);
        true // Always return true for now
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
                            let idx = s * self.num_channels + ch;
                            channel.tmp_buf[s] = if idx < slot.sample_count {
                                slot.data[idx]
                            } else {
                                0.0
                            };
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
            let slot_idx = (last_out as usize) % NUM_SLOTS;
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

        // Use wrapping arithmetic to handle sequence number wraparound (u16 wraps at 65535)
        let new_pkts = last_seq_in.wrapping_sub(start_seq).wrapping_add(1) as usize;
        if new_pkts == 0 || new_pkts > NUM_SLOTS {
            return None;
        }

        let mut skipped = 0u64;
        let mut first_good_skipped: Option<(u16, usize, f64)> = None;

        // Find the best candidate (NO allocations - just return indices)
        let mut best_candidate: Option<(u16, usize, f64)> = None;

        for i in (0..new_pkts).rev() {
            let seq = last_seq_in.wrapping_sub(i as u16);
            let slot_idx = (seq as usize) % NUM_SLOTS;

            let timestamp = match &self.slots[slot_idx] {
                Some(slot) => slot.timestamp,
                None => continue,
            };

            // Skip packets that arrived too early (out of order)
            if let Some(last_out) = self.last_seq_out {
                let last_out_idx = (last_out as usize) % NUM_SLOTS;
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
        let last_out_idx = (last_out as usize) % NUM_SLOTS;

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

        // Reset slots without deallocating
        for slot in &mut self.slots {
            if let Some(ref mut s) = slot {
                s.timestamp = 0.0;
                s.sample_count = 0;
                // No need to zero the data array - sample_count tracks what's valid
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
        let total_samples = depth * self.samples_per_packet as u32;
        (total_samples as f32 / self.sample_rate as f32) * 1000.0
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

    #[test]
    fn test_burg_algorithm_creation() {
        let burg = BurgAlgorithm::new(128);
        assert_eq!(burg.order, 127);
        assert_eq!(burg.input_size, 128);
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
    fn test_regulator_creation() {
        let reg = Regulator::with_params(2, 128, 48000, -1.0);
        assert_eq!(reg.num_channels, 2);
        assert_eq!(reg.fpp, 128);
        assert_eq!(reg.sample_rate, 48000);
        assert!(reg.auto_mode);
    }

    #[test]
    fn test_regulator_push_pop() {
        let mut reg = Regulator::with_params(1, 128, 48000, 10.0);

        // Push a packet
        let samples: Vec<f32> = (0..128).map(|i| (i as f32 * 0.01).sin()).collect();
        reg.push_internal(0, &samples, 0.0);

        // Pop (should get silence during startup)
        let mut output = vec![0.0f32; 128];
        let result = reg.pop_internal(&mut output, 5.0);
        assert!(!result); // Still in startup

        // Pop after tolerance met
        reg.push_internal(1, &samples, 15.0);
        let _result = reg.pop_internal(&mut output, 15.0);
        // May or may not have data depending on timing
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
        reg.push_internal(near_max, &samples, 0.0);
        reg.push_internal(near_max.wrapping_add(1), &samples, 3.0);
        reg.push_internal(near_max.wrapping_add(2), &samples, 6.0); // This wraps to 0
        reg.push_internal(0, &samples, 9.0); // Already wrapped
        reg.push_internal(1, &samples, 12.0);
        
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

        if let Some(slot) = &mut reg.slots[(last_seq as usize) % NUM_SLOTS] {
            slot.timestamp = 40.0;
            slot.sample_count = reg.fpp;
            slot.data[..reg.fpp].copy_from_slice(&last_packet);
        }

        if let Some(slot) = &mut reg.slots[(good_seq as usize) % NUM_SLOTS] {
            slot.timestamp = 45.0;
            slot.sample_count = reg.fpp;
            slot.data[..reg.fpp].copy_from_slice(&packet);
        }

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

    /// After a full push/pop cycle that accumulates state (stats, sequence
    /// numbers, timing), `reset()` must scrub the regulator back to a
    /// freshly-constructed state. This covers the field-by-field cleanup
    /// required for safe stream reconnection.
    #[test]
    fn test_reset_clears_state_after_active_stream() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 5.0);
        let samples = vec![0.25f32; reg.fpp];

        // Drive a small stream through the regulator.
        reg.push_internal(0, &samples, 0.0);
        reg.push_internal(1, &samples, 2.0);
        let mut out = vec![0.0f32; reg.fpp];
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
        reg.last_stashed = Some((9, (9usize) % NUM_SLOTS));
        if let Some(slot) = &mut reg.slots[9 % NUM_SLOTS] {
            slot.timestamp = 100.0;
            slot.sample_count = reg.fpp;
            slot.data[..reg.fpp].copy_from_slice(&samples);
        }

        reg.reset();

        assert_eq!(
            reg.last_stashed, None,
            "stashed packet from prior connection must be cleared"
        );
        assert!(!reg.is_initialized());
        assert_eq!(reg.last_seq_out, None);

        // After reset, the first pop on a fresh stream should return silence
        // (startup), not the stale stashed buffer.
        reg.push_internal(0, &samples, 0.0);
        let mut out = vec![0.0f32; reg.fpp];
        let result = reg.pop_internal(&mut out, 1.0); // still inside tolerance window
        assert!(!result, "should not replay the pre-reset stash");
        assert!(out.iter().all(|s| *s == 0.0));
    }

    /// Once the buffer is drained, additional pops with no new input must
    /// increment the underrun counter exactly once per call and stay below the
    /// long-stall threshold so PLC concealment continues to run.
    #[test]
    fn test_underrun_counter_increments_when_no_new_packets() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 5.0);
        let samples = vec![0.25f32; reg.fpp];

        reg.push_internal(0, &samples, 0.0);
        let mut out = vec![0.0f32; reg.fpp];

        // First pop consumes the only buffered packet.
        let r1 = reg.pop_internal(&mut out, 10.0);
        assert!(r1, "first pop should return the real packet");
        assert_eq!(reg.pull_stats.underruns, 0);

        // Subsequent pops with no new pushes must increment the underrun
        // counter and report concealment (returns false).
        for i in 0..5 {
            let pop_time = 12.0 + i as f64 * 2.0;
            let result = reg.pop_internal(&mut out, pop_time);
            assert!(!result, "pop {i} should be an underrun");
        }
        assert_eq!(reg.pull_stats.underruns, 5);
        assert_eq!(reg.pull_stats.overruns, 0);
    }

    /// Push two packets with a 2-packet gap, then pop. The regulator should
    /// detect the gap, emit a PLC concealment block, and bump the overrun
    /// counter by the number of skipped packets.
    #[test]
    fn test_overrun_counter_increments_when_packets_skipped() {
        let mut reg = Regulator::with_params(1, 32, 48_000, 5.0);
        let samples = vec![0.5f32; reg.fpp];

        reg.push_internal(0, &samples, 0.0);
        let mut out = vec![0.0f32; reg.fpp];
        let r1 = reg.pop_internal(&mut out, 10.0);
        assert!(r1);
        assert_eq!(reg.last_seq_out, Some(0));

        // Skip seq 1, 2 — push seq 3 directly. With `skipped = 2`, the
        // regulator should treat this as a glitch, conceal, stash, and bump
        // overruns.
        reg.push_internal(3, &samples, 20.0);
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

        reg.push_internal(0, &samples, 0.0);
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
        reg.push_internal(1, &samples, 25.0);
        let r_resume = reg.pop_internal(&mut out, 30.0);
        assert!(r_resume, "PLC must disengage once a real packet is available");
        assert_eq!(reg.last_seq_out, Some(1));
        assert_eq!(reg.packet_count, packets_after_first + 1);
        let underruns_after = reg.pull_stats.underruns;

        // No further underruns when consumption keeps pace.
        reg.push_internal(2, &samples, 32.0);
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
        reg.push_internal(0, &samples, 0.0);
        reg.push_internal(1, &samples, 2.0);
        assert_eq!(reg.depth(), 0, "with no pop, read==write so depth is zero");

        // First pop sets last_seq_out; the in-flight buffer is consumed.
        let mut out = vec![0.0f32; reg.samples_per_packet];
        let r = reg.pop_internal(&mut out, 10.0);
        assert!(r);
        let seq_after_first_pop = reg.last_seq_out.expect("pop must set last_seq_out");

        // Push more packets without popping — depth grows by one per push.
        for (i, t) in (1u16..=3).zip([12.0_f64, 14.0, 16.0]) {
            let seq = seq_after_first_pop.wrapping_add(i);
            reg.push_internal(seq, &samples, t);
            assert_eq!(
                reg.depth() as u16,
                i,
                "depth should equal pushes-since-last-pop ({i})"
            );
        }

        // Latency = depth * samples_per_packet / sample_rate (in ms).
        let depth = reg.depth();
        let expected_ms =
            (depth as f32 * reg.samples_per_packet as f32 / reg.sample_rate as f32) * 1000.0;
        assert!(
            (reg.latency_ms() - expected_ms).abs() < 1e-3,
            "latency_ms ({}) should match depth*samples/sr ({expected_ms})",
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
        reg.push_internal(42, &samples, 0.0);
        reg.push_internal(43, &samples, 2.0);

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
}
