use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use wasm_bindgen::prelude::*;

/// Minimum dB level (silence threshold)
pub const MIN_DB: f32 = -60.0;
/// Maximum dB level (clipping)
pub const MAX_DB: f32 = 0.0;

/// Parameters shared between main thread and audio processing thread
#[wasm_bindgen]
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
    /// Audio processing settings
    pub(crate) auto_gain_control: AtomicBool,
    pub(crate) echo_cancellation: AtomicBool,
    pub(crate) noise_suppression: AtomicBool,
    /// Total number of audio callbacks (process() calls)
    pub(crate) callback_count: AtomicU64,
    /// Number of output channels (1=mono, 2=stereo)
    pub(crate) output_channels: AtomicU32,
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
            auto_gain_control: AtomicBool::new(false),
            echo_cancellation: AtomicBool::new(false),
            noise_suppression: AtomicBool::new(false),
            callback_count: AtomicU64::new(0),
            output_channels: AtomicU32::new(2), // Default to stereo
        }
    }
}

impl AudioParams {
    /// Set output channels (1=mono, 2=stereo)
    pub fn set_output_channels(&self, channels: u32) {
        self.output_channels.store(channels.clamp(1, 8), Ordering::Relaxed);
    }

    /// Get output channels
    pub fn get_output_channels(&self) -> u32 {
        self.output_channels.load(Ordering::Relaxed)
    }
}

#[wasm_bindgen]
impl AudioParams {
    /// Get the current dB level (-60.0 to 0.0)
    #[wasm_bindgen(js_name = getDbLevel)]
    pub fn get_db_level(&self) -> f32 {
        let stored = self.db_level.load(Ordering::Relaxed);
        (stored as f32 / 100.0) + MIN_DB
    }

    /// Get the peak dB level (-60.0 to 0.0)
    #[wasm_bindgen(js_name = getPeakDbLevel)]
    pub fn get_peak_db_level(&self) -> f32 {
        let stored = self.peak_db_level.load(Ordering::Relaxed);
        (stored as f32 / 100.0) + MIN_DB
    }

    /// Get the current volume level as a percentage (0.0 to 100.0)
    /// Maps -60dB to 0% and 0dB to 100%
    #[wasm_bindgen(js_name = getVolumeLevel)]
    pub fn get_volume_level(&self) -> f32 {
        let db = self.get_db_level();
        // Map dB range to percentage
        ((db - MIN_DB) / (MAX_DB - MIN_DB) * 100.0).max(0.0).min(100.0)
    }

    /// Get the peak level as a percentage (0.0 to 100.0)
    #[wasm_bindgen(js_name = getPeakLevel)]
    pub fn get_peak_level(&self) -> f32 {
        let db = self.get_peak_db_level();
        ((db - MIN_DB) / (MAX_DB - MIN_DB) * 100.0).max(0.0).min(100.0)
    }

    /// Set monitor volume (0.0 to 1.0)
    #[wasm_bindgen(js_name = setMonitorVolume)]
    pub fn set_monitor_volume(&self, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        self.monitor_volume.store((clamped * 1000.0) as u32, Ordering::Relaxed);
    }

    /// Get monitor volume (0.0 to 1.0)
    #[wasm_bindgen(js_name = getMonitorVolume)]
    pub fn get_monitor_volume(&self) -> f32 {
        self.monitor_volume.load(Ordering::Relaxed) as f32 / 1000.0
    }

    /// Set auto gain control
    #[wasm_bindgen(js_name = setAutoGainControl)]
    pub fn set_auto_gain_control(&self, enabled: bool) {
        self.auto_gain_control.store(enabled, Ordering::Relaxed);
    }

    /// Get auto gain control setting
    #[wasm_bindgen(js_name = getAutoGainControl)]
    pub fn get_auto_gain_control(&self) -> bool {
        self.auto_gain_control.load(Ordering::Relaxed)
    }

