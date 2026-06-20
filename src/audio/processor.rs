use std::sync::atomic::Ordering;

use crate::audio::params::{AudioParams, MAX_DB, MIN_DB, decode_db, decode_volume, encode_db};
use crate::audio::regulator::Regulator;
use crate::audio::ring_buffer::RingBuffer;

/// Peak hold time in process calls (~48kHz / 128 samples = ~375 calls/sec)
/// Hold peak for about 1.5 seconds
const PEAK_HOLD_FRAMES: u32 = 560;
/// Peak decay rate in dB per process call (smooth falloff)
const PEAK_DECAY_RATE: f32 = 0.15;

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

    /// Process audio: calculate volume levels, handle network audio, and generate output
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> bool {
        // Increment callback counter for stats tracking
        self.params.callback_count.fetch_add(1, Ordering::Relaxed);
        
        // Get input gain (dB) and convert to linear
        let input_gain_db = self.params.input_gain_db.load(Ordering::Relaxed) as f32 / 100.0;
        let input_gain_linear = Self::db_to_linear(input_gain_db);
        
        // Ensure buffers are correct size
        if self.gained_buffer.len() != input.len() {
            self.gained_buffer.resize(input.len(), 0.0);
            self.remote_buffer.resize(input.len(), 0.0);
        }

        // Apply input gain to local audio
        for (i, &sample) in input.iter().enumerate() {
            self.gained_buffer[i] = (sample * input_gain_linear).clamp(-1.0, 1.0);
        }

        // Calculate RMS for volume metering
        let sum_squares: f32 = self.gained_buffer.iter().map(|&s| s * s).sum();
        let rms = (sum_squares / input.len() as f32).sqrt();
        let current_db = Self::amplitude_to_db(rms);
        
        // Store dB level
        let db_stored = encode_db(current_db);
        self.params.db_level.store(db_stored, Ordering::Relaxed);

        // Peak level tracking with hold and decay
        self.update_peak_level(current_db, db_stored);

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
    fn update_peak_level(&self, current_db: f32, db_stored: i32) {
        let current_peak_db = decode_db(self.params.peak_db_level.load(Ordering::Relaxed));
        
        if current_db >= current_peak_db {
            // New peak detected
            self.params.peak_db_level.store(db_stored, Ordering::Relaxed);
            self.params.peak_hold_counter.store(PEAK_HOLD_FRAMES, Ordering::Relaxed);
        } else {
            let hold_counter = self.params.peak_hold_counter.load(Ordering::Relaxed);
            if hold_counter > 0 {
                self.params.peak_hold_counter.store(hold_counter - 1, Ordering::Relaxed);
            } else {
                let decayed_stored = encode_db((current_peak_db - PEAK_DECAY_RATE).max(MIN_DB));
                self.params.peak_db_level.store(decayed_stored, Ordering::Relaxed);
            }
        }
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
