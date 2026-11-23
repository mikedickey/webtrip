//! The device↔network bridge, expressed as N device streams and M network
//! streams. The only implementation, [`SimpleMixer`], handles one of each.
//!
//! The engine does no I/O: no `Regulator`, `RingBuffer`, `SharedPtr` or
//! `AudioParams` appear here. `AudioProcessor` does the pointer work, decodes
//! atomics into [`MixControls`] and encodes [`MixMeters`] back, which keeps the
//! engine portable and testable with plain slices.
//!
//! An absent network is an empty slice: `net_in: &[]` is silence and
//! `net_out: &mut []` means nothing is sent.

use crate::audio::dsp::{
    apply_gain, conform_interleaved_channels, interleave_planar, map_to_output, max_channel_rms, FixedSide,
    StreamResampler,
};
use crate::limits::{MAX_DEVICE_SAMPLE_RATE, MAX_WIRE_CHANNELS, MIN_DEVICE_SAMPLE_RATE};

/// Why a layout was refused by [`MixEngine::prepare`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixError {
    /// A dimension of the layout was zero, which could never hold a stream.
    EmptyLayout(&'static str),
    /// The device and wire rates cannot be converted between: a rate is zero
    /// or the device rate is outside the supported range. Names both so the
    /// failure is diagnosable, never a stream silently played at the wrong speed.
    UnsupportedRates { device_rate: u32, wire_rate: u32 },
    /// The regulator cannot serve the callbacks it is attached to: it would
    /// return 0 from every pop (a silent peer, nothing logged).
    RegulatorCapacity { capacity: usize, needed: usize },
    /// The regulator runs at a different rate than the wire stream; it never
    /// sees the device rate, so every conversion would be wrong.
    RegulatorRate { regulator_rate: u32, wire_rate: u32 },
}

impl std::fmt::Display for MixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MixError::EmptyLayout(what) => write!(f, "mix layout has zero {what}"),
            MixError::UnsupportedRates { device_rate, wire_rate } => write!(
                f,
                "cannot convert between the audio device rate ({device_rate} Hz) and the wire rate ({wire_rate} Hz): \
                 device rates must be {MIN_DEVICE_SAMPLE_RATE}..={MAX_DEVICE_SAMPLE_RATE} Hz"
            ),
            MixError::RegulatorCapacity { capacity, needed } => write!(
                f,
                "regulator callback capacity ({capacity} frames) is below the {needed} wire frames one callback can need"
            ),
            MixError::RegulatorRate { regulator_rate, wire_rate } => write!(
                f,
                "the regulator runs at {regulator_rate} Hz but the wire rate is {wire_rate} Hz"
            ),
        }
    }
}

impl std::error::Error for MixError {}

/// The largest buffers the engine will be asked to mix; sizes every scratch
/// buffer in [`MixEngine::prepare`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MixLayout {
    /// Most frames any one `render` call will carry.
    pub max_frames: usize,
    /// Widest device stream (capture or playback), in channels.
    pub max_device_channels: usize,
    /// The device's actual sample rate (`AudioContext.sampleRate`), which may
    /// differ from the rate it was asked for.
    pub device_rate: u32,
    /// The rate network streams run at. The mixer converts between the two
    /// when they differ, and does nothing when they match.
    pub wire_rate: u32,
}

/// Per-callback control values, already decoded from the shared atomics.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixControls {
    /// Linear gain applied to the capture.
    pub input_gain: f32,
    /// Linear level of the local monitor in the output.
    pub monitor_volume: f32,
    /// Linear level of the whole output.
    pub output_volume: f32,
}

/// What a `render` call measured, for the caller to publish.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MixMeters {
    /// The loudest captured channel's RMS after input gain (linear).
    pub input_rms: f32,
}

/// A captured device stream, planar: `planar[ch * frames + frame]`.
pub struct DeviceIn<'a> {
    pub planar: &'a [f32],
    pub channels: usize,
}

/// A playback device stream, planar like [`DeviceIn`].
pub struct DeviceOut<'a> {
    pub planar: &'a mut [f32],
    pub channels: usize,
}

/// A received network stream at the **wire** rate, interleaved:
/// `interleaved[frame * channels + ch]`, exactly
/// [`MixEngine::net_in_frames_needed`] frames. `channels == 0` means no audio
/// has arrived yet and counts as silence.
pub struct NetIn<'a> {
    pub interleaved: &'a [f32],
    pub channels: usize,
}

