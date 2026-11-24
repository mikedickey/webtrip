use std::sync::atomic::Ordering;

use crate::audio::mixer::{ensure_len, DeviceIn, DeviceOut, MixControls, MixEngine, MixError, MixIo, MixLayout, NetIn, NetOut, SimpleMixer};
use crate::audio::params::{AudioParams, MAX_DB, MIN_DB, decode_db, decode_volume, encode_db};
use crate::limits::{wire_frames_per_callback, MAX_DEVICE_CHANNELS, MAX_WIRE_CHANNELS, RENDER_QUANTUM_FRAMES, WIRE_SAMPLE_RATE};
use crate::stream::regulator::Regulator;
use crate::stream::ring_buffer::RingBuffer;
use crate::stream::shared_ptr::SharedPtr;

/// Peak hold time in process calls (~48kHz / 128 samples = ~375 calls/sec)
/// Hold peak for about 1.5 seconds
const PEAK_HOLD_FRAMES: u32 = 560;
/// Peak decay rate in dB per process call (smooth falloff)
const PEAK_DECAY_RATE: f32 = 0.15;

// ==============================================================================
// Meter math — dB conversion and peak hold/decay. These need `params`'
// range constants, so they live here rather than in `dsp`.
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
// AudioProcessor — pointer work and atomics around a `MixEngine`
// ==============================================================================

/// Core audio processor for real-time audio processing.
///
/// Owns every pointer and atomic (ring, regulator, `AudioParams`); the mixing
/// itself lives behind [`MixEngine`], which sees only plain slices.
pub struct AudioProcessor {
    params: &'static AudioParams,
    /// Ring buffer for sending local audio to network (audio device → worklet → main thread → network).
    /// Null when no network is attached; reached via its `&self` API through [`SharedPtr::as_ref`].
    local_to_network_buffer: SharedPtr<RingBuffer>,
    /// Jitter buffer for receiving audio from network (network → main thread → jitter buffer → worklet → audio device).
    /// Null when no network is attached; reached via its `&self` SPSC API through [`SharedPtr::as_ref`].
    network_to_local_buffer: SharedPtr<Regulator>,
    mixer: Box<dyn MixEngine>,
    /// The regulator's output (peer audio at the wire's channel count), before
    /// mapping to the output device's channel count. Preallocated at
    /// `max_callback_frames * MAX_WIRE_CHANNELS`: a callback never exceeds it,
    /// so the render thread never resizes.
    remote_interleaved: Vec<f32>,
    /// Staging for the mixer's network output (the capture conformed to the
    /// session's fixed wire channel count), written to the ring afterwards.
    /// Same preallocation as `remote_interleaved`.
    wire_staging: Vec<f32>,
    device_rate: u32,
    wire_rate: u32,
}

impl AudioProcessor {
    pub fn new(params: &'static AudioParams) -> Self {
        // No network streams, so the rates never convert anything.
        Self::with_network(
            params,
            SharedPtr::null(),
            SharedPtr::null(),
            RENDER_QUANTUM_FRAMES,
            WIRE_SAMPLE_RATE,
            WIRE_SAMPLE_RATE,
        )
        .expect("equal rates always build")
    }

