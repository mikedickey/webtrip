use std::sync::atomic::Ordering;

use crate::audio::params::{AudioParams, MAX_DB, MIN_DB, decode_db, decode_volume, encode_db};
use crate::audio::protocol::MAX_CHANNELS;
use crate::audio::regulator::Regulator;
use crate::audio::ring_buffer::RingBuffer;
use crate::audio::shared_ptr::SharedPtr;

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
///
/// # Panics
/// Panics if `output` is shorter than `input` — a shorter output would silently
/// drop samples, which is always a bug at the call site.
pub(crate) fn apply_gain(input: &[f32], gain: f32, output: &mut [f32]) {
    assert!(
        output.len() >= input.len(),
        "apply_gain: output buffer ({}) is shorter than input buffer ({})",
        output.len(),
        input.len(),
    );
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

/// The level meter's RMS: the loudest of `channels` planar channels'
/// individual RMS values, not their average — a single hot channel among
/// quiet ones must still show on the meter. `planar` is `channels * frames`
/// samples laid out `planar[ch * frames + frame]` (same layout `interleave_planar` reads).
pub(crate) fn max_channel_rms(planar: &[f32], channels: usize, frames: usize) -> f32 {
    (0..channels)
        .map(|ch| compute_rms(&planar[ch * frames..(ch + 1) * frames]))
        .fold(0.0f32, f32::max)
}

/// Downmix `channels`-wide interleaved audio to mono by averaging each
/// frame's channels, writing one sample per frame into `mono_out`.
///
/// Reads defensively past the end of `interleaved` as silence (treats a short
/// final frame as zero-padded) rather than panicking — this runs on the
/// realtime audio thread, where a malformed buffer must degrade gracefully,
/// not crash playback.
pub(crate) fn downmix_to_mono(interleaved: &[f32], channels: usize, mono_out: &mut [f32]) {
    let channels = channels.max(1);
    for (frame, out) in mono_out.iter_mut().enumerate() {
        let base = frame * channels;
        let mut sum = 0.0f32;
        for ch in 0..channels {
            sum += interleaved.get(base + ch).copied().unwrap_or(0.0);
        }
        *out = sum / channels as f32;
    }
}

/// Map `src_channels`-wide interleaved audio onto `out_channels` planar output
/// channels (`out[ch * frames + frame]`, `frames == src.len() / src_channels`),
/// per the target-behavior rules in the channel model plan:
///
/// - `out_channels == 1` → average all `src_channels` into it (delegates to
///   [`downmix_to_mono`] — one implementation, not two).
/// - `src_channels == 1`, `out_channels >= 2` → copy to channels 0 and 1; the
///   rest silent.
/// - `src_channels == 2`, `out_channels > 2` → channels 0 and 1; the rest
///   silent.
/// - otherwise → 1:1 for `min(src_channels, out_channels)`; extra source
///   channels are dropped, extra output channels are silent.
///
/// Used for both the peer stream (regulator output) and the local monitor mix
/// (capture), so both play back through the same mapping.
pub(crate) fn map_to_output(src: &[f32], src_channels: usize, out: &mut [f32], out_channels: usize) {
    let src_channels = src_channels.max(1);
    let out_channels = out_channels.max(1);
    let frames = out.len() / out_channels;

    if out_channels == 1 {
        downmix_to_mono(src, src_channels, &mut out[..frames]);
        return;
    }

    out[..frames * out_channels].fill(0.0);

    if src_channels == 1 {
        // Copy the single source channel to output channels 0 and 1.
        for dst_ch in 0..out_channels.min(2) {
            out[dst_ch * frames..dst_ch * frames + frames.min(src.len())]
                .copy_from_slice(&src[..frames.min(src.len())]);
        }
        return;
    }

    if src_channels == 2 && out_channels > 2 {
        for dst_ch in 0..2 {
            for frame in 0..frames {
                out[dst_ch * frames + frame] = src.get(frame * src_channels + dst_ch).copied().unwrap_or(0.0);
            }
        }
        return;
    }

    // 1:1 for min(src_channels, out_channels); extra source channels
    // dropped, extra output channels left silent (already zeroed above).
    let mapped_channels = src_channels.min(out_channels);
    for ch in 0..mapped_channels {
        for frame in 0..frames {
            out[ch * frames + frame] = src.get(frame * src_channels + ch).copied().unwrap_or(0.0);
        }
    }
}

/// Conform `src_channels`-wide interleaved audio to `out_channels`-wide
/// interleaved audio, one frame at a time (`frames == out.len() /
/// out_channels`).
///
/// Mirrors [`map_to_output`]'s documented channel-mapping policy — the same
/// four rules: downmix-to-1, mono-duplicated-to-first-two,
/// stereo-to-first-two-of-many, otherwise 1:1 with extra source channels
/// dropped and extra destination channels silent — but for an INTERLEAVED
/// source *and* destination.
///
/// Used to conform the browser's actually-granted capture width to the
/// network wire's fixed per-session channel count (decided once at connect
/// time from `AudioParams::capture_channels`), which `map_to_output`'s planar
/// output layout cannot serve directly. The two functions deliberately keep
/// separate implementations: they walk genuinely different memory layouts,
/// and the parallel structure is cheaper than an abstraction over both.
///
/// Reads defensively past the end of `src` as silence, same as
/// [`downmix_to_mono`] — this runs on the realtime audio thread.
pub(crate) fn conform_interleaved_channels(src: &[f32], src_channels: usize, out: &mut [f32], out_channels: usize) {
    let src_channels = src_channels.max(1);
    let out_channels = out_channels.max(1);
    let frames = out.len() / out_channels;

    if out_channels == 1 {
        downmix_to_mono(src, src_channels, &mut out[..frames]);
        return;
    }

    out[..frames * out_channels].fill(0.0);

    if src_channels == 1 {
        // Duplicate the single source channel into output channels 0 and 1.
        for frame in 0..frames {
            let sample = src.get(frame).copied().unwrap_or(0.0);
            for dst_ch in 0..out_channels.min(2) {
                out[frame * out_channels + dst_ch] = sample;
            }
        }
        return;
    }

    if src_channels == 2 && out_channels > 2 {
        for frame in 0..frames {
            for dst_ch in 0..2 {
                out[frame * out_channels + dst_ch] = src.get(frame * src_channels + dst_ch).copied().unwrap_or(0.0);
            }
        }
        return;
    }

    // 1:1 for min(src_channels, out_channels); extra source channels dropped,
    // extra output channels left silent (already zeroed above).
    let mapped_channels = src_channels.min(out_channels);
    for frame in 0..frames {
        for ch in 0..mapped_channels {
            out[frame * out_channels + ch] = src.get(frame * src_channels + ch).copied().unwrap_or(0.0);
        }
    }
}

/// Convert `channels`-wide planar audio (`planar[ch * frames + frame]`) to
/// interleaved (`out[frame * channels + ch]`). The inverse layout
/// transform of what `map_to_output` consumes as its `src` — used to turn
/// the worklet's planar capture into the interleaved shape both the
/// network wire format and `map_to_output` expect.
///
/// Reads defensively past the end of `planar` as silence, same as
/// `downmix_to_mono`.
pub(crate) fn interleave_planar(planar: &[f32], channels: usize, frames: usize, out: &mut [f32]) {
    for ch in 0..channels {
        for frame in 0..frames {
            out[frame * channels + ch] = planar.get(ch * frames + frame).copied().unwrap_or(0.0);
        }
    }
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
    /// Ring buffer for sending local audio to network (audio device → worklet → main thread → network).
    /// Null when no network is attached; reached via its `&self` API through [`SharedPtr::as_ref`].
    local_to_network_buffer: SharedPtr<RingBuffer>,
    /// Jitter buffer for receiving audio from network (network → main thread → jitter buffer → worklet → audio device).
    /// Null when no network is attached; still `&mut`-accessed via [`SharedPtr::as_mut`].
    network_to_local_buffer: SharedPtr<Regulator>,
    /// Temporary buffer for gained audio. Planar multi-channel, same layout
    /// as `process()`'s `input`: `in_channels * frames` samples, `gained[ch *
    /// frames + frame]`. Preallocated at `128 * MAX_CHANNELS` so a widening
    /// resize never allocates on the render thread.
    gained_buffer: Vec<f32>,
    /// The captured audio converted from the worklet's planar layout to
    /// interleaved (`[frame * channels + ch]`), shared by the network-send path
    /// and the local monitor mix so the planar→interleaved conversion happens
    /// once per callback, not twice. Preallocated at `128 * MAX_CHANNELS` so a
    /// widening resize never allocates on the render thread.
    captured_interleaved: Vec<f32>,
    /// Buffer for the regulator's interleaved output, before mapping to the
    /// output device's channel count. Preallocated at `128 * MAX_CHANNELS` so
    /// a widening `receive_from_network` resize never allocates on the render
    /// thread.
    remote_interleaved: Vec<f32>,
    /// The peer stream mapped onto the output device's channels (planar:
    /// `[ch * frames + frame]`). Preallocated at `128 * MAX_CHANNELS`.
    remote_mapped: Vec<f32>,
    /// The local monitor mapped onto the output device's channels, same
    /// planar layout as `remote_mapped`.
    monitor_mapped: Vec<f32>,
    /// Scratch buffer for `send_local_to_network`'s conform step (real
    /// captured width → the session's fixed wire channel count). Kept
    /// separate from `captured_interleaved` because the monitor mix still
    /// needs the TRUE captured width, not the wire-conformed one.
    /// Preallocated at `128 * MAX_CHANNELS`.
    wire_conformed: Vec<f32>,
}

impl AudioProcessor {
    pub fn new(params: &'static AudioParams) -> Self {
        Self {
            params,
            local_to_network_buffer: SharedPtr::null(),
            network_to_local_buffer: SharedPtr::null(),
            gained_buffer: vec![0.0; 128 * MAX_CHANNELS as usize],
            captured_interleaved: vec![0.0; 128 * MAX_CHANNELS as usize],
            remote_interleaved: vec![0.0; 128 * MAX_CHANNELS as usize],
            remote_mapped: vec![0.0; 128 * MAX_CHANNELS as usize],
            monitor_mapped: vec![0.0; 128 * MAX_CHANNELS as usize],
            wire_conformed: vec![0.0; 128 * MAX_CHANNELS as usize],
        }
    }

    /// Create processor with network audio support
    /// - local_to_network_buffer: ring buffer for sending local audio to network
    /// - network_to_local_buffer: jitter buffer for receiving audio from network
    pub fn with_network(
        params: &'static AudioParams,
        local_to_network_buffer: SharedPtr<RingBuffer>,
        network_to_local_buffer: SharedPtr<Regulator>,
    ) -> Self {
        Self {
            params,
            local_to_network_buffer,
            network_to_local_buffer,
            gained_buffer: vec![0.0; 128 * MAX_CHANNELS as usize],
            captured_interleaved: vec![0.0; 128 * MAX_CHANNELS as usize],
            remote_interleaved: vec![0.0; 128 * MAX_CHANNELS as usize],
            remote_mapped: vec![0.0; 128 * MAX_CHANNELS as usize],
            monitor_mapped: vec![0.0; 128 * MAX_CHANNELS as usize],
            wire_conformed: vec![0.0; 128 * MAX_CHANNELS as usize],
        }
    }

    /// Process audio: calculate volume levels, handle network audio, and generate output.
    ///
    /// `input` is planar across `in_channels` input channels: `input[ch *
    /// frames + frame]`, `frames == input.len() / in_channels`. `output` is
    /// planar across `out_channels` output channels: `output[ch * frames +
    /// frame]`.
    pub fn process(&mut self, input: &[f32], in_channels: usize, output: &mut [f32], out_channels: usize) -> bool {
        let in_channels = in_channels.max(1);
        let out_channels = out_channels.max(1);
        let frames = input.len() / in_channels;

        // Increment callback counter for stats tracking
        self.params.callback_count.fetch_add(1, Ordering::Relaxed);

        // Get input gain (dB) and convert to linear
        let input_gain_db = self.params.input_gain_db.load(Ordering::Relaxed) as f32 / 100.0;
        let input_gain_linear = db_to_linear(input_gain_db);

        // Ensure buffers are correct size
        let gained_len = in_channels * frames;
        if self.gained_buffer.len() < gained_len {
            self.gained_buffer.resize(gained_len, 0.0);
        }
        let mapped_len = frames * out_channels;
        if self.remote_mapped.len() < mapped_len {
            self.remote_mapped.resize(mapped_len, 0.0);
        }
        if self.monitor_mapped.len() < mapped_len {
            self.monitor_mapped.resize(mapped_len, 0.0);
        }

        // Apply input gain to local audio (clamped to [-1.0, 1.0])
        apply_gain(input, input_gain_linear, &mut self.gained_buffer);

        // Calculate RMS for volume metering: the loudest of the captured
        // channels, not their average — a single hot channel must still show.
        let rms = max_channel_rms(&self.gained_buffer, in_channels, frames);
        let current_db = amplitude_to_db(rms);

        // Store dB level
        self.params.db_level.store(encode_db(current_db), Ordering::Relaxed);

        // Peak level tracking with hold and decay
        self.update_peak_level(current_db);

        // Convert the planar capture to interleaved once, shared by the
        // network-send path and the local monitor mix below.
        let captured_len = frames * in_channels;
        if self.captured_interleaved.len() < captured_len {
            self.captured_interleaved.resize(captured_len, 0.0);
        }
        interleave_planar(&self.gained_buffer, in_channels, frames, &mut self.captured_interleaved[..captured_len]);

        // Send local audio to network (if enabled)
        self.send_local_to_network(in_channels, frames);

        // Receive remote audio from network into `remote_interleaved` and map
        // it onto `out_channels` output channels. `receive_from_network`
        // returns 0 when no network buffer is attached, in which case the
        // peer stream is silence.
        let remote_channels = self.receive_from_network(frames);
        if remote_channels == 0 {
            self.remote_mapped[..mapped_len].fill(0.0);
        } else {
            map_to_output(
                &self.remote_interleaved[..frames * remote_channels],
                remote_channels,
                &mut self.remote_mapped[..mapped_len],
                out_channels,
            );
        }

        // Map the local monitor (captured, interleaved) onto the same output width.
        map_to_output(
            &self.captured_interleaved[..captured_len],
            in_channels,
            &mut self.monitor_mapped[..mapped_len],
            out_channels,
        );

        // Generate output: mix monitor + remote audio, per output channel.
        let monitor_volume = decode_volume(self.params.monitor_volume.load(Ordering::Relaxed));
        let output_volume  = decode_volume(self.params.output_volume.load(Ordering::Relaxed));

        let out_len = output.len().min(mapped_len);
        for i in 0..out_len {
            // Start with remote audio (zeros if no network connected)
            let mut out_sample = self.remote_mapped[i];

            // Add local monitor audio (if enabled)
            if monitor_volume > 0.0 {
                out_sample += self.monitor_mapped[i] * monitor_volume;
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

        self.params.peak_hold_counter.store(new_hold_counter, Ordering::Relaxed);
        // During the hold phase `compute_peak_update` returns the same f32 value it
        // received, so `new_peak_db == current_peak_db` exactly (no arithmetic was
        // done on it). Skipping the write avoids a decode→encode round-trip that
        // can drift ±1 fixed-point count per frame due to f32 rounding.
        if new_peak_db != current_peak_db {
            self.params.peak_db_level.store(encode_db(new_peak_db), Ordering::Relaxed);
        }
    }

    /// Send local audio to network via ring buffer, as interleaved audio at
    /// the session's *wire* channel count.
    ///
    /// `in_channels` is what the worklet actually captured this callback — the
    /// browser's granted width, which can be narrower than what was requested
    /// (a mono-only device, or echo cancellation forcing mono). The transports
    /// downstream of this ring buffer frame every outbound packet at a fixed
    /// width chosen once at connect time (`AudioParams::capture_channels`), so
    /// a mismatch here would mis-frame the packets: two mono quantums packed
    /// and labelled as one stereo quantum. Conform the captured width to the
    /// wire width before writing.
    fn send_local_to_network(&mut self, in_channels: usize, frames: usize) {
        // Sound shared borrow: `RingBuffer`'s write path is `&self` (interior
        // mutability), so producer and consumer may hold `&RingBuffer` at once.
        let Some(buffer) = self.local_to_network_buffer.as_ref() else {
            return;
        };

        if !buffer.is_streaming() {
            return;
        }

        let wire_channels = (self.params.get_capture_channels() as usize).max(1);
        if in_channels == wire_channels {
            buffer.write(&self.captured_interleaved[..frames * in_channels]);
            return;
        }

        let wire_len = frames * wire_channels;
        if self.wire_conformed.len() < wire_len {
            self.wire_conformed.resize(wire_len, 0.0);
        }
        conform_interleaved_channels(
            &self.captured_interleaved[..frames * in_channels],
            in_channels,
            &mut self.wire_conformed[..wire_len],
            wire_channels,
        );
        buffer.write(&self.wire_conformed[..wire_len]);
    }

    /// Receive remote audio from network via jitter buffer into
    /// `remote_interleaved`. Returns the regulator's channel count, or `0`
    /// when no network buffer is attached (the caller treats that as
    /// silence).
    ///
    /// `Regulator::pop()`'s return value is informational metadata (real vs.
    /// concealed) and must NOT gate playback — concealed audio is the entire
    /// point of jitter buffering and must be played to avoid clicks.
    fn receive_from_network(&mut self, frames: usize) -> usize {
        // SAFETY: `Regulator::pop` is still `&mut self`, so we take a `&mut`
        // through `SharedPtr::as_mut`. The push side runs on the network thread;
        // this remains the not-yet-sound path documented on `SharedPtr::as_mut`.
        let Some(regulator) = (unsafe { self.network_to_local_buffer.as_mut() }) else {
            return 0;
        };

        // The pop width is the regulator's own — it is the peer's channel
        // count (adopted from their first packet) and fpp, not a local
        // playback-device setting. `AudioParams::capture_channels` governs
        // only the send path.
        let channels = regulator.channels();
        let fpp = regulator.fpp();
        debug_assert_eq!(fpp, frames, "regulator fpp must match the worklet's frame count");
        let interleaved_len = fpp * channels;

        if self.remote_interleaved.len() < interleaved_len {
            self.remote_interleaved.resize(interleaved_len, 0.0);
        }

        // Read the regulator's output (always populates the buffer; pop()'s
        // bool distinguishes real vs concealed but is irrelevant for mixing).
        regulator.pop(&mut self.remote_interleaved[..interleaved_len]);

        channels
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
    #[should_panic(expected = "apply_gain: output buffer")]
    fn test_apply_gain_panics_when_output_shorter_than_input() {
        let input = [0.5f32; 4];
        let mut output = [0.0f32; 3]; // shorter than input — must panic
        apply_gain(&input, 1.0, &mut output);
    }

    // --- downmix_to_mono ----------------------------------------------------

    #[test]
    fn test_downmix_to_mono_averages_each_channel_count() {
        // For each channel count 1..=8, build 2 frames of per-channel-distinct
        // interleaved audio (channel `ch` always carries value `ch + 1`), and
        // assert the downmix is the exact average.
        for channels in 1..=8usize {
            let frame: Vec<f32> = (0..channels).map(|ch| (ch + 1) as f32).collect();
            let mut interleaved = frame.clone();
            interleaved.extend_from_slice(&frame); // 2 identical frames

            let mut mono_out = vec![-1.0f32; 2];
            downmix_to_mono(&interleaved, channels, &mut mono_out);

            let expected: f32 = frame.iter().sum::<f32>() / channels as f32;
            for (i, &sample) in mono_out.iter().enumerate() {
                assert!(
                    (sample - expected).abs() < EPS,
                    "channels={channels} frame={i}: expected {expected}, got {sample}"
                );
            }
        }
    }

    #[test]
    fn test_downmix_to_mono_mono_is_identity() {
        let interleaved = [0.25f32, -0.5, 1.0];
        let mut mono_out = vec![0.0f32; 3];
        downmix_to_mono(&interleaved, 1, &mut mono_out);
        assert_eq!(mono_out, interleaved);
    }

    #[test]
    fn test_downmix_to_mono_short_input_treated_as_silence() {
        // Fewer samples than `mono_out.len() * channels` — the missing
        // channels of the last frame must read as zero, not panic or read
        // out of bounds.
        let interleaved = [1.0f32, 1.0]; // one full stereo frame, second frame missing
        let mut mono_out = vec![-1.0f32; 2];
        downmix_to_mono(&interleaved, 2, &mut mono_out);
        assert!((mono_out[0] - 1.0).abs() < EPS, "full frame must average normally");
        assert!((mono_out[1] - 0.0).abs() < EPS, "missing frame must read as silence");
    }

    // --- channel-mapping policy (map_to_output / conform_interleaved_channels) ---

    /// The channel-mapping policy that `map_to_output` and
    /// `conform_interleaved_channels` both implement, stated once for both
    /// matrix tests (they differ only in destination memory layout).
    ///
    /// For `out_channels >= 2`, returns which SOURCE channel each mapped
    /// destination channel must carry, in destination order; destination
    /// channels at or beyond the returned length must be silent. The
    /// `out_channels == 1` averaging branch is asserted separately, since it
    /// is not a per-channel copy.
    fn expected_channel_sources(src_channels: usize, out_channels: usize) -> Vec<usize> {
        if src_channels == 1 {
            // Mono duplicated into destination channels 0 and 1.
            vec![0; out_channels.min(2)]
        } else if src_channels == 2 && out_channels > 2 {
            vec![0, 1]
        } else {
            (0..src_channels.min(out_channels)).collect()
        }
    }

    /// Interleaved source for the mapping matrices: frame `f` channel `ch`
    /// carries `f * 10 + ch + 1`, never repeated across frames or channels, so
    /// a bug that reads the wrong frame (e.g. always frame 0) or the wrong
    /// channel fails instead of coincidentally matching.
    fn mapping_value(frame: usize, ch: usize) -> f32 {
        (frame * 10 + ch + 1) as f32
    }

    fn mapping_source(frames: usize, src_channels: usize) -> Vec<f32> {
        let mut src = Vec::with_capacity(frames * src_channels);
        for frame in 0..frames {
            for ch in 0..src_channels {
                src.push(mapping_value(frame, ch));
            }
        }
        src
    }

    /// The `out_channels == 1` branch's expected value: the average of every
    /// source channel in that frame.
    fn mapping_mono_average(frame: usize, src_channels: usize) -> f32 {
        (0..src_channels).map(|ch| mapping_value(frame, ch)).sum::<f32>() / src_channels as f32
    }

    // --- map_to_output ------------------------------------------------------

    /// Full `src_channels × out_channels` matrix over `1..=8`, with
    /// per-frame-*and*-per-channel-distinguishable input (frame `f` channel
    /// `ch` carries value `f * 10 + ch + 1`, never repeated across frames) so
    /// a mapping bug that reads the wrong frame — e.g. always frame 0 — fails
    /// the assertions instead of accidentally matching. Asserts every rule
    /// branch: the `out_channels == 1` average (delegated to
    /// `downmix_to_mono`), the `src_channels == 1` copy-to-0/1, the
    /// `src_channels == 2, out_channels > 2` copy-to-0/1, the general
    /// 1:1-with-drop case, and silence on every unmapped output channel.
    #[test]
    fn test_map_to_output_matrix_covers_every_branch() {
        let frames = 3usize;

        for src_channels in 1..=8usize {
            let src = mapping_source(frames, src_channels);

            for out_channels in 1..=8usize {
                let mut out = vec![-99.0f32; frames * out_channels];
                map_to_output(&src, src_channels, &mut out, out_channels);

                if out_channels == 1 {
                    for frame in 0..frames {
                        let expected = mapping_mono_average(frame, src_channels);
                        let s = out[frame];
                        assert!(
                            (s - expected).abs() < EPS,
                            "src={src_channels} out={out_channels} frame={frame}: average branch expected {expected}, got {s}"
                        );
                    }
                    continue;
                }

                let sources = expected_channel_sources(src_channels, out_channels);

                for ch in 0..out_channels {
                    let plane = &out[ch * frames..ch * frames + frames];
                    if let Some(&src_ch) = sources.get(ch) {
                        for (frame, &s) in plane.iter().enumerate() {
                            let expected = mapping_value(frame, src_ch);
                            assert!(
                                (s - expected).abs() < EPS,
                                "src={src_channels} out={out_channels} ch={ch} frame={frame}: expected {expected}, got {s}"
                            );
                        }
                    } else {
                        for &s in plane {
                            assert_eq!(
                                s, 0.0,
                                "src={src_channels} out={out_channels} ch={ch}: unmapped channel must be silent"
                            );
                        }
                    }
                }
            }
        }
    }

    // --- conform_interleaved_channels ---------------------------------------

    /// Same full `src_channels × out_channels` matrix over `1..=8` as
    /// `test_map_to_output_matrix_covers_every_branch`, but for the
    /// interleaved→interleaved conform used on the network send path (the
    /// browser's granted capture width → the session's fixed wire width).
    /// Asserts every rule branch — the `out_channels == 1` average, the
    /// mono-to-0/1 duplication, the stereo-to-0/1 of a wider wire, the
    /// general 1:1 case with extra source channels DROPPED — plus silence on
    /// every unmapped destination channel. A destination-indexing bug (planar
    /// instead of interleaved, or a frame/channel transposition) fails here
    /// rather than shipping garbled audio to the peer.
    #[test]
    fn test_conform_interleaved_channels_matrix_covers_every_branch() {
        let frames = 3usize;

        for src_channels in 1..=8usize {
            let src = mapping_source(frames, src_channels);

            for out_channels in 1..=8usize {
                let mut out = vec![-99.0f32; frames * out_channels];
                conform_interleaved_channels(&src, src_channels, &mut out, out_channels);

                if out_channels == 1 {
                    for frame in 0..frames {
                        let expected = mapping_mono_average(frame, src_channels);
                        let s = out[frame];
                        assert!(
                            (s - expected).abs() < EPS,
                            "src={src_channels} out={out_channels} frame={frame}: average branch expected {expected}, got {s}"
                        );
                    }
                    continue;
                }

                let sources = expected_channel_sources(src_channels, out_channels);

                for frame in 0..frames {
                    for ch in 0..out_channels {
                        let s = out[frame * out_channels + ch];
                        match sources.get(ch) {
                            Some(&src_ch) => {
                                let expected = mapping_value(frame, src_ch);
                                assert!(
                                    (s - expected).abs() < EPS,
                                    "src={src_channels} out={out_channels} frame={frame} ch={ch}: expected {expected}, got {s}"
                                );
                            }
                            None => assert_eq!(
                                s, 0.0,
                                "src={src_channels} out={out_channels} frame={frame} ch={ch}: unmapped channel must be silent"
                            ),
                        }
                    }
                }
            }
        }
    }

    // --- interleave_planar --------------------------------------------------

    /// Full `channels` matrix over `1..=8`, with per-frame-*and*-per-channel
    /// distinguishable input (channel `ch` frame `f` carries value `ch * 100
    /// + f + 1`, never repeated) so a swapped planar/interleaved index bug —
    /// exactly the kind of transposition mistake that would silently produce
    /// plausible-sounding but wrong audio — fails the assertions instead of
    /// coincidentally passing.
    #[test]
    fn test_interleave_planar_matrix_distinguishes_frame_and_channel_order() {
        let frames = 3usize;
        let value = |ch: usize, frame: usize| (ch * 100 + frame + 1) as f32;

        for channels in 1..=8usize {
            let mut planar = vec![0.0f32; channels * frames];
            for ch in 0..channels {
                for frame in 0..frames {
                    planar[ch * frames + frame] = value(ch, frame);
                }
            }

            let mut out = vec![-99.0f32; frames * channels];
            interleave_planar(&planar, channels, frames, &mut out);

            for frame in 0..frames {
                for ch in 0..channels {
                    let expected = value(ch, frame);
                    let actual = out[frame * channels + ch];
                    assert!(
                        (actual - expected).abs() < EPS,
                        "channels={channels} frame={frame} ch={ch}: expected {expected}, got {actual}"
                    );
                }
            }
        }
    }

    // --- compute_rms ------------------------------------------------------------

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

    // --- max_channel_rms ----------------------------------------------------

    /// For `channels` in `1..=8`, construct per-channel DC levels that are
    /// NOT all equal (channel `ch` holds constant value `(ch + 1) * 0.1`) so
    /// a regression that accidentally averages instead of maxing fails this
    /// assertion instead of coincidentally passing on uniform input. The
    /// loudest channel is always the last one (`channels`), so its RMS
    /// (== its DC level) must be the result.
    #[test]
    fn test_max_channel_rms_returns_loudest_channel_not_average() {
        let frames = 8usize;
        for channels in 1..=8usize {
            let mut planar = vec![0.0f32; channels * frames];
            for ch in 0..channels {
                let level = (ch + 1) as f32 * 0.1;
                planar[ch * frames..(ch + 1) * frames].fill(level);
            }

            let result = max_channel_rms(&planar, channels, frames);
            let expected_loudest = channels as f32 * 0.1;
            assert!(
                (result - expected_loudest).abs() < EPS,
                "channels={channels}: expected max (loudest channel) {expected_loudest}, got {result}"
            );

            let average: f32 = (1..=channels).map(|ch| ch as f32 * 0.1).sum::<f32>() / channels as f32;
            if channels > 1 {
                assert!(
                    (result - average).abs() > EPS,
                    "channels={channels}: result must not equal the average across channels"
                );
            }
        }
    }

    /// At `channels = 1`, `max_channel_rms` must be exactly `compute_rms` of
    /// the single channel — pins the pass-through identity using the same
    /// full-scale sine signal as `test_rms_full_scale_sine`.
    #[test]
    fn test_max_channel_rms_single_channel_is_compute_rms() {
        let n = 1024usize;
        let samples: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * i as f32 / n as f32).sin())
            .collect();

        let expected = compute_rms(&samples);
        let actual = max_channel_rms(&samples, 1, n);
        assert!(
            (actual - expected).abs() < EPS,
            "single-channel max_channel_rms must equal compute_rms: expected {expected}, got {actual}"
        );
    }

    // --- compute_peak_update ----------------------------------------------------

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

    // --- update_peak_level integration: hold-phase must not drift -----------

    /// Verify that the stored `peak_db_level` fixed-point value does not change
    /// during the hold window.  The regression being guarded: re-encoding a
    /// decoded f32 via `encode_db(decode_db(stored))` can drift by ±1 count
    /// per frame due to f32 rounding (e.g. 4321/100*100 → 4320.999 → 4320).
    #[test]
    fn test_peak_db_level_stable_during_hold() {
        use crate::audio::params::{AudioParams, encode_db};

        let params: &'static AudioParams = Box::leak(Box::new(AudioParams::default()));
        let mut processor = AudioProcessor::new(params);

        // Drive a loud signal through process() to establish a peak.
        let loud: Vec<f32> = vec![1.0; 128];
        let mut out = vec![0.0f32; 128];
        processor.process(&loud, 1, &mut out, 1);

        let stored_after_peak = params.peak_db_level.load(std::sync::atomic::Ordering::Relaxed);
        assert!(stored_after_peak > encode_db(-60.0), "a loud signal must raise the peak");

        // Now feed silence for exactly PEAK_HOLD_FRAMES callbacks.
        // The stored fixed-point value must remain bit-for-bit identical throughout.
        let silence: Vec<f32> = vec![0.0; 128];
        for frame in 0..PEAK_HOLD_FRAMES {
            processor.process(&silence, 1, &mut out, 1);
            let stored = params.peak_db_level.load(std::sync::atomic::Ordering::Relaxed);
            assert_eq!(
                stored,
                stored_after_peak,
                "peak_db_level drifted on hold frame {frame}: expected {stored_after_peak}, got {stored}"
            );
        }

        // After hold expires, decay must have begun (stored value decreases).
        processor.process(&silence, 1, &mut out, 1);
        let stored_after_decay = params.peak_db_level.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            stored_after_decay < stored_after_peak,
            "peak must start decaying after the hold window expires"
        );
    }

    // --- process() wiring, end to end --------------------------------------

    /// A processor wired to `ring` (streaming) with no regulator attached, so
    /// the peer stream is silence and the output carries only the local
    /// monitor. `ring` must already live where it will stay: the processor
    /// keeps a raw pointer to it.
    fn networked_processor(params: &'static AudioParams, ring: &mut RingBuffer) -> AudioProcessor {
        ring.set_streaming(true);
        AudioProcessor::with_network(params, SharedPtr::new(ring as *mut RingBuffer), SharedPtr::null())
    }

    /// `frames` of planar capture (`[ch * frames + frame]`) where channel `ch`
    /// holds the constant `levels[ch]` — distinct per channel, so a dropped,
    /// swapped or duplicated channel is detectable downstream.
    fn planar_capture(levels: &[f32], frames: usize) -> Vec<f32> {
        let mut input = vec![0.0f32; levels.len() * frames];
        for (ch, &level) in levels.iter().enumerate() {
            input[ch * frames..(ch + 1) * frames].fill(level);
        }
        input
    }

    /// End-to-end wiring of `process()` at a real multichannel capture width:
    /// gain → RMS → interleave-once → (ring write AND monitor mix). The pure
    /// helpers each have their own matrix test; what this pins is that
    /// `process` hands each of them the right buffer in the right layout —
    /// e.g. feeding the still-planar `gained_buffer` to `map_to_output`, or
    /// writing the wrong buffer to the ring, would pass every other test.
    #[test]
    fn test_process_stereo_capture_feeds_ring_and_monitor() {
        const FRAMES: usize = 128;
        const LEFT: f32 = 0.25;
        const RIGHT: f32 = -0.5;
        const MONITOR: f32 = 0.5;

        let params: &'static AudioParams = Box::leak(Box::new(AudioParams::default()));
        params.set_capture_channels(2); // wire width matches the capture width here
        params.set_monitor_volume(MONITOR);

        let mut ring = RingBuffer::new();
        let mut processor = networked_processor(params, &mut ring);

        let input = planar_capture(&[LEFT, RIGHT], FRAMES);
        let mut output = vec![-99.0f32; FRAMES * 2];
        assert!(processor.process(&input, 2, &mut output, 2));

        // The network path must see interleaved stereo: L,R per frame — not
        // planar, not one channel, not an average.
        assert_eq!(
            ring.available(),
            (FRAMES * 2) as u32,
            "a stereo quantum must contribute frames * 2 samples to the wire"
        );
        let mut wire = vec![-99.0f32; FRAMES * 2];
        assert!(ring.read(&mut wire));
        for frame in 0..FRAMES {
            assert!(
                (wire[frame * 2] - LEFT).abs() < EPS,
                "wire frame {frame} channel 0: expected {LEFT}, got {}",
                wire[frame * 2]
            );
            assert!(
                (wire[frame * 2 + 1] - RIGHT).abs() < EPS,
                "wire frame {frame} channel 1: expected {RIGHT}, got {}",
                wire[frame * 2 + 1]
            );
        }

        // The monitor mix must reach the output planes 1:1 at monitor volume,
        // with both source channels still distinguishable (no downmix, no
        // channel-0-only copy). The peer stream is silence (no regulator).
        for frame in 0..FRAMES {
            let left = output[frame];
            let right = output[FRAMES + frame];
            assert!(
                (left - LEFT * MONITOR).abs() < EPS,
                "output plane 0 frame {frame}: expected {}, got {left}",
                LEFT * MONITOR
            );
            assert!(
                (right - RIGHT * MONITOR).abs() < EPS,
                "output plane 1 frame {frame}: expected {}, got {right}",
                RIGHT * MONITOR
            );
        }
    }

    /// Regression test: the transports frame every outbound packet at the
    /// session's fixed wire width (`AudioParams::capture_channels`, chosen at
    /// connect time), so when the browser grants a NARROWER capture than was
    /// requested — a mono-only device against the default stereo session —
    /// `process` must conform before writing. Writing `frames * in_channels`
    /// instead would pack two mono quantums into one "stereo" packet, which
    /// the peer decodes as double-speed garble.
    #[test]
    fn test_process_conforms_mono_capture_to_stereo_wire_width() {
        const FRAMES: usize = 128;
        const LEVEL: f32 = 0.4;

        let params: &'static AudioParams = Box::leak(Box::new(AudioParams::default()));
        params.set_capture_channels(2); // the wire is stereo …

        let mut ring = RingBuffer::new();
        let mut processor = networked_processor(params, &mut ring);

        // … but the browser granted mono.
        let input = planar_capture(&[LEVEL], FRAMES);
        let mut output = vec![0.0f32; FRAMES * 2];
        assert!(processor.process(&input, 1, &mut output, 2));

        assert_eq!(
            ring.available(),
            (FRAMES * 2) as u32,
            "a mono quantum must still contribute frames * wire_channels samples to the wire"
        );
        let mut wire = vec![-99.0f32; FRAMES * 2];
        assert!(ring.read(&mut wire));
        for (i, &sample) in wire.iter().enumerate() {
            assert!(
                (sample - LEVEL).abs() < EPS,
                "wire sample {i}: mono capture must be duplicated into both wire channels, got {sample}"
            );
        }
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