/// A network stream to send, interleaved at the **wire** rate. The caller
/// sizes `interleaved` for the most wire frames one callback can yield
/// ([`wire_frames_per_callback`]); `render` fills a prefix and reports its
/// length in `frames`. With a rate conversion the count varies per call.
pub struct NetOut<'a> {
    pub interleaved: &'a mut [f32],
    pub channels: usize,
    /// Set by `render`: wire frames written to `interleaved`.
    pub frames: usize,
}

/// Every stream one `render` call reads or writes.
pub struct MixIo<'a, 'b> {
    pub device_in: &'a [DeviceIn<'b>],
    pub device_out: &'a mut [DeviceOut<'b>],
    pub net_in: &'a [NetIn<'b>],
    pub net_out: &'a mut [NetOut<'b>],
}

pub trait MixEngine: Send {
    /// Off the RT thread. Sizes every scratch buffer. The only place it allocates.
    fn prepare(&mut self, layout: &MixLayout) -> Result<(), MixError>;
    /// On the RT thread. No allocation, no locking, no unbounded work.
    fn render(&mut self, controls: &MixControls, io: &mut MixIo<'_, '_>) -> MixMeters;
    /// Wire-rate frames the next `render` of `device_frames` device frames
    /// reads from each `net_in`. Equal to `device_frames` unless the engine
    /// converts between the device and wire rates, in which case it varies per
    /// call and the caller must supply exactly this many.
    fn net_in_frames_needed(&self, device_frames: usize) -> usize {
        device_frames
    }
}

/// One device in, one device out, at most one network stream each way.
#[derive(Default)]
pub struct SimpleMixer {
    /// Gained capture, planar like `device_in[0]`.
    gained: Vec<f32>,
    /// The gained capture converted to interleaved once, shared by the network
    /// send and the monitor mix.
    captured_interleaved: Vec<f32>,
    /// The peer stream mapped onto the output device's channels (planar).
    remote_mapped: Vec<f32>,
    /// The local monitor mapped onto the output device's channels (planar).
    monitor_mapped: Vec<f32>,
    /// Capture → wire rate. `None` when the rates match (no conversion runs).
    send_resampler: Option<StreamResampler>,
    /// The capture at wire width but still at the device rate, the resampler's input.
    send_native: Vec<f32>,
    /// Wire rate → device rate for the peer stream. `None` when the rates match.
    recv_resampler: Option<StreamResampler>,
    /// The peer stream converted to the device rate, at wire width.
    recv_native: Vec<f32>,
}

impl SimpleMixer {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    fn scratch_lens(&self) -> [usize; 4] {
        [
            self.gained.len(),
            self.captured_interleaved.len(),
            self.remote_mapped.len(),
            self.monitor_mapped.len(),
        ]
    }
}

/// Grow `buf` to `len` if a caller exceeded the prepared layout. Never taken
/// when `prepare` saw the real maxima; it exists so an oversized callback
/// degrades to one allocation rather than a panic on the render thread.
pub(crate) fn ensure_len(buf: &mut Vec<f32>, len: usize) {
    if buf.len() < len {
        buf.resize(len, 0.0);
    }
}

impl MixEngine for SimpleMixer {
    fn net_in_frames_needed(&self, device_frames: usize) -> usize {
        match &self.recv_resampler {
            Some(resampler) if device_frames == resampler.output_frames_next() => resampler.input_frames_next(),
            _ => device_frames,
        }
    }

    fn prepare(&mut self, layout: &MixLayout) -> Result<(), MixError> {
        if layout.max_frames == 0 {
            return Err(MixError::EmptyLayout("frames"));
        }
        if layout.max_device_channels == 0 {
            return Err(MixError::EmptyLayout("device channels"));
        }
        let rates = MixError::UnsupportedRates { device_rate: layout.device_rate, wire_rate: layout.wire_rate };
        if layout.wire_rate == 0 || !(MIN_DEVICE_SAMPLE_RATE..=MAX_DEVICE_SAMPLE_RATE).contains(&layout.device_rate) {
            return Err(rates);
        }
        let wire_channels = MAX_WIRE_CHANNELS as usize;
        (self.send_resampler, self.recv_resampler) = if layout.device_rate == layout.wire_rate {
            (None, None)
        } else {
            let send = StreamResampler::new(layout.device_rate, layout.wire_rate, layout.max_frames, wire_channels, FixedSide::Input);
            let recv = StreamResampler::new(layout.wire_rate, layout.device_rate, layout.max_frames, wire_channels, FixedSide::Output);
            match (send, recv) {
                (Ok(send), Ok(recv)) => {
                    log::info!(
                        "Resampling active: audio device runs at {} Hz, wire at {} Hz; converting both directions",
                        layout.device_rate,
                        layout.wire_rate
                    );
                    (Some(send), Some(recv))
                }
                _ => return Err(rates),
            }
        };
        let device_frames = layout.max_frames;
        self.send_native = vec![0.0; device_frames * wire_channels];
        self.recv_native = vec![0.0; device_frames * wire_channels];
        let len = layout.max_frames * layout.max_device_channels;
        for buf in [
            &mut self.gained,
            &mut self.captured_interleaved,
            &mut self.remote_mapped,
            &mut self.monitor_mapped,
        ] {
            buf.clear();
            buf.resize(len, 0.0);
        }
        Ok(())
    }

