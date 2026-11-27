use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, Ordering};
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// Minimum dB level (silence threshold)
pub const MIN_DB: f32 = -60.0;
/// Maximum dB level (clipping)
pub const MAX_DB: f32 = 0.0;

#[inline] pub(crate) fn encode_db(db: f32) -> i32     { ((db - MIN_DB) * 100.0) as i32 }
#[inline] pub(crate) fn decode_db(stored: i32) -> f32 { (stored as f32 / 100.0) + MIN_DB }
#[inline] pub(crate) fn encode_volume(v: f32) -> u32   { (v * 1000.0) as u32 }
#[inline] pub(crate) fn decode_volume(stored: u32) -> f32 { stored as f32 / 1000.0 }

#[inline]
fn db_to_percent(db: f32) -> f32 {
    ((db - MIN_DB) / (MAX_DB - MIN_DB) * 100.0).max(0.0).min(100.0)
}

/// Controls and meters shared between the control thread and the audio thread.
///
/// Every field is an atomic, so any thread may call any method at any time.
/// The control side sets gain and volumes; the
/// [`AudioProcessor`](crate::audio::processor::AudioProcessor) reads them each
/// callback and writes the input meters and callback count back. Values are
/// stored in fixed point: dB to 0.01 dB, volumes to 0.001.
///
/// In Rust, create one with [`Default`] (unity gain, full output, monitor off)
/// and give the processor a `&'static` to it, e.g. `Box::leak(Box::default())`,
/// or call [`create_audio_params`], which does that and returns an
/// [`AudioParamsHandle`]. JavaScript callers get the handle from
/// `createAudioParams()` and use its methods, which mirror these. The JavaScript
/// class named `AudioParams` is that handle, not this struct, which has no
/// JavaScript binding.
pub struct AudioParams {
    /// Current dB level (stored as (db - MIN_DB) * 100 for fixed-point)
    pub(crate) db_level: AtomicI32,
    /// Peak dB level with hold/decay (stored same as db_level)
    pub(crate) peak_db_level: AtomicI32,
    /// Peak hold counter (frames remaining before decay starts)
    pub(crate) peak_hold_counter: AtomicU32,
    /// Input gain in dB * 100 (range: -2000 to +2000 for -20dB to +20dB)
    pub(crate) input_gain_db: AtomicI32,
    /// Output volume as linear multiplier * 1000 (range: 0 to 1000 for 0.0 to 1.0)
    pub(crate) output_volume: AtomicU32,
    /// Monitor volume as linear multiplier * 1000 (range: 0 to 1000 for 0.0 to 1.0)
    /// Replaces the old loopback toggle - 0 means no monitoring
    pub(crate) monitor_volume: AtomicU32,
    /// Total number of audio callbacks (process() calls)
    pub(crate) callback_count: AtomicU64,
}

impl Default for AudioParams {
    fn default() -> Self {
        Self {
            db_level: AtomicI32::new(0),
            peak_db_level: AtomicI32::new(0),
            peak_hold_counter: AtomicU32::new(0),
            input_gain_db: AtomicI32::new(0), // 0 dB (unity gain)
            output_volume: AtomicU32::new(1000), // 1.0 (full volume)
            monitor_volume: AtomicU32::new(0), // 0.0 (monitoring off by default)
            callback_count: AtomicU64::new(0),
        }
    }
}

/// A handle to an [`AudioParams`] that lives for the rest of the program.
///
/// Exported to JavaScript and TypeScript as the class `AudioParams` (the Rust
/// struct of that name is not exported), so `new WebTripSession(params)` takes
/// "an `AudioParams`" there and an `AudioParamsHandle` here.
///
/// [`create_audio_params`] (`createAudioParams()` in JavaScript) returns one.
/// Pass it to `new WebTripSession(..)`, and read and write the parameters
/// through its methods, which mirror [`AudioParams`]'s. The parameters are
/// never freed, so a handle, and every copy of it, stays valid: JavaScript's
/// `free()` releases only the handle object.
#[cfg_attr(feature = "wasm", wasm_bindgen(js_name = AudioParams))]
#[derive(Clone, Copy)]
pub struct AudioParamsHandle {
    params: &'static AudioParams,
}

impl AudioParamsHandle {
    /// A handle to `params`.
    pub fn new(params: &'static AudioParams) -> Self {
        Self { params }
    }

    /// The parameters themselves, as an
    /// [`AudioProcessor`](crate::audio::processor::AudioProcessor) takes them.
    pub fn get(self) -> &'static AudioParams {
        self.params
    }
}

