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
    apply_gain, conform_interleaved_channels, interleave_planar, map_to_output, max_channel_rms,
};

/// Why a layout was refused by [`MixEngine::prepare`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixError {
    /// A dimension of the layout was zero, which could never hold a stream.
    EmptyLayout(&'static str),
}

impl std::fmt::Display for MixError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MixError::EmptyLayout(what) => write!(f, "mix layout has zero {what}"),
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

/// A received network stream, interleaved: `interleaved[frame * channels + ch]`.
/// `channels == 0` means no audio has arrived yet and counts as silence.
pub struct NetIn<'a> {
    pub interleaved: &'a [f32],
    pub channels: usize,
}

/// A network stream to send, interleaved. The caller sizes `interleaved` to
/// at least `frames * channels`; `render` fills exactly that prefix.
pub struct NetOut<'a> {
    pub interleaved: &'a mut [f32],
    pub channels: usize,
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
fn ensure_len(buf: &mut Vec<f32>, len: usize) {
    if buf.len() < len {
        buf.resize(len, 0.0);
    }
}

impl MixEngine for SimpleMixer {
    fn prepare(&mut self, layout: &MixLayout) -> Result<(), MixError> {
        if layout.max_frames == 0 {
            return Err(MixError::EmptyLayout("frames"));
        }
        if layout.max_device_channels == 0 {
            return Err(MixError::EmptyLayout("device channels"));
        }
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
            let wire_channels = net.channels.max(1);
            let wire_len = frames * wire_channels;
            debug_assert!(net.interleaved.len() >= wire_len, "net_out is smaller than one render");
            if let Some(wire) = net.interleaved.get_mut(..wire_len) {
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
            }
        }

        match io.net_in.first().filter(|net| net.channels > 0) {
            Some(net) => map_to_output(
                &net.interleaved[..frames * net.channels],
                net.channels,
                &mut self.remote_mapped[..mapped_len],
                out_channels,
            ),
            None => self.remote_mapped[..mapped_len].fill(0.0),
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

    const EPS: f32 = 1e-4;
    const FRAMES: usize = 8;

    fn controls(monitor_volume: f32) -> MixControls {
        MixControls { input_gain: 1.0, monitor_volume, output_volume: 1.0 }
    }

    /// Planar capture where channel `ch` holds the constant `levels[ch]`.
    fn planar(levels: &[f32]) -> Vec<f32> {
        levels.iter().flat_map(|&l| std::iter::repeat(l).take(FRAMES)).collect()
    }

    fn prepared(max_device_channels: usize) -> SimpleMixer {
        let mut mixer = SimpleMixer::new();
        mixer.prepare(&MixLayout { max_frames: FRAMES, max_device_channels }).unwrap();
        mixer
    }

    #[test]
    fn prepare_rejects_empty_layouts() {
        let mut mixer = SimpleMixer::new();
        assert_eq!(
            mixer.prepare(&MixLayout { max_frames: 0, max_device_channels: 2 }),
            Err(MixError::EmptyLayout("frames"))
        );
        assert_eq!(
            mixer.prepare(&MixLayout { max_frames: 8, max_device_channels: 0 }),
            Err(MixError::EmptyLayout("device channels"))
        );
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
                net_out: &mut [NetOut { interleaved: &mut wire, channels: 2 }],
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
                net_out: &mut [NetOut { interleaved: &mut wire, channels: 2 }],
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
}
