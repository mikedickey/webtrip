use std::sync::atomic::Ordering;

use crate::audio_params::{AudioParams, MAX_DB, MIN_DB};

/// Peak hold time in process calls (~48kHz / 128 samples = ~375 calls/sec)
/// Hold peak for about 1.5 seconds
const PEAK_HOLD_FRAMES: u32 = 560;
/// Peak decay rate in dB per process call (smooth falloff)
const PEAK_DECAY_RATE: f32 = 0.15;

/// Core audio processor for real-time audio processing
/// Handles volume metering, gain control, and audio monitoring
pub struct AudioProcessor {
    params: &'static AudioParams,
}

impl AudioProcessor {
    pub fn new(params: &'static AudioParams) -> Self {
        Self { params }
    }

    /// Convert linear amplitude to decibels
    fn amplitude_to_db(amplitude: f32) -> f32 {
        if amplitude <= 0.0 {
            MIN_DB
        } else {
            (20.0 * amplitude.log10()).max(MIN_DB).min(MAX_DB)
        }
    }

    /// Convert dB to linear gain multiplier
    fn db_to_linear(db: f32) -> f32 {
        10.0_f32.powf(db / 20.0)
    }

    /// Process audio: calculate volume levels from input and optionally copy to output
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> bool {
        // Get input gain (dB) and convert to linear
        let input_gain_db = self.params.input_gain_db.load(Ordering::Relaxed) as f32 / 100.0;
        let input_gain_linear = Self::db_to_linear(input_gain_db);
        
        // Apply input gain and calculate RMS (Root Mean Square) for volume level
        let sum_squares: f32 = input.iter()
            .map(|&s| {
                let gained = s * input_gain_linear;
                gained * gained
            })
            .sum();
        let rms = (sum_squares / input.len() as f32).sqrt();
        
        // Convert RMS to decibels
        let current_db = Self::amplitude_to_db(rms);
        
        // Store dB level as fixed-point: multiply by 100 for 2 decimal places
        // Offset by MIN_DB to make it positive for storage
        let db_stored = ((current_db - MIN_DB) * 100.0) as i32;
        self.params.db_level.store(db_stored, Ordering::Relaxed);

        // Peak level tracking with hold and decay
        let current_peak_stored = self.params.peak_db_level.load(Ordering::Relaxed);
        let current_peak_db = (current_peak_stored as f32 / 100.0) + MIN_DB;
        
        if current_db >= current_peak_db {
            // New peak detected - update and reset hold counter
            self.params.peak_db_level.store(db_stored, Ordering::Relaxed);
            self.params.peak_hold_counter.store(PEAK_HOLD_FRAMES, Ordering::Relaxed);
        } else {
            // Check hold counter
            let hold_counter = self.params.peak_hold_counter.load(Ordering::Relaxed);
            if hold_counter > 0 {
                // Still holding
                self.params.peak_hold_counter.store(hold_counter - 1, Ordering::Relaxed);
            } else {
                // Decay the peak
                let decayed_db = current_peak_db - PEAK_DECAY_RATE;
                let decayed_stored = ((decayed_db.max(MIN_DB) - MIN_DB) * 100.0) as i32;
                self.params.peak_db_level.store(decayed_stored, Ordering::Relaxed);
            }
        }

        // Handle monitoring: apply input gain, output volume, and monitor volume
        let monitor_volume = self.params.monitor_volume.load(Ordering::Relaxed) as f32 / 1000.0;
        
        if monitor_volume > 0.0 {
            // Get output volume (0.0 to 1.0)
            let output_volume = self.params.output_volume.load(Ordering::Relaxed) as f32 / 1000.0;
            let total_gain = input_gain_linear * output_volume * monitor_volume;
            
            let len = input.len().min(output.len());
            for i in 0..len {
                output[i] = (input[i] * total_gain).clamp(-1.0, 1.0);
            }
        } else {
            // Fill output with silence
            output.fill(0.0);
        }

        true
    }
}