    /// Set echo cancellation
    #[wasm_bindgen(js_name = setEchoCancellation)]
    pub fn set_echo_cancellation(&self, enabled: bool) {
        self.echo_cancellation.store(enabled, Ordering::Relaxed);
    }

    /// Get echo cancellation setting
    #[wasm_bindgen(js_name = getEchoCancellation)]
    pub fn get_echo_cancellation(&self) -> bool {
        self.echo_cancellation.load(Ordering::Relaxed)
    }

    /// Set noise suppression
    #[wasm_bindgen(js_name = setNoiseSuppression)]
    pub fn set_noise_suppression(&self, enabled: bool) {
        self.noise_suppression.store(enabled, Ordering::Relaxed);
    }

    /// Get noise suppression setting
    #[wasm_bindgen(js_name = getNoiseSuppression)]
    pub fn get_noise_suppression(&self) -> bool {
        self.noise_suppression.load(Ordering::Relaxed)
    }

    /// Get the total number of audio callbacks (process() calls)
    #[wasm_bindgen(js_name = getCallbackCount)]
    pub fn get_callback_count(&self) -> u64 {
        self.callback_count.load(Ordering::Relaxed)
    }

    /// Set input gain in dB (-20.0 to +20.0)
    #[wasm_bindgen(js_name = setInputGain)]
    pub fn set_input_gain(&self, gain_db: f32) {
        let clamped = gain_db.clamp(-20.0, 20.0);
        self.input_gain_db.store((clamped * 100.0) as i32, Ordering::Relaxed);
    }

    /// Get input gain in dB
    #[wasm_bindgen(js_name = getInputGain)]
    pub fn get_input_gain(&self) -> f32 {
        self.input_gain_db.load(Ordering::Relaxed) as f32 / 100.0
    }

    /// Set output volume (0.0 to 1.0)
    #[wasm_bindgen(js_name = setOutputVolume)]
    pub fn set_output_volume(&self, volume: f32) {
        let clamped = volume.clamp(0.0, 1.0);
        self.output_volume.store((clamped * 1000.0) as u32, Ordering::Relaxed);
    }

    /// Get output volume (0.0 to 1.0)
    #[wasm_bindgen(js_name = getOutputVolume)]
    pub fn get_output_volume(&self) -> f32 {
        self.output_volume.load(Ordering::Relaxed) as f32 / 1000.0
    }
}

// ==============================================================================
// Pointer-based helper functions for cross-thread access
// ==============================================================================

/// Create a new AudioParams instance for sharing state between threads
#[wasm_bindgen(js_name = createAudioParams)]
pub fn create_audio_params() -> *const AudioParams {
    let params: &'static AudioParams = Box::leak(Box::default());
    params as *const AudioParams
}

/// Get volume level from params pointer (0.0 to 100.0)
#[wasm_bindgen(js_name = getVolumeLevelFromPtr)]
pub fn get_volume_level_from_ptr(ptr: *const AudioParams) -> f32 {
    if ptr.is_null() {
        return 0.0;
    }
    unsafe { (*ptr).get_volume_level() }
}

/// Get dB level from params pointer (-60.0 to 0.0)
#[wasm_bindgen(js_name = getDbLevelFromPtr)]
pub fn get_db_level_from_ptr(ptr: *const AudioParams) -> f32 {
    if ptr.is_null() {
        return -60.0;
    }
    unsafe { (*ptr).get_db_level() }
}

/// Get peak level from params pointer (0.0 to 100.0)
#[wasm_bindgen(js_name = getPeakLevelFromPtr)]
pub fn get_peak_level_from_ptr(ptr: *const AudioParams) -> f32 {
    if ptr.is_null() {
        return 0.0;
    }
    unsafe { (*ptr).get_peak_level() }
}

/// Get peak dB level from params pointer (-60.0 to 0.0)
#[wasm_bindgen(js_name = getPeakDbLevelFromPtr)]
pub fn get_peak_db_level_from_ptr(ptr: *const AudioParams) -> f32 {
    if ptr.is_null() {
        return -60.0;
    }
    unsafe { (*ptr).get_peak_db_level() }
}

