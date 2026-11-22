//! Pure DSP math — free functions over plain slices and scalars, with no
//! knowledge of parameters, rings or regulators. The mixer composes these;
//! nothing here may reach `params.rs` or any wasm-bound type.

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
/// Used to conform the capture width to the network wire's send channel
/// count (fixed for the connection, `AudioParams::send_channels`) — e.g.
/// averaging two captured channels into one sent — which `map_to_output`'s
/// planar output layout cannot serve directly. The two functions deliberately keep
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Tolerance for floating-point comparisons.
    const EPS: f32 = 1e-4;

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
}
