use std::sync::atomic::Ordering;

use crate::audio::params::{AudioParams, MAX_DB, MIN_DB, decode_db, decode_volume, encode_db};
use crate::audio::regulator::Regulator;
use crate::audio::ring_buffer::RingBuffer;

/// Peak hold time in process calls (~48kHz / 128 samples = ~375 calls/sec)
/// Hold peak for about 1.5 seconds
const PEAK_HOLD_FRAMES: u32 = 560;
/// Peak decay rate in dB per process call (smooth falloff)
const PEAK_DECAY_RATE: f32 = 0.15;

// ==============================================================================
// Pure DSP math — free functions operating on plain slices/scalars.
// These are the single source of truth; `AudioProcessor` methods delegate here.
// ==============================================================================

/// Convert linear amplitude to decibels, clamped to [MIN_DB, MAX_DB].
pub(crate) fn amplitude_to_db(amplitude: f32) -> f32 {
    if amplitude <= 0.0 {
        MIN_DB
    } else {
        (20.0 * amplitude.log10()).max(MIN_DB).min(MAX_DB)
    }
}

/// Convert dB to linear gain multiplier.
pub(crate) fn db_to_linear(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Apply `gain` to every sample in `input`, writing into `output`.
/// Output samples are clamped to `[-1.0, 1.0]`.
/// `output` must be at least as long as `input`.
pub(crate) fn apply_gain(input: &[f32], gain: f32, output: &mut [f32]) {
    for (out, &inp) in output.iter_mut().zip(input) {
        *out = (inp * gain).clamp(-1.0, 1.0);
    }
}

/// Compute the RMS (root mean square) of a sample slice.
/// Returns `0.0` for an empty slice.
pub(crate) fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_squares / samples.len() as f32).sqrt()
}

/// Compute the next peak-hold/decay state given the current audio level and
/// the previously stored peak state.
///
/// Returns `(new_peak_db, new_hold_counter)`.
///
/// Rules:
/// - If `current_db >= peak_db`: new peak detected → reset hold counter to
///   `PEAK_HOLD_FRAMES`.
/// - Else if `hold_counter > 0`: still in hold window → decrement counter,
///   peak stays put.
/// - Else: hold expired → apply `PEAK_DECAY_RATE` (floored at `MIN_DB`).
pub(crate) fn compute_peak_update(current_db: f32, peak_db: f32, hold_counter: u32) -> (f32, u32) {
    if current_db >= peak_db {
        (current_db, PEAK_HOLD_FRAMES)
    } else if hold_counter > 0 {
        (peak_db, hold_counter - 1)
    } else {
        ((peak_db - PEAK_DECAY_RATE).max(MIN_DB), 0)
    }
}

// ==============================================================================
// AudioProcessor — delegates all math to the free functions above
// ==============================================================================

/// Core audio processor for real-time audio processing
/// Handles volume metering, gain control, monitoring, and network audio
pub struct AudioProcessor {
    params: &'static AudioParams,
    /// Ring buffer for sending local audio to network (audio device → worklet → main thread → network)
    local_to_network_buffer: Option<*mut RingBuffer>,
    /// Jitter buffer for receiving audio from network (network → main thread → jitter buffer → worklet → audio device)
    network_to_local_buffer: Option<*mut Regulator>,
    /// Temporary buffer for gained audio (mono from mic)
    gained_buffer: Vec<f32>,
    /// Temporary buffer for remote audio (mono, after downmix)
    remote_buffer: Vec<f32>,
    /// Buffer for stereo send (mono duplicated to both channels)
    stereo_buffer: Vec<f32>,
    /// Buffer for stereo receive (before downmix to mono)
    stereo_receive_buffer: Vec<f32>,
}

impl AudioProcessor {
    pub fn new(params: &'static AudioParams) -> Self {
        Self {
            params,
            local_to_network_buffer: None,
            network_to_local_buffer: None,
            gained_buffer: vec![0.0; 128],
            remote_buffer: vec![0.0; 128],
            stereo_buffer: vec![0.0; 256], // 128 samples * 2 channels
            stereo_receive_buffer: vec![0.0; 256], // 128 samples * 2 channels
        }
    }

    /// Create processor with network audio support
    /// - local_to_network_buffer: ring buffer for sending local audio to network
    /// - network_to_local_buffer: jitter buffer for receiving audio from network
    pub fn with_network(
        params: &'static AudioParams,
        local_to_network_buffer: *mut RingBuffer,
        network_to_local_buffer: *mut Regulator,
    ) -> Self {
        Self {
            params,
            local_to_network_buffer: if local_to_network_buffer.is_null() { None } else { Some(local_to_network_buffer) },
            network_to_local_buffer: if network_to_local_buffer.is_null() { None } else { Some(network_to_local_buffer) },
            gained_buffer: vec![0.0; 128],
            remote_buffer: vec![0.0; 128],
            stereo_buffer: vec![0.0; 256], // 128 samples * 2 channels
            stereo_receive_buffer: vec![0.0; 256], // 128 samples * 2 channels
        }
    }