/// Set monitor volume from params pointer (0.0 to 1.0)
#[wasm_bindgen(js_name = setMonitorVolumeFromPtr)]
pub fn set_monitor_volume_from_ptr(ptr: *const AudioParams, volume: f32) {
    if !ptr.is_null() {
        unsafe { (*ptr).set_monitor_volume(volume) }
    }
}

/// Get monitor volume from params pointer (0.0 to 1.0)
#[wasm_bindgen(js_name = getMonitorVolumeFromPtr)]
pub fn get_monitor_volume_from_ptr(ptr: *const AudioParams) -> f32 {
    if ptr.is_null() {
        return 0.0;
    }
    unsafe { (*ptr).get_monitor_volume() }
}

/// Set input gain from params pointer (-20.0 to +20.0 dB)
#[wasm_bindgen(js_name = setInputGainFromPtr)]
pub fn set_input_gain_from_ptr(ptr: *const AudioParams, gain_db: f32) {
    if !ptr.is_null() {
        unsafe { (*ptr).set_input_gain(gain_db) }
    }
}

/// Get input gain from params pointer (dB)
#[wasm_bindgen(js_name = getInputGainFromPtr)]
pub fn get_input_gain_from_ptr(ptr: *const AudioParams) -> f32 {
    if ptr.is_null() {
        return 0.0;
    }
    unsafe { (*ptr).get_input_gain() }
}

/// Set output volume from params pointer (0.0 to 1.0)
#[wasm_bindgen(js_name = setOutputVolumeFromPtr)]
pub fn set_output_volume_from_ptr(ptr: *const AudioParams, volume: f32) {
    if !ptr.is_null() {
        unsafe { (*ptr).set_output_volume(volume) }
    }
}

/// Get output volume from params pointer (0.0 to 1.0)
#[wasm_bindgen(js_name = getOutputVolumeFromPtr)]
pub fn get_output_volume_from_ptr(ptr: *const AudioParams) -> f32 {
    if ptr.is_null() {
        return 1.0;
    }
    unsafe { (*ptr).get_output_volume() }
}