    /// Gain → peak RMS → interleave once → conform to wire width → map remote
    /// and monitor onto the output width → sum, scale, clamp.
    fn render(&mut self, controls: &MixControls, io: &mut MixIo<'_, '_>) -> MixMeters {
        let (Some(input), Some(output)) = (io.device_in.first(), io.device_out.first_mut()) else {
            return MixMeters::default();
        };
        let in_channels = input.channels.max(1);
        let out_channels = output.channels.max(1);
        let frames = input.planar.len() / in_channels;
        let captured_len = frames * in_channels;
        let mapped_len = frames * out_channels;

        ensure_len(&mut self.gained, captured_len);
        ensure_len(&mut self.captured_interleaved, captured_len);
        ensure_len(&mut self.remote_mapped, mapped_len);
        ensure_len(&mut self.monitor_mapped, mapped_len);

        apply_gain(input.planar, controls.input_gain, &mut self.gained);
        // The loudest captured channel, not the average: a single hot channel
        // must still show on the meter.
        let input_rms = max_channel_rms(&self.gained, in_channels, frames);

        interleave_planar(&self.gained, in_channels, frames, &mut self.captured_interleaved[..captured_len]);

        if let Some(net) = io.net_out.first_mut() {
            net.frames = 0;
            let wire_channels = net.channels.max(1);
            let wire_len = frames * wire_channels;
            // Conform straight into the output when no conversion follows,
            // otherwise into the resampler's input.
            let converting = self.send_resampler.is_some();
            let conformed = if converting {
                self.send_native.get_mut(..wire_len)
            } else {
                debug_assert!(net.interleaved.len() >= wire_len, "net_out is smaller than one render");
                net.interleaved.get_mut(..wire_len)
            };
            if let Some(wire) = conformed {
                if in_channels == wire_channels {
                    wire.copy_from_slice(&self.captured_interleaved[..captured_len]);
                } else {
                    conform_interleaved_channels(
                        &self.captured_interleaved[..captured_len],
                        in_channels,
                        wire,
                        wire_channels,
                    );
                }
                net.frames = match self.send_resampler.as_mut() {
                    None => frames,
                    // A width change would reset the resampler and move the
                    // frame counts this callback was sized with: drop the
                    // history and skip this one callback.
                    Some(resampler) if !resampler.continues(wire_channels) => {
                        resampler.idle();
                        0
                    }
                    // A callback of any other size than the prepared chunk
                    // cannot be converted; the host guarantees a fixed
                    // quantum, so this is a contract break, not a state.
                    Some(resampler) if frames == resampler.input_frames_next() => {
                        resampler.process(&self.send_native[..wire_len], wire_channels, net.interleaved)
                    }
                    Some(_) => {
                        debug_assert!(false, "callback of {frames} frames differs from the prepared chunk");
                        0
                    }
                };
            } else if let Some(resampler) = self.send_resampler.as_mut() {
                resampler.idle();
            }
        } else if let Some(resampler) = self.send_resampler.as_mut() {
            // No streaming ring: nothing is sent, so the history goes stale.
            resampler.idle();
        }

        match io.net_in.first().filter(|net| net.channels > 0) {
            Some(net) => {
                let wire_frames = net.interleaved.len() / net.channels;
                let peer: Option<&[f32]> = match self.recv_resampler.as_mut() {
                    None => net.interleaved.get(..frames * net.channels),
                    Some(resampler) if !resampler.continues(net.channels) => {
                        resampler.idle();
                        None
                    }
                    Some(resampler) => {
                        let converted = frames * net.channels;
                        if frames == resampler.output_frames_next() && wire_frames == resampler.input_frames_next() {
                            // A failed conversion returns 0 and leaves `recv_native`
                            // holding the previous callback's audio; play silence
                            // rather than replaying it.
                            let produced = resampler.process(net.interleaved, net.channels, &mut self.recv_native);
                            if produced == frames { self.recv_native.get(..converted) } else { None }
                        } else {
                            debug_assert!(false, "net_in carries {wire_frames} frames, not the {} the engine asked for", resampler.input_frames_next());
                            None
                        }
                    }
                };
                match peer {
                    Some(peer) => map_to_output(peer, net.channels, &mut self.remote_mapped[..mapped_len], out_channels),
                    None => self.remote_mapped[..mapped_len].fill(0.0),
                }
            }
            None => {
                // No peer stream yet: the history goes stale.
                if let Some(resampler) = self.recv_resampler.as_mut() {
                    resampler.idle();
                }
                self.remote_mapped[..mapped_len].fill(0.0);
            }
        }

        map_to_output(
            &self.captured_interleaved[..captured_len],
            in_channels,
            &mut self.monitor_mapped[..mapped_len],
            out_channels,
        );

        let out_len = output.planar.len().min(mapped_len);
        for i in 0..out_len {
            let mut sample = self.remote_mapped[i];
            if controls.monitor_volume > 0.0 {
                sample += self.monitor_mapped[i] * controls.monitor_volume;
            }
            output.planar[i] = (sample * controls.output_volume).clamp(-1.0, 1.0);
        }

        MixMeters { input_rms }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::limits::wire_frames_per_callback;

    const EPS: f32 = 1e-4;
    const FRAMES: usize = 8;
    const RATE: u32 = 48_000;

    fn controls(monitor_volume: f32) -> MixControls {
        MixControls { input_gain: 1.0, monitor_volume, output_volume: 1.0 }
    }

    /// Planar capture where channel `ch` holds the constant `levels[ch]`.
    fn planar(levels: &[f32]) -> Vec<f32> {
        levels.iter().flat_map(|&l| std::iter::repeat(l).take(FRAMES)).collect()
    }

    /// A mixer prepared for the given layout, or the `prepare` error.
    fn try_prepared(max_frames: usize, max_device_channels: usize, device_rate: u32, wire_rate: u32) -> Result<SimpleMixer, MixError> {
        let mut mixer = SimpleMixer::new();
        mixer.prepare(&MixLayout { max_frames, max_device_channels, device_rate, wire_rate })?;
        Ok(mixer)
    }

    fn prepared(max_device_channels: usize) -> SimpleMixer {
        try_prepared(FRAMES, max_device_channels, RATE, RATE).unwrap()
    }

    #[test]
    fn prepare_rejects_empty_layouts() {
        assert_eq!(try_prepared(0, 2, RATE, RATE).err(), Some(MixError::EmptyLayout("frames")));
        assert_eq!(try_prepared(8, 0, RATE, RATE).err(), Some(MixError::EmptyLayout("device channels")));
    }

    /// A layout sized at the maxima must absorb a render at those maxima with
    /// no scratch growth — growth is an allocation on the render thread.
    #[test]
    fn render_at_prepared_layout_never_grows_scratch() {
        const WIDE: usize = 32;
        let mut mixer = prepared(WIDE);
        let before = mixer.scratch_lens();

        let levels: Vec<f32> = (0..WIDE).map(|c| 0.01 * (c + 1) as f32).collect();
        let input = planar(&levels);
        let mut output = vec![0.0; FRAMES * WIDE];
        let mut wire = vec![0.0; FRAMES * 2];
        let remote = vec![0.1; FRAMES * 2];
        mixer.render(
            &controls(1.0),
            &mut MixIo {
                device_in: &[DeviceIn { planar: &input, channels: WIDE }],
                device_out: &mut [DeviceOut { planar: &mut output, channels: WIDE }],
                net_in: &[NetIn { interleaved: &remote, channels: 2 }],
                net_out: &mut [NetOut { interleaved: &mut wire, channels: 2, frames: 0 }],
            },
        );

        assert_eq!(mixer.scratch_lens(), before);
    }

    /// Stereo capture against a stereo wire and stereo device, remote present:
    /// the wire gets interleaved L,R; the output is remote + monitor per plane;
    /// the meter reports the loudest channel.
    #[test]
    fn render_routes_capture_to_wire_monitor_and_meter() {
        const L: f32 = 0.25;
        const R: f32 = -0.5;
        const REMOTE: f32 = 0.125;
        const MONITOR: f32 = 0.5;
        let mut mixer = prepared(2);

        let input = planar(&[L, R]);
        let mut output = vec![0.0; FRAMES * 2];
        let mut wire = vec![-99.0; FRAMES * 2];
        let remote = vec![REMOTE; FRAMES * 2];
        let meters = mixer.render(
            &controls(MONITOR),
            &mut MixIo {
                device_in: &[DeviceIn { planar: &input, channels: 2 }],
                device_out: &mut [DeviceOut { planar: &mut output, channels: 2 }],
                net_in: &[NetIn { interleaved: &remote, channels: 2 }],
                net_out: &mut [NetOut { interleaved: &mut wire, channels: 2, frames: 0 }],
            },
        );

        for f in 0..FRAMES {
            assert!((wire[f * 2] - L).abs() < EPS && (wire[f * 2 + 1] - R).abs() < EPS, "wire frame {f}");
            assert!((output[f] - (REMOTE + L * MONITOR)).abs() < EPS, "plane 0 frame {f}");
            assert!((output[FRAMES + f] - (REMOTE + R * MONITOR)).abs() < EPS, "plane 1 frame {f}");
        }
        assert!((meters.input_rms - 0.5).abs() < EPS, "meter is the loudest channel, got {}", meters.input_rms);
    }

    /// The absent-network contract: empty `net_in` is silence (output is the
    /// monitor alone) and empty `net_out` writes nothing. A `NetIn` with zero
    /// channels is the same silence.
    #[test]
    fn absent_network_is_silence_and_sends_nothing() {
        const LEVEL: f32 = 0.4;
        for net_in in [&[][..], &[NetIn { interleaved: &[], channels: 0 }][..]] {
            let mut mixer = prepared(1);
            let input = planar(&[LEVEL]);
            let mut output = vec![-99.0; FRAMES];
            mixer.render(
                &controls(1.0),
                &mut MixIo {
                    device_in: &[DeviceIn { planar: &input, channels: 1 }],
                    device_out: &mut [DeviceOut { planar: &mut output, channels: 1 }],
                    net_in,
                    net_out: &mut [],
                },
            );
            assert!(output.iter().all(|&s| (s - LEVEL).abs() < EPS), "monitor only: {output:?}");
        }
    }

    // --- rate conversion -----------------------------------------------------

    const CHUNK: usize = 128;

    fn converting(device_rate: u32, wire_rate: u32) -> SimpleMixer {
        try_prepared(CHUNK, 2, device_rate, wire_rate).unwrap()
    }

    /// A rate the engine cannot convert is refused at `prepare` with both rates
    /// in the message, rather than a stream silently running at the wrong speed.
    #[test]
    fn prepare_rejects_unconvertible_rates_naming_both() {
        for (device_rate, wire_rate) in [(0, 48_000), (4_000, 48_000), (MAX_DEVICE_SAMPLE_RATE + 1, 48_000), (48_000, 0)] {
            let err = try_prepared(CHUNK, 2, device_rate, wire_rate).err().expect("unconvertible rates must be refused");
            assert_eq!(err, MixError::UnsupportedRates { device_rate, wire_rate });
            let message = err.to_string();
            assert!(message.contains(&device_rate.to_string()) && message.contains(&wire_rate.to_string()), "{message}");
        }
    }

    /// The motivating failure: a 96 kHz device on a 48 kHz link. Sent audio
    /// must be packetised as 48 kHz (a 1 kHz capture still reads 1 kHz on the
    /// wire, at half the frames), and received 48 kHz audio must play at the
    /// device's rate with the callback consuming half as many wire frames.
    /// Run at a fractional ratio too (44.1 kHz), where the per-call frame
    /// counts vary and only the long-run average is exact.
    #[test]
    fn render_converts_both_directions_between_device_and_wire_rate() {
        use crate::audio::dsp::test_signals::{measured_hz, tones};
        const TONE: f32 = 1000.0;
        const CALLS: usize = 300;
        for device_rate in [96_000u32, 44_100] {
            let wire_rate = 48_000u32;
            let mut mixer = converting(device_rate, wire_rate);
            let mut sent = Vec::new();
            let mut played = Vec::new();
            let (mut device_clock, mut wire_clock) = (0usize, 0usize);
            for _ in 0..CALLS {
                let needed = mixer.net_in_frames_needed(CHUNK);
                let peer = tones(&[TONE], 0.5, wire_rate, wire_clock, needed);
                wire_clock += needed;
                let capture = tones(&[TONE], 0.5, device_rate, device_clock, CHUNK);
                device_clock += CHUNK;
                let mut output = vec![0.0; CHUNK];
                let mut staging = vec![0.0; wire_frames_per_callback(CHUNK, device_rate, wire_rate)];
                let mut net_out = [NetOut { interleaved: &mut staging, channels: 1, frames: 0 }];
                mixer.render(
                    &MixControls { input_gain: 1.0, monitor_volume: 0.0, output_volume: 1.0 },
                    &mut MixIo {
                        device_in: &[DeviceIn { planar: &capture, channels: 1 }],
                        device_out: &mut [DeviceOut { planar: &mut output, channels: 1 }],
                        net_in: &[NetIn { interleaved: &peer, channels: 1 }],
                        net_out: &mut net_out,
                    },
                );
                let frames = net_out[0].frames;
                sent.extend_from_slice(&staging[..frames]);
                played.extend_from_slice(&output);
            }
            // Wire frames per device frame is wire/device, to within the
            // filter's start-up, which is why this is a rate and not a count.
            let expected_sent = CALLS * CHUNK * wire_rate as usize / device_rate as usize;
            assert!(sent.len().abs_diff(expected_sent) <= 2, "{device_rate}: sent {} wire frames, expected {expected_sent}", sent.len());
            assert!(wire_clock.abs_diff(expected_sent) <= 2, "{device_rate}: consumed {wire_clock} wire frames, expected {expected_sent}");
            let on_wire = measured_hz(&sent, 1, 0, wire_rate, sent.len() / 5);
            assert!((on_wire - TONE).abs() < 10.0, "{device_rate}: capture reached the wire as {on_wire} Hz");
            let at_device = measured_hz(&played, 1, 0, device_rate, played.len() / 5);
            assert!((at_device - TONE).abs() < 10.0, "{device_rate}: peer played as {at_device} Hz");
        }
    }

    /// Review of #116: callbacks that skip a resampler (ring not streaming, no
    /// peer yet) must not leave its filter history behind, or the audio from
    /// before the gap rings out into the silence after it.
    #[test]
    fn resamplers_drop_history_across_skipped_callbacks() {
        let mut mixer = converting(96_000, 48_000);
        let controls = MixControls { input_gain: 1.0, monitor_volume: 0.0, output_volume: 1.0 };
        let loud = vec![0.9; CHUNK];
        let quiet = vec![0.0; CHUNK];
        let wire_loud = vec![0.9; mixer.net_in_frames_needed(CHUNK)];
        let mut output = vec![0.0; CHUNK];
        let mut staging = vec![0.0; wire_frames_per_callback(CHUNK, 96_000, 48_000)];
        let mut net_out = [NetOut { interleaved: &mut staging, channels: 1, frames: 0 }];
        mixer.render(
            &controls,
            &mut MixIo {
                device_in: &[DeviceIn { planar: &loud, channels: 1 }],
                device_out: &mut [DeviceOut { planar: &mut output, channels: 1 }],
                net_in: &[NetIn { interleaved: &wire_loud, channels: 1 }],
                net_out: &mut net_out,
            },
        );
        // A callback with neither a send ring nor a peer.
        mixer.render(
            &controls,
            &mut MixIo {
                device_in: &[DeviceIn { planar: &quiet, channels: 1 }],
                device_out: &mut [DeviceOut { planar: &mut output, channels: 1 }],
                net_in: &[],
                net_out: &mut [],
            },
        );
        // Both streams resume with silence: nothing of the loud audio may remain.
        let wire_quiet = vec![0.0; mixer.net_in_frames_needed(CHUNK)];
        let mut net_out = [NetOut { interleaved: &mut staging, channels: 1, frames: 0 }];
        mixer.render(
            &controls,
            &mut MixIo {
                device_in: &[DeviceIn { planar: &quiet, channels: 1 }],
                device_out: &mut [DeviceOut { planar: &mut output, channels: 1 }],
                net_in: &[NetIn { interleaved: &wire_quiet, channels: 1 }],
                net_out: &mut net_out,
            },
        );
        let sent = net_out[0].frames;
        assert!(staging[..sent].iter().all(|&s| s == 0.0), "send resampler replayed stale history");
        assert!(output.iter().all(|&s| s == 0.0), "recv resampler replayed stale history");
    }
}