    /// Process audio: calculate volume levels, handle network audio, and generate output
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> bool {
        // Increment callback counter for stats tracking
        self.params.callback_count.fetch_add(1, Ordering::Relaxed);
        
        // Get input gain (dB) and convert to linear
        let input_gain_db = self.params.input_gain_db.load(Ordering::Relaxed) as f32 / 100.0;
        let input_gain_linear = db_to_linear(input_gain_db);
        
        // Ensure buffers are correct size
        if self.gained_buffer.len() != input.len() {
            self.gained_buffer.resize(input.len(), 0.0);
            self.remote_buffer.resize(input.len(), 0.0);
        }

        // Apply input gain to local audio (clamped to [-1.0, 1.0])
        apply_gain(input, input_gain_linear, &mut self.gained_buffer);

        // Calculate RMS for volume metering
        let rms = compute_rms(&self.gained_buffer);
        let current_db = amplitude_to_db(rms);
        
        // Store dB level
        self.params.db_level.store(encode_db(current_db), Ordering::Relaxed);

        // Peak level tracking with hold and decay
        self.update_peak_level(current_db);

        // Send local audio to network (if enabled)
        self.send_local_to_network();

        // Receive remote audio from network. Fills `remote_buffer` with the regulator's
        // output (real, Burg-predicted concealment, or intentional silence) when a network
        // buffer is connected, or with zeros otherwise — so we can mix it unconditionally.
        self.receive_from_network();

        // Generate output: mix monitor + remote audio
        let monitor_volume = decode_volume(self.params.monitor_volume.load(Ordering::Relaxed));
        let output_volume  = decode_volume(self.params.output_volume.load(Ordering::Relaxed));
        
        let len = input.len().min(output.len());
        for i in 0..len {
            // Start with remote audio (zeros if no network connected)
            let mut out_sample = self.remote_buffer[i];

            // Add local monitor audio (if enabled)
            if monitor_volume > 0.0 {
                out_sample += self.gained_buffer[i] * monitor_volume;
            }

            // Apply output volume and clamp
            output[i] = (out_sample * output_volume).clamp(-1.0, 1.0);
        }

        true
    }

    /// Update peak level with hold and decay
    fn update_peak_level(&self, current_db: f32) {
        let current_peak_db = decode_db(self.params.peak_db_level.load(Ordering::Relaxed));
        let hold_counter = self.params.peak_hold_counter.load(Ordering::Relaxed);

        let (new_peak_db, new_hold_counter) = compute_peak_update(current_db, current_peak_db, hold_counter);

        self.params.peak_db_level.store(encode_db(new_peak_db), Ordering::Relaxed);
        self.params.peak_hold_counter.store(new_hold_counter, Ordering::Relaxed);
    }

    /// Send local audio to network via ring buffer
    fn send_local_to_network(&mut self) {
        let Some(buffer_ptr) = self.local_to_network_buffer else {
            return;
        };

        let buffer = unsafe { &mut *buffer_ptr };
        
        if !buffer.is_streaming() {
            return;
        }

        let output_channels = self.params.get_output_channels();
        
        if output_channels >= 2 {
            // Duplicate mono to stereo (interleaved: L R L R ...)
            let mono_len = self.gained_buffer.len();
            let stereo_len = mono_len * 2;
            
            // Resize stereo buffer if needed
            if self.stereo_buffer.len() != stereo_len {
                self.stereo_buffer.resize(stereo_len, 0.0);
            }
            
            for (i, &sample) in self.gained_buffer.iter().enumerate() {
                self.stereo_buffer[i * 2] = sample;     // Left channel
                self.stereo_buffer[i * 2 + 1] = sample; // Right channel
            }
            
            buffer.write(&self.stereo_buffer);
        } else {
            // Write mono directly
            buffer.write(&self.gained_buffer);
        }
    }