/// Defines each [`AudioParams`] method once and, from the same definition, the
/// JavaScript method of the same name on [`AudioParamsHandle`] that forwards to
/// it. A new parameter, or a changed doc range, is edited here and nowhere else.
macro_rules! audio_param_methods {
    ($(
        $(#[$doc:meta])*
        $js_name:ident => fn $name:ident($self_:ident $(, $arg:ident: $ty:ty)*) $(-> $ret:ty)? $body:block
    )*) => {
        impl AudioParams {
            $(
                $(#[$doc])*
                pub fn $name(&$self_ $(, $arg: $ty)*) $(-> $ret)? $body
            )*
        }

        /// The JavaScript surface of the handle: each method is the
        /// [`AudioParams`] method of the same name.
        #[cfg(feature = "wasm")]
        #[wasm_bindgen(js_class = "AudioParams")]
        impl AudioParamsHandle {
            $(
                $(#[$doc])*
                #[wasm_bindgen(js_name = $js_name)]
                pub fn $name(&self $(, $arg: $ty)*) $(-> $ret)? {
                    self.params.$name($($arg),*)
                }
            )*
        }
    };
}

audio_param_methods! {
    /// Get the current dB level (-60.0 to 0.0)
    getDbLevel => fn get_db_level(self) -> f32 {
        decode_db(self.db_level.load(Ordering::Relaxed))
    }

    /// Get the peak dB level (-60.0 to 0.0)
    getPeakDbLevel => fn get_peak_db_level(self) -> f32 {
        decode_db(self.peak_db_level.load(Ordering::Relaxed))
    }

    /// Get the current volume level as a percentage (0.0 to 100.0)
    /// Maps -60dB to 0% and 0dB to 100%
    getVolumeLevel => fn get_volume_level(self) -> f32 {
        db_to_percent(self.get_db_level())
    }

    /// Get the peak level as a percentage (0.0 to 100.0)
    getPeakLevel => fn get_peak_level(self) -> f32 {
        db_to_percent(self.get_peak_db_level())
    }

    /// Set monitor volume (0.0 to 1.0)
    setMonitorVolume => fn set_monitor_volume(self, volume: f32) {
        self.monitor_volume.store(encode_volume(volume.clamp(0.0, 1.0)), Ordering::Relaxed);
    }

    /// Get monitor volume (0.0 to 1.0)
    getMonitorVolume => fn get_monitor_volume(self) -> f32 {
        decode_volume(self.monitor_volume.load(Ordering::Relaxed))
    }

    /// Get the total number of audio callbacks processed so far
    getCallbackCount => fn get_callback_count(self) -> u64 {
        self.callback_count.load(Ordering::Relaxed)
    }

    /// Set input gain in dB (-20.0 to +20.0)
    setInputGain => fn set_input_gain(self, gain_db: f32) {
        let clamped = gain_db.clamp(-20.0, 20.0);
        self.input_gain_db.store((clamped * 100.0) as i32, Ordering::Relaxed);
    }

    /// Get input gain in dB
    getInputGain => fn get_input_gain(self) -> f32 {
        self.input_gain_db.load(Ordering::Relaxed) as f32 / 100.0
    }

    /// Set output volume (0.0 to 1.0)
    setOutputVolume => fn set_output_volume(self, volume: f32) {
        self.output_volume.store(encode_volume(volume.clamp(0.0, 1.0)), Ordering::Relaxed);
    }

    /// Get output volume (0.0 to 1.0)
    getOutputVolume => fn get_output_volume(self) -> f32 {
        decode_volume(self.output_volume.load(Ordering::Relaxed))
    }
}

/// Create a new set of shared audio controls and meters that is never freed,
/// and return a handle to it.
///
/// Pass the handle to `new WebTripSession(...)`. Create it once per page (or
/// audio engine): sessions may share it, and because the parameters are never
/// freed the handle stays valid on every thread that holds it.
#[cfg_attr(feature = "wasm", wasm_bindgen(js_name = createAudioParams))]
pub fn create_audio_params() -> AudioParamsHandle {
    AudioParamsHandle::new(Box::leak(Box::default()))
}

// ==============================================================================
// Tests
// ==============================================================================
//
// These run on the native target via `npm run test` (i.e. `cargo test` with
// `--cfg=web_sys_unstable_apis`). `AudioParams` does its work in plain Rust with
// `#[wasm_bindgen]` only for JS interop, so it is fully testable natively.
//
// `db_level` / `peak_db_level` have no public setters — they are written by
// `AudioProcessor` using the fixed-point encoding `stored = (db - MIN_DB) * 100`.
// The tests reach the `pub(crate)` atomics directly to drive the dB getters,
// mirroring that encoding via the `encode_db` helper.
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::thread;

    /// Tolerance for float comparisons. Fixed-point storage resolves to 0.01 dB
    /// (levels/gain) and 0.001 (linear volume), so 1e-4 is comfortably tight.
    const EPS: f32 = 1e-4;

    #[test]
    fn test_monitor_volume_roundtrip_and_clamp() {
        let p = AudioParams::default();
        for v in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            p.set_monitor_volume(v);
            assert!((p.get_monitor_volume() - v).abs() < EPS, "monitor volume {v} did not round-trip");
        }
        // Out-of-range inputs clamp to [0.0, 1.0].
        p.set_monitor_volume(-0.5);
        assert!((p.get_monitor_volume() - 0.0).abs() < EPS);
        p.set_monitor_volume(2.0);
        assert!((p.get_monitor_volume() - 1.0).abs() < EPS);
    }

    #[test]
    fn test_output_volume_roundtrip_and_clamp() {
        let p = AudioParams::default();
        for v in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            p.set_output_volume(v);
            assert!((p.get_output_volume() - v).abs() < EPS, "output volume {v} did not round-trip");
        }
        p.set_output_volume(-1.0);
        assert!((p.get_output_volume() - 0.0).abs() < EPS);
        p.set_output_volume(5.0);
        assert!((p.get_output_volume() - 1.0).abs() < EPS);
    }

    #[test]
    fn test_input_gain_roundtrip_and_clamp() {
        let p = AudioParams::default();
        for g in [-20.0_f32, -12.0, -6.0, 0.0, 6.0, 12.0, 20.0] {
            p.set_input_gain(g);
            assert!((p.get_input_gain() - g).abs() < EPS, "input gain {g} did not round-trip");
        }
        // Clamp to [-20.0, +20.0] dB.
        p.set_input_gain(-100.0);
        assert!((p.get_input_gain() - (-20.0)).abs() < EPS);
        p.set_input_gain(100.0);
        assert!((p.get_input_gain() - 20.0).abs() < EPS);
    }

    #[test]
    fn test_db_level_conversion_boundaries() {
        let p = AudioParams::default();
        // Silence: stored 0 decodes to MIN_DB.
        p.db_level.store(encode_db(MIN_DB), Ordering::Relaxed);
        assert!((p.get_db_level() - MIN_DB).abs() < EPS);
        // Mid-scale.
        p.db_level.store(encode_db(-30.0), Ordering::Relaxed);
        assert!((p.get_db_level() - (-30.0)).abs() < EPS);
        // Unity / full-scale: 0 dB == MAX_DB, the clipping ceiling.
        p.db_level.store(encode_db(MAX_DB), Ordering::Relaxed);
        assert!((p.get_db_level() - MAX_DB).abs() < EPS);
    }

    #[test]
    fn test_level_percentage_mapping() {
        let p = AudioParams::default();
        // -60 dB -> 0%, -30 dB -> 50%, 0 dB -> 100% for both current and peak meters.
        for (db, pct) in [(MIN_DB, 0.0_f32), (-30.0, 50.0), (MAX_DB, 100.0)] {
            p.db_level.store(encode_db(db), Ordering::Relaxed);
            p.peak_db_level.store(encode_db(db), Ordering::Relaxed);
            assert!((p.get_volume_level() - pct).abs() < EPS, "{db} dB should map to {pct}%");
            assert!((p.get_peak_level() - pct).abs() < EPS, "{db} dB peak should map to {pct}%");
        }
    }

    #[test]
    fn test_level_percentage_clamps_out_of_range() {
        let p = AudioParams::default();
        // Above 0 dB (clipping) clamps to 100%.
        p.db_level.store(encode_db(10.0), Ordering::Relaxed);
        p.peak_db_level.store(encode_db(10.0), Ordering::Relaxed);
        assert!((p.get_volume_level() - 100.0).abs() < EPS);
        assert!((p.get_peak_level() - 100.0).abs() < EPS);
        // Below -60 dB clamps to 0%.
        p.db_level.store(encode_db(-80.0), Ordering::Relaxed);
        p.peak_db_level.store(encode_db(-80.0), Ordering::Relaxed);
        assert!((p.get_volume_level() - 0.0).abs() < EPS);
        assert!((p.get_peak_level() - 0.0).abs() < EPS);
    }
}