    /// Create processor with network audio support
    /// - local_to_network_buffer: ring buffer for sending local audio to network
    /// - network_to_local_buffer: jitter buffer for receiving audio from network
    /// - max_callback_frames: the most frames any `process` call will carry;
    ///   every render-thread scratch buffer is sized from it here, off the
    ///   render thread
    /// - device_rate / wire_rate: the audio device's actual rate and the rate
    ///   network streams (and the regulator) run at. When they differ the mixer
    ///   converts both directions; the regulator only ever sees wire frames.
    ///
    /// Fails when the rates cannot be converted between, naming both.
    pub fn with_network(
        params: &'static AudioParams,
        local_to_network_buffer: SharedPtr<RingBuffer>,
        network_to_local_buffer: SharedPtr<Regulator>,
        max_callback_frames: usize,
        device_rate: u32,
        wire_rate: u32,
    ) -> Result<Self, MixError> {
        // Validate the rates first: everything below divides by or sizes from
        // them (a 0 Hz device would divide by zero), so an unsupported rate must
        // surface as `Err`, not a panic.
        // Device-width scratch is sized from the device ceiling so a wide
        // interface never `resize`s on the render thread.
        let mut mixer = SimpleMixer::new();
        mixer.prepare(&MixLayout {
            max_frames: max_callback_frames,
            max_device_channels: MAX_DEVICE_CHANNELS as usize,
            device_rate,
            wire_rate,
        })?;
        let max_wire_frames = wire_frames_per_callback(max_callback_frames, device_rate, wire_rate);
        // The regulator's FIFO capacity and this callback ceiling are set
        // independently; a regulator smaller than the callbacks it serves
        // returns 0 from every pop (silent peer, nothing logged). Setup is off
        // the render thread, so fail loudly here instead.
        if let Some(regulator) = network_to_local_buffer.as_ref() {
            if regulator.callback_capacity() < max_wire_frames {
                return Err(MixError::RegulatorCapacity { capacity: regulator.callback_capacity(), needed: max_wire_frames });
            }
            if regulator.sample_rate() != wire_rate {
                return Err(MixError::RegulatorRate { regulator_rate: regulator.sample_rate(), wire_rate });
            }
        }
        let wire_scratch_len = max_wire_frames * MAX_WIRE_CHANNELS as usize;
        Ok(Self {
            params,
            local_to_network_buffer,
            network_to_local_buffer,
            mixer: Box::new(mixer),
            remote_interleaved: vec![0.0; wire_scratch_len],
            wire_staging: vec![0.0; wire_scratch_len],
            device_rate,
            wire_rate,
        })
    }