    /// Receive remote audio from network via jitter buffer.
    ///
    /// Always fills `remote_buffer` with the appropriate audio for this callback:
    /// the regulator's output (real packet, Burg-predicted concealment, or intentional
    /// silence) when a network buffer is connected, or zeros when none is attached.
    /// `Regulator::pop()`'s return value is informational metadata (real vs. concealed)
    /// and must NOT gate playback — concealed audio is the entire point of jitter
    /// buffering and must be played to avoid clicks.
    fn receive_from_network(&mut self) {
        let Some(buffer_ptr) = self.network_to_local_buffer else {
            self.remote_buffer.fill(0.0);
            return;
        };

        let output_channels = self.params.get_output_channels();

        if output_channels >= 2 {
            // Read stereo packet, then downmix to mono for playback
            let mono_len = self.remote_buffer.len();
            let stereo_len = mono_len * 2;

            // Ensure stereo receive buffer is correct size
            if self.stereo_receive_buffer.len() != stereo_len {
                self.stereo_receive_buffer.resize(stereo_len, 0.0);
            }

            // Read stereo from jitter buffer (always populates the buffer; pop()'s bool
            // distinguishes real vs concealed but is irrelevant for mixing)
            unsafe { (*buffer_ptr).pop(&mut self.stereo_receive_buffer) };

            // Downmix stereo to mono (average L+R)
            for i in 0..mono_len {
                let left = self.stereo_receive_buffer[i * 2];
                let right = self.stereo_receive_buffer[i * 2 + 1];
                self.remote_buffer[i] = (left + right) * 0.5;
            }
        } else {
            // Read mono directly
            unsafe { (*buffer_ptr).pop(&mut self.remote_buffer) };
        }
    }
}

// ==============================================================================
// Tests
// ==============================================================================
//
// Run on the native target via `npm run test`.
// Only the pure DSP math functions are tested here; the unsafe ring-buffer /
// regulator pointer paths are covered structurally elsewhere.
#[cfg(test)]
mod tests {
    use super::*;

    /// Tolerance for floating-point comparisons.
    const EPS: f32 = 1e-4;

    // --- db_to_linear / amplitude_to_db -----------------------------------------

    #[test]
    fn test_db_linear_inversion_at_unity() {
        // 0 dB == 1.0 in both directions.
        assert!((db_to_linear(0.0) - 1.0).abs() < EPS);
        assert!((amplitude_to_db(1.0) - 0.0).abs() < EPS);
    }

    #[test]
    fn test_db_linear_roundtrip_at_known_points() {
        // dB → linear → dB must be identity within float tolerance.
        for &db in &[-60.0_f32, -30.0, -20.0, -6.0, 0.0] {
            let roundtrip = amplitude_to_db(db_to_linear(db));
            assert!(
                (roundtrip - db).abs() < EPS,
                "round-trip failed at {db} dB: got {roundtrip}"
            );
        }
    }

    #[test]
    fn test_amplitude_to_db_silence_clamps_to_min() {
        assert!((amplitude_to_db(0.0) - MIN_DB).abs() < EPS);
        assert!((amplitude_to_db(-1.0) - MIN_DB).abs() < EPS);
    }

    // --- apply_gain -------------------------------------------------------------

    #[test]
    fn test_apply_gain_unity() {
        let input = [0.25f32, -0.5, 0.75, -1.0, 1.0];
        let mut output = [0.0f32; 5];
        apply_gain(&input, 1.0, &mut output);
        for (i, (&inp, &out)) in input.iter().zip(output.iter()).enumerate() {
            assert!((out - inp).abs() < EPS, "unity gain mismatch at index {i}");
        }
    }

    #[test]
    fn test_apply_gain_clamped_above_full_scale() {
        // At full scale (±1.0) with any gain > 1.0, the result must clamp to ±1.0.
        let mut out = [0.0f32; 1];
        apply_gain(&[1.0], 100.0, &mut out);
        assert!((out[0] - 1.0).abs() < EPS, "positive over-gain must clamp to 1.0");

        apply_gain(&[-1.0], 100.0, &mut out);
        assert!((out[0] - (-1.0)).abs() < EPS, "negative over-gain must clamp to -1.0");
    }

    #[test]
    fn test_apply_gain_scales_and_clamps() {
        let input = [0.5f32, -0.5, 0.4, -0.4];
        let mut output = [0.0f32; 4];

        // Gain of 2.0: 0.5*2=1.0 (exactly at the ceiling), 0.4*2=0.8 (no clamp).
        apply_gain(&input, 2.0, &mut output);
        assert!((output[0] - 1.0).abs() < EPS);
        assert!((output[1] - (-1.0)).abs() < EPS);
        assert!((output[2] - 0.8).abs() < EPS);
        assert!((output[3] - (-0.8)).abs() < EPS);
    }

    #[test]
    fn test_apply_gain_silence() {
        let input = [1.0f32, -1.0, 0.5];
        let mut output = [0.0f32; 3];
        apply_gain(&input, 0.0, &mut output);
        for &s in &output {
            assert!(s.abs() < EPS, "zero gain must produce silence");
        }
    }

    // --- compute_rms ------------------------------------------------------------

    #[test]
    fn test_rms_silence() {
        let silence = vec![0.0f32; 128];
        assert!(compute_rms(&silence).abs() < EPS, "RMS of silence must be 0.0");
    }