/// Get callback count from params pointer
#[wasm_bindgen(js_name = getCallbackCountFromPtr)]
pub fn get_callback_count_from_ptr(ptr: *const AudioParams) -> u64 {
    if ptr.is_null() {
        return 0;
    }
    unsafe { (*ptr).get_callback_count() }
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
// mirroring that encoding via the `enc_db` helper below.
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::thread;

    /// Tolerance for float comparisons. Fixed-point storage resolves to 0.01 dB
    /// (levels/gain) and 0.001 (linear volume), so 1e-4 is comfortably tight.
    const EPS: f32 = 1e-4;

    /// Encode a dB value into the fixed-point representation used by
    /// `db_level` / `peak_db_level`, mirroring `AudioProcessor`.
    fn enc_db(db: f32) -> i32 {
        ((db - MIN_DB) * 100.0) as i32
    }

    #[test]
    fn test_default_values() {
        let p = AudioParams::default();
        // Levels start at silence (stored 0 => MIN_DB) and 0% metering.
        assert!((p.get_db_level() - MIN_DB).abs() < EPS);
        assert!((p.get_peak_db_level() - MIN_DB).abs() < EPS);
        assert!((p.get_volume_level() - 0.0).abs() < EPS);
        assert!((p.get_peak_level() - 0.0).abs() < EPS);
        // Monitoring off, output at full volume, unity input gain.
        assert!((p.get_monitor_volume() - 0.0).abs() < EPS);
        assert!((p.get_output_volume() - 1.0).abs() < EPS);
        assert!((p.get_input_gain() - 0.0).abs() < EPS);
        // Processing toggles default off.
        assert!(!p.get_auto_gain_control());
        assert!(!p.get_echo_cancellation());
        assert!(!p.get_noise_suppression());
        // No callbacks yet; stereo by default.
        assert_eq!(p.get_callback_count(), 0);
        assert_eq!(p.get_output_channels(), 2);
    }

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
    fn test_output_channels_roundtrip_and_clamp() {
        let p = AudioParams::default();
        for c in [1_u32, 2, 4, 8] {
            p.set_output_channels(c);
            assert_eq!(p.get_output_channels(), c);
        }
        // Clamp to [1, 8].
        p.set_output_channels(0);
        assert_eq!(p.get_output_channels(), 1);
        p.set_output_channels(64);
        assert_eq!(p.get_output_channels(), 8);
    }

    #[test]
    fn test_bool_params_roundtrip() {
        let p = AudioParams::default();
        for state in [true, false, true] {
            p.set_auto_gain_control(state);
            p.set_echo_cancellation(state);
            p.set_noise_suppression(state);
            assert_eq!(p.get_auto_gain_control(), state);
            assert_eq!(p.get_echo_cancellation(), state);
            assert_eq!(p.get_noise_suppression(), state);
        }
    }

    #[test]
    fn test_bool_params_are_independent() {
        // Each toggle is a distinct atomic; flipping one must not disturb the others.
        let p = AudioParams::default();
        p.set_auto_gain_control(true);
        assert!(p.get_auto_gain_control());
        assert!(!p.get_echo_cancellation());
        assert!(!p.get_noise_suppression());

        p.set_noise_suppression(true);
        assert!(p.get_auto_gain_control());
        assert!(!p.get_echo_cancellation());
        assert!(p.get_noise_suppression());

        p.set_auto_gain_control(false);
        assert!(!p.get_auto_gain_control());
        assert!(!p.get_echo_cancellation());
        assert!(p.get_noise_suppression());
    }

    #[test]
    fn test_db_level_conversion_boundaries() {
        let p = AudioParams::default();
        // Silence: stored 0 decodes to MIN_DB.
        p.db_level.store(enc_db(MIN_DB), Ordering::Relaxed);
        assert!((p.get_db_level() - MIN_DB).abs() < EPS);
        // Mid-scale.
        p.db_level.store(enc_db(-30.0), Ordering::Relaxed);
        assert!((p.get_db_level() - (-30.0)).abs() < EPS);
        // Unity / full-scale: 0 dB == MAX_DB, the clipping ceiling.
        p.db_level.store(enc_db(MAX_DB), Ordering::Relaxed);
        assert!((p.get_db_level() - MAX_DB).abs() < EPS);
    }

    #[test]
    fn test_peak_db_level_conversion_boundaries() {
        let p = AudioParams::default();
        p.peak_db_level.store(enc_db(MIN_DB), Ordering::Relaxed);
        assert!((p.get_peak_db_level() - MIN_DB).abs() < EPS);
        p.peak_db_level.store(enc_db(-15.0), Ordering::Relaxed);
        assert!((p.get_peak_db_level() - (-15.0)).abs() < EPS);
        p.peak_db_level.store(enc_db(MAX_DB), Ordering::Relaxed);
        assert!((p.get_peak_db_level() - MAX_DB).abs() < EPS);
    }

    #[test]
    fn test_level_percentage_mapping() {
        let p = AudioParams::default();
        // -60 dB -> 0%, -30 dB -> 50%, 0 dB -> 100% for both current and peak meters.
        for (db, pct) in [(MIN_DB, 0.0_f32), (-30.0, 50.0), (MAX_DB, 100.0)] {
            p.db_level.store(enc_db(db), Ordering::Relaxed);
            p.peak_db_level.store(enc_db(db), Ordering::Relaxed);
            assert!((p.get_volume_level() - pct).abs() < EPS, "{db} dB should map to {pct}%");
            assert!((p.get_peak_level() - pct).abs() < EPS, "{db} dB peak should map to {pct}%");
        }
    }

    #[test]
    fn test_level_percentage_clamps_out_of_range() {
        let p = AudioParams::default();
        // Above 0 dB (clipping) clamps to 100%.
        p.db_level.store(enc_db(10.0), Ordering::Relaxed);
        p.peak_db_level.store(enc_db(10.0), Ordering::Relaxed);
        assert!((p.get_volume_level() - 100.0).abs() < EPS);
        assert!((p.get_peak_level() - 100.0).abs() < EPS);
        // Below -60 dB clamps to 0%.
        p.db_level.store(enc_db(-80.0), Ordering::Relaxed);
        p.peak_db_level.store(enc_db(-80.0), Ordering::Relaxed);
        assert!((p.get_volume_level() - 0.0).abs() < EPS);
        assert!((p.get_peak_level() - 0.0).abs() < EPS);
    }

    #[test]
    fn test_peak_level_tracks_successive_tick_updates() {
        // params.rs only stores the peak level + hold counter atomics; the hold/decay
        // *algorithm* lives in `AudioProcessor::update_peak_level`. Here we verify the
        // storage contract that algorithm depends on: successive per-tick writes are
        // observed by the getter, including a held value followed by a decayed value.
        let p = AudioParams::default();

        // Tick 1: a new peak at -10 dB with a full hold counter.
        p.peak_db_level.store(enc_db(-10.0), Ordering::Relaxed);
        p.peak_hold_counter.store(3, Ordering::Relaxed);
        assert!((p.get_peak_db_level() - (-10.0)).abs() < EPS);
        assert_eq!(p.peak_hold_counter.load(Ordering::Relaxed), 3);

        // Ticks 2-4: hold — counter decrements while the peak stays put.
        for expected in [2_u32, 1, 0] {
            let c = p.peak_hold_counter.load(Ordering::Relaxed);
            p.peak_hold_counter.store(c - 1, Ordering::Relaxed);
            assert_eq!(p.peak_hold_counter.load(Ordering::Relaxed), expected);
            assert!((p.get_peak_db_level() - (-10.0)).abs() < EPS);
        }

        // Tick 5: hold expired -> a decayed peak is stored and read back.
        p.peak_db_level.store(enc_db(-12.5), Ordering::Relaxed);
        assert!((p.get_peak_db_level() - (-12.5)).abs() < EPS);
    }

    #[test]
    fn test_callback_count_reflects_underlying_atomic() {
        // `AudioProcessor::process()` bumps callback_count once per audio callback via
        // fetch_add; the getter must surface the running total.
        let p = AudioParams::default();
        assert_eq!(p.get_callback_count(), 0);
        for _ in 0..5 {
            p.callback_count.fetch_add(1, Ordering::Relaxed);
        }
        assert_eq!(p.get_callback_count(), 5);
        p.callback_count.fetch_add(1000, Ordering::Relaxed);
        assert_eq!(p.get_callback_count(), 1005);
    }

    #[test]
    fn test_cross_thread_visibility_writer_reader() {
        // AudioParams is shared (via a raw pointer in production) between the main
        // thread and the audio/worker threads. Values written on one thread must be
        // visible on another. `thread::join` provides the happens-before edge here.
        let params = Arc::new(AudioParams::default());

        let writer = {
            let p = Arc::clone(&params);
            thread::spawn(move || {
                p.set_input_gain(12.0);
                p.set_monitor_volume(0.75);
                p.set_output_volume(0.5);
                p.set_auto_gain_control(true);
                p.set_output_channels(1);
                p.callback_count.fetch_add(7, Ordering::Relaxed);
            })
        };
        writer.join().unwrap();

        assert!((params.get_input_gain() - 12.0).abs() < EPS);
        assert!((params.get_monitor_volume() - 0.75).abs() < EPS);
        assert!((params.get_output_volume() - 0.5).abs() < EPS);
        assert!(params.get_auto_gain_control());
        assert_eq!(params.get_output_channels(), 1);
        assert_eq!(params.get_callback_count(), 7);
    }

    #[test]
    fn test_cross_thread_concurrent_reader_observes_latest() {
        // A reader spinning concurrently with a writer must observe the final value
        // without tearing or deadlock (single writer, relaxed atomics).
        let params = Arc::new(AudioParams::default());
        let target_channels = 6_u32;

        let writer = {
            let p = Arc::clone(&params);
            thread::spawn(move || {
                for c in 1..=target_channels {
                    p.set_output_channels(c);
                    thread::yield_now();
                }
            })
        };

        let reader = {
            let p = Arc::clone(&params);
            thread::spawn(move || {
                // Wait on a generous wall-clock deadline rather than a fixed iteration
                // count. The writer performs only a handful of trivial stores, so under
                // any normal scheduling the sticky target value is observed within
                // microseconds. Bounding by time (not iteration count) avoids a spurious
                // miss under scheduling pressure while still failing fast — instead of
                // hanging forever — if cross-thread visibility ever regresses.
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while std::time::Instant::now() < deadline {
                    if p.get_output_channels() == target_channels {
                        return true;
                    }
                    thread::yield_now();
                }
                false
            })
        };

        writer.join().unwrap();
        let observed = reader.join().unwrap();
        assert_eq!(params.get_output_channels(), target_channels);
        assert!(observed, "reader thread never observed the final value");
    }

    #[test]
    fn test_pointer_helpers_roundtrip() {
        // `create_audio_params` leaks a 'static AudioParams and returns a raw pointer;
        // the *_from_ptr helpers are how JS threads read/write the shared state.
        let ptr = create_audio_params();
        assert!(!ptr.is_null());

        // Defaults observed through the pointer.
        assert!((get_db_level_from_ptr(ptr) - MIN_DB).abs() < EPS);
        assert!((get_peak_db_level_from_ptr(ptr) - MIN_DB).abs() < EPS);
        assert!((get_volume_level_from_ptr(ptr) - 0.0).abs() < EPS);
        assert!((get_peak_level_from_ptr(ptr) - 0.0).abs() < EPS);
        assert!((get_monitor_volume_from_ptr(ptr) - 0.0).abs() < EPS);
        assert!((get_input_gain_from_ptr(ptr) - 0.0).abs() < EPS);
        assert!((get_output_volume_from_ptr(ptr) - 1.0).abs() < EPS);
        assert_eq!(get_callback_count_from_ptr(ptr), 0);

        // Writes through the pointer are observed through the pointer.
        set_monitor_volume_from_ptr(ptr, 0.5);
        set_input_gain_from_ptr(ptr, -6.0);
        set_output_volume_from_ptr(ptr, 0.25);
        assert!((get_monitor_volume_from_ptr(ptr) - 0.5).abs() < EPS);
        assert!((get_input_gain_from_ptr(ptr) - (-6.0)).abs() < EPS);
        assert!((get_output_volume_from_ptr(ptr) - 0.25).abs() < EPS);

        // Reclaim the leaked allocation so the test itself does not leak.
        unsafe {
            drop(Box::from_raw(ptr as *mut AudioParams));
        }
    }

    #[test]
    fn test_pointer_helpers_null_safe() {
        // Every helper must tolerate a null pointer and return its documented fallback.
        let null = std::ptr::null::<AudioParams>();
        assert!((get_db_level_from_ptr(null) - (-60.0)).abs() < EPS);
        assert!((get_peak_db_level_from_ptr(null) - (-60.0)).abs() < EPS);
        assert!((get_volume_level_from_ptr(null) - 0.0).abs() < EPS);
        assert!((get_peak_level_from_ptr(null) - 0.0).abs() < EPS);
        assert!((get_monitor_volume_from_ptr(null) - 0.0).abs() < EPS);
        assert!((get_input_gain_from_ptr(null) - 0.0).abs() < EPS);
        assert!((get_output_volume_from_ptr(null) - 1.0).abs() < EPS);
        assert_eq!(get_callback_count_from_ptr(null), 0);
        // Setters on null are no-ops and must not panic / segfault.
        set_monitor_volume_from_ptr(null, 0.5);
        set_input_gain_from_ptr(null, 3.0);
        set_output_volume_from_ptr(null, 0.5);
    }
}