    /// Process audio: calculate volume levels, handle network audio, and generate output.
    ///
    /// `input` is planar across `in_channels` input channels: `input[ch *
    /// frames + frame]`, `frames == input.len() / in_channels`. `output` is
    /// planar across `out_channels` output channels: `output[ch * frames +
    /// frame]`.
    ///
    /// `now_ms` is the host's wall-clock time for this callback, handed to the
    /// regulator's `pop_frames_at`; the processor itself never reads a clock.
    pub fn process(&mut self, input: &[f32], in_channels: usize, output: &mut [f32], out_channels: usize, now_ms: f64) -> bool {
        let in_channels = in_channels.max(1);
        let out_channels = out_channels.max(1);
        let frames = input.len() / in_channels;

        // Increment callback counter for stats tracking
        self.params.callback_count.fetch_add(1, Ordering::Relaxed);

        let controls = MixControls {
            input_gain: db_to_linear(self.params.input_gain_db.load(Ordering::Relaxed) as f32 / 100.0),
            monitor_volume: decode_volume(self.params.monitor_volume.load(Ordering::Relaxed)),
            output_volume: decode_volume(self.params.output_volume.load(Ordering::Relaxed)),
        };

        // Pop the peer stream first; `remote_channels == 0` (no network buffer,
        // or nothing adopted yet) is the empty `net_in` — silence. The mixer
        // says how many *wire* frames this callback consumes: more or fewer
        // than `frames` when it converts between the device and wire rates, so
        // the regulator's pull clock still ticks once per device callback.
        // A callback past `max_callback_frames` breaks the preallocation
        // contract; degrade to one allocation (as the mixer does) rather than
        // panic on the render thread.
        let wire_in_frames = self.mixer.net_in_frames_needed(frames);
        ensure_len(&mut self.remote_interleaved, wire_in_frames * MAX_WIRE_CHANNELS as usize);
        let remote_channels = self.receive_from_network(wire_in_frames, now_ms);
        let net_in = [NetIn {
            interleaved: &self.remote_interleaved[..wire_in_frames * remote_channels],
            channels: remote_channels,
        }];
        let net_in: &[NetIn] = if remote_channels == 0 { &[] } else { &net_in };

        // Send only while a streaming ring is attached: otherwise `net_out` is
        // empty and the mixer skips the conform entirely. The ring is read
        // through its `&self` API (sound shared borrow, see `RingBuffer`).
        let ring = self.local_to_network_buffer.as_ref().filter(|ring| ring.is_streaming());
        let wire_channels = ring.map_or(1, |ring| ring.wire_channels());
        // The mixer yields a varying number of wire frames per call when it
        // converts rates; the staging buffer holds the most it can.
        let wire_out_len = wire_frames_per_callback(frames, self.device_rate, self.wire_rate) * wire_channels;
        ensure_len(&mut self.wire_staging, wire_out_len);
        let mut net_out = [NetOut {
            interleaved: &mut self.wire_staging[..if ring.is_some() { wire_out_len } else { 0 }],
            channels: wire_channels,
            frames: 0,
        }];
        let net_out_slice: &mut [NetOut] = if ring.is_some() { &mut net_out } else { &mut [] };

        let meters = self.mixer.render(
            &controls,
            &mut MixIo {
                device_in: &[DeviceIn { planar: input, channels: in_channels }],
                device_out: &mut [DeviceOut { planar: output, channels: out_channels }],
                net_in,
                net_out: net_out_slice,
            },
        );
        let wire_out_frames = net_out[0].frames;

        if let Some(ring) = ring {
            ring.write(&self.wire_staging[..wire_out_frames * wire_channels]);
        }

        let current_db = amplitude_to_db(meters.input_rms);
        self.params.db_level.store(encode_db(current_db), Ordering::Relaxed);
        self.update_peak_level(current_db);

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

    /// Receive `frames` frames of remote audio into `remote_interleaved` from
    /// the regulator. Returns the stream's channel count — read after the
    /// regulator's Acquire (WEB-53 Bug 3) — or `0` when no network buffer is
    /// attached or it cannot serve `frames` (the caller treats that as
    /// silence). The regulator must have been built with a callback capacity of
    /// at least `frames`.
    ///
    /// The regulator's concealed-vs-real distinction is informational and
    /// must NOT gate playback: concealed audio is the entire point of jitter
    /// buffering and must be played to avoid clicks.
    fn receive_from_network(&mut self, frames: usize, now_ms: f64) -> usize {
        match self.network_to_local_buffer.as_ref() {
            Some(regulator) => regulator.pop_frames_at(&mut self.remote_interleaved, frames, now_ms),
            None => 0,
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
    use crate::limits::{MAX_DEVICE_SAMPLE_RATE, MIN_DEVICE_SAMPLE_RATE};

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
        processor.process(&loud, 1, &mut out, 1, 0.0);

        let stored_after_peak = params.peak_db_level.load(std::sync::atomic::Ordering::Relaxed);
        assert!(stored_after_peak > encode_db(-60.0), "a loud signal must raise the peak");

        // Now feed silence for exactly PEAK_HOLD_FRAMES callbacks.
        // The stored fixed-point value must remain bit-for-bit identical throughout.
        let silence: Vec<f32> = vec![0.0; 128];
        for frame in 0..PEAK_HOLD_FRAMES {
            processor.process(&silence, 1, &mut out, 1, 0.0);
            let stored = params.peak_db_level.load(std::sync::atomic::Ordering::Relaxed);
            assert_eq!(
                stored,
                stored_after_peak,
                "peak_db_level drifted on hold frame {frame}: expected {stored_after_peak}, got {stored}"
            );
        }

        // After hold expires, decay must have begun (stored value decreases).
        processor.process(&silence, 1, &mut out, 1, 0.0);
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
        // SAFETY: `ring` is borrowed for the processor's lifetime in these
        // tests; the processor never outlives the stack local.
        let ring_ptr = unsafe { SharedPtr::new(ring as *mut RingBuffer) };
        matched_rate_processor(params, ring_ptr, SharedPtr::null(), RENDER_QUANTUM_FRAMES).expect("matched rates build")
    }

    /// `with_network` at the wire rate on both sides: only the ring,
    /// regulator and callback size vary between these tests.
    fn matched_rate_processor(
        params: &'static AudioParams,
        ring: SharedPtr<RingBuffer>,
        regulator: SharedPtr<crate::stream::regulator::Regulator>,
        frames: usize,
    ) -> Result<AudioProcessor, MixError> {
        AudioProcessor::with_network(params, ring, regulator, frames, WIRE_SAMPLE_RATE, WIRE_SAMPLE_RATE)
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
        params.set_monitor_volume(MONITOR);

        let mut ring = RingBuffer::new();
        ring.set_wire_format(2);
        let mut processor = networked_processor(params, &mut ring);

        let input = planar_capture(&[LEFT, RIGHT], FRAMES);
        let mut output = vec![-99.0f32; FRAMES * 2];
        assert!(processor.process(&input, 2, &mut output, 2, 0.0));

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
    /// session's fixed send width (`RingBuffer::wire_channels`, chosen at
    /// connect time), so when the capture is NARROWER than the send width —
    /// a mono-only device against a stereo send — `process` must conform
    /// before writing. Writing `frames * in_channels`
    /// instead would pack two mono quantums into one "stereo" packet, which
    /// the peer decodes as double-speed garble.
    #[test]
    fn test_process_conforms_mono_capture_to_stereo_wire_width() {
        const FRAMES: usize = 128;
        const LEVEL: f32 = 0.4;

        let params: &'static AudioParams = Box::leak(Box::new(AudioParams::default()));

        let mut ring = RingBuffer::new();
        ring.set_wire_format(2); // the wire is stereo …
        let mut processor = networked_processor(params, &mut ring);

        // … but the browser granted mono.
        let input = planar_capture(&[LEVEL], FRAMES);
        let mut output = vec![0.0f32; FRAMES * 2];
        assert!(processor.process(&input, 1, &mut output, 2, 0.0));

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

    /// A full-width device (capture and playback at `MAX_DEVICE_CHANNELS`,
    /// well past the wire ceiling) must not make the processor's wire-width
    /// buffers grow: a `resize` on the render thread allocates. (The mixer's
    /// device-width buffers are pinned in `mixer.rs`.)
    #[test]
    fn test_process_full_width_device_never_resizes_scratch() {
        const FRAMES: usize = 128;
        const WIDE: usize = MAX_DEVICE_CHANNELS as usize;

        let params: &'static AudioParams = Box::leak(Box::new(AudioParams::default()));
        let mut ring = RingBuffer::new();
        ring.set_wire_format(2);
        let mut processor = networked_processor(params, &mut ring);
        let lens = |p: &AudioProcessor| [p.remote_interleaved.len(), p.wire_staging.len()];
        let before = lens(&processor);

        let levels: Vec<f32> = (0..WIDE).map(|c| 0.01 * (c + 1) as f32).collect();
        let input = planar_capture(&levels, FRAMES);
        let mut output = vec![0.0f32; FRAMES * WIDE];
        assert!(processor.process(&input, WIDE, &mut output, WIDE, 0.0));

        assert_eq!(lens(&processor), before, "no scratch buffer may resize on the render thread");
    }

    /// A callback larger than `max_callback_frames` (a host with a 256- or
    /// 512-frame callback built with the default capacity) must degrade, not
    /// panic on the render thread, and the oversized capture must still reach
    /// the ring.
    #[test]
    fn test_process_callback_beyond_max_frames_degrades() {
        const FRAMES: usize = RENDER_QUANTUM_FRAMES * 2;
        const LEVEL: f32 = 0.25;

        let params: &'static AudioParams = Box::leak(Box::new(AudioParams::default()));
        let mut ring = RingBuffer::new();
        ring.set_wire_format(1);
        let mut processor = networked_processor(params, &mut ring);

        let input = planar_capture(&[LEVEL], FRAMES);
        let mut output = vec![0.0f32; FRAMES];
        assert!(processor.process(&input, 1, &mut output, 1, 0.0));

        let mut wire = vec![0.0f32; FRAMES];
        assert!(ring.read(&mut wire), "the whole oversized callback must reach the ring");
        assert!(wire.iter().all(|&s| (s - LEVEL).abs() < EPS));
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

    /// A regulator whose FIFO is smaller than the callbacks the processor will
    /// carry would silence the peer on every pop; the mismatch must be caught
    /// at construction, not discovered as silence.
    #[test]
    fn test_with_network_rejects_mismatched_regulator() {
        use crate::stream::regulator::Regulator;
        let params: &'static AudioParams = Box::leak(Box::new(AudioParams::default()));
        // Default capacity is one packet (64 frames); callbacks are 128.
        let reg = Regulator::with_params(1, 64, 48_000, 5.0);
        let result = matched_rate_processor(params, SharedPtr::null(), SharedPtr::from_ref(&reg), RENDER_QUANTUM_FRAMES);
        assert!(matches!(result, Err(MixError::RegulatorCapacity { capacity: 64, .. })));

        // Capacity is fine but the regulator runs at a rate other than the wire's.
        let reg = Regulator::with_params(1, 64, 44_100, 5.0).with_callback_capacity(RENDER_QUANTUM_FRAMES);
        let result = matched_rate_processor(params, SharedPtr::null(), SharedPtr::from_ref(&reg), RENDER_QUANTUM_FRAMES);
        assert!(matches!(result, Err(MixError::RegulatorRate { regulator_rate: 44_100, .. })));
    }

    /// An unsupported rate is an `Err` even with a regulator attached: the
    /// rate check runs before the sizing math (0 Hz would divide by zero) and
    /// the regulator asserts (a sub-minimum rate would trip the capacity one).
    #[test]
    fn test_with_network_returns_err_for_unsupported_device_rate() {
        use crate::stream::regulator::Regulator;
        let params: &'static AudioParams = Box::leak(Box::new(AudioParams::default()));
        let reg = Regulator::with_params(1, 64, WIRE_SAMPLE_RATE, 5.0);
        for device_rate in [0, MIN_DEVICE_SAMPLE_RATE - 1, MAX_DEVICE_SAMPLE_RATE + 1] {
            let result = AudioProcessor::with_network(params, SharedPtr::null(), SharedPtr::from_ref(&reg), RENDER_QUANTUM_FRAMES, device_rate, WIRE_SAMPLE_RATE);
            assert!(matches!(result, Err(MixError::UnsupportedRates { .. })), "device rate {device_rate}");
        }
    }

    /// The processor must hand the peer stream to the mixer at the width and
    /// frame count the regulator returned: a 256-fpp peer served by 128-frame
    /// callbacks (a packet split across callbacks) reaches the output intact.
    /// Packet/callback ratios themselves are the regulator's contract, pinned
    /// in its own tests.
    #[test]
    fn test_process_plays_peer_stream_to_output() {
        use crate::stream::regulator::{test_support::*, Regulator};
        const FPP: usize = 256;
        const FRAMES: usize = 128;
        const LEVEL: f32 = 0.5;

        let params: &'static AudioParams = Box::leak(Box::new(AudioParams::default()));
        let reg = Regulator::with_params(1, FPP, 48_000, 5.0).with_callback_capacity(FRAMES);
        let mut processor = matched_rate_processor(params, SharedPtr::null(), SharedPtr::from_ref(&reg), FRAMES).expect("matched rates build");
        let input = vec![0.0f32; FRAMES];
        let mut output = vec![0.0f32; FRAMES];
        let mut checked = 0;
        drive_callbacks(&reg, (FPP, FRAMES, 1), 120, |_| vec![LEVEL; FPP], |k, now| {
            processor.process(&input, 1, &mut output, 1, now);
            if k >= SETTLE_CALLBACKS {
                checked += 1;
                assert!(output.iter().all(|&s| (s - LEVEL).abs() < EPS), "callback {k}: peer audio must fill the whole callback");
            }
        });
        assert!(checked > 0);
    }

    /// A 96 kHz device on the 48 kHz link (the observed failure): each device
    /// callback must pull the wire frames it spans — half as many — so the
    /// peer's tone plays at its own pitch. Pulling a device-sized 128 frames
    /// per callback plays it at double speed and, with the pull clock running
    /// twice the packet rate, conceals half of every pop.
    #[test]
    fn test_process_plays_peer_at_device_rate() {
        use crate::audio::dsp::test_signals::{measured_hz, tones};
        use crate::stream::regulator::{test_support::*, Regulator};
        const DEVICE_RATE: u32 = 96_000;
        const TONE: f32 = 1000.0;
        // One packet per callback: 64 wire frames span one 128-frame callback.
        const FPP: usize = 64;

        let params: &'static AudioParams = Box::leak(Box::new(AudioParams::default()));
        let reg = Regulator::with_params(1, FPP, WIRE_SAMPLE_RATE, 5.0)
            .with_callback_capacity(wire_frames_per_callback(RENDER_QUANTUM_FRAMES, DEVICE_RATE, WIRE_SAMPLE_RATE));
        let mut processor = AudioProcessor::with_network(
            params,
            SharedPtr::null(),
            SharedPtr::from_ref(&reg),
            RENDER_QUANTUM_FRAMES,
            DEVICE_RATE,
            WIRE_SAMPLE_RATE,
        )
        .expect("a 96 kHz device converts to the wire rate");
        let input = vec![0.0f32; RENDER_QUANTUM_FRAMES];
        let mut output = vec![0.0f32; RENDER_QUANTUM_FRAMES];
        let mut played = Vec::new();
        drive_callbacks(&reg, (FPP, FPP, 1), 300, |seq| tones(&[TONE], 0.5, WIRE_SAMPLE_RATE, seq * FPP, FPP), |k, now| {
            processor.process(&input, 1, &mut output, 1, now);
            if k >= SETTLE_CALLBACKS {
                played.extend_from_slice(&output);
            }
        });
        let pitch = measured_hz(&played, 1, 0, DEVICE_RATE, 0);
        assert!((pitch - TONE).abs() < 10.0, "a {TONE} Hz peer tone played at {pitch} Hz");
    }
}