    #[test]
    fn test_rms_empty_slice() {
        assert!(compute_rms(&[]).abs() < EPS, "RMS of empty slice must be 0.0");
    }

    #[test]
    fn test_rms_full_scale_sine() {
        // A full-scale sine has theoretical RMS = 1/√2 ≈ 0.7071.
        // With 1024 samples we get very close.
        let n = 1024usize;
        let samples: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * i as f32 / n as f32).sin())
            .collect();
        let rms = compute_rms(&samples);
        let expected = 1.0f32 / 2.0f32.sqrt(); // ≈ 0.70711
        assert!(
            (rms - expected).abs() < 1e-3,
            "full-scale sine RMS: expected ≈{expected:.5}, got {rms:.5}"
        );
    }

    #[test]
    fn test_rms_dc_offset() {
        // A constant signal of value `v` has RMS == |v|.
        let dc = vec![0.5f32; 64];
        assert!((compute_rms(&dc) - 0.5).abs() < EPS, "DC RMS must equal |v|");

        let dc_neg = vec![-0.5f32; 64];
        assert!((compute_rms(&dc_neg) - 0.5).abs() < EPS, "negative DC RMS must be 0.5");
    }

    // --- compute_peak_update ----------------------------------------------------

    #[test]
    fn test_peak_new_peak_resets_hold_counter() {
        let (new_peak, new_counter) = compute_peak_update(-10.0, -20.0, 0);
        assert!((new_peak - (-10.0)).abs() < EPS, "new peak must be adopted");
        assert_eq!(new_counter, PEAK_HOLD_FRAMES, "hold counter must reset to PEAK_HOLD_FRAMES");
    }

    #[test]
    fn test_peak_hold_phase_decrements_counter() {
        // While hold_counter > 0 the peak stays and counter decrements.
        let (peak_after, counter_after) = compute_peak_update(-20.0, -10.0, 5);
        assert!((peak_after - (-10.0)).abs() < EPS, "peak must be held");
        assert_eq!(counter_after, 4, "counter must decrement by 1");
    }

    #[test]
    fn test_peak_decay_phase_applies_decay_rate() {
        // When hold_counter == 0 and no new peak, the peak decays by PEAK_DECAY_RATE.
        let peak_db = -10.0f32;
        let (decayed, counter) = compute_peak_update(-30.0, peak_db, 0);
        let expected = (peak_db - PEAK_DECAY_RATE).max(MIN_DB);
        assert!((decayed - expected).abs() < EPS, "peak must decay by PEAK_DECAY_RATE");
        assert_eq!(counter, 0, "counter stays at 0 during decay");
    }

    #[test]
    fn test_peak_decay_does_not_go_below_min_db() {
        // Decay from a level very close to MIN_DB must floor at MIN_DB.
        let near_floor = MIN_DB + 0.05; // less than one decay step above floor
        let (floored, _) = compute_peak_update(MIN_DB - 10.0, near_floor, 0);
        assert!(
            (floored - MIN_DB).abs() < EPS,
            "decayed peak must not go below MIN_DB"
        );
    }

    #[test]
    fn test_peak_hold_then_decay_sequence() {
        // Simulate a full hold-then-decay scenario over several ticks.
        let signal_peak = -5.0f32;
        let low_level = -40.0f32;

        // Tick 1: signal hits a new peak.
        let (p, c) = compute_peak_update(signal_peak, MIN_DB, 0);
        assert!((p - signal_peak).abs() < EPS);
        assert_eq!(c, PEAK_HOLD_FRAMES);

        // Tick 2: level drops but hold keeps the peak.
        let (p2, c2) = compute_peak_update(low_level, p, c);
        assert!((p2 - signal_peak).abs() < EPS, "peak held during hold window");
        assert_eq!(c2, PEAK_HOLD_FRAMES - 1);

        // Exhaust hold window by running PEAK_HOLD_FRAMES - 1 more ticks.
        let mut peak = p2;
        let mut counter = c2;
        for _ in 0..(PEAK_HOLD_FRAMES - 1) {
            let (np, nc) = compute_peak_update(low_level, peak, counter);
            peak = np;
            counter = nc;
        }
        assert_eq!(counter, 0, "hold window must be fully exhausted");
        assert!((peak - signal_peak).abs() < EPS, "peak unchanged during hold");

        // Next tick: decay begins.
        let (decayed, dc) = compute_peak_update(low_level, peak, counter);
        assert!(decayed < peak, "peak must start decaying after hold expires");
        assert!((decayed - (signal_peak - PEAK_DECAY_RATE).max(MIN_DB)).abs() < EPS);
        assert_eq!(dc, 0);
    }
}
