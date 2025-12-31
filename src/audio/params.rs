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

