//! Mock transport for testing
//!
//! This transport simulates network behavior without requiring a real connection.
//! Useful for unit testing and development without a server.
//!
//! ## Features
//!
//! ### Sine Wave Generation
//!
//! The mock transport can automatically generate audio packets containing a sine wave,
//! which is useful for testing the audio pipeline end-to-end without requiring a real
//! network connection or server.
//!
//! #### Basic Usage
//!
//! ```rust,no_run
//! use jacktrip_web::audio::{MockTransport, Transport};
//!
//! let mut transport = MockTransport::new();
//! 
//! // Enable sine wave generation with default settings (220 Hz, mono, 0.3 amplitude)
//! transport.enable_sine_wave();
//!
//! // Now receive_packet() will return continuous sine wave packets
//! let packet = transport.receive_packet().unwrap();
//! assert!(packet.is_some());
//! ```
//!
//! #### Custom Configuration
//!
//! ```rust,no_run
//! use jacktrip_web::audio::{MockTransport, SineWaveConfig, Transport};
//!
//! let mut transport = MockTransport::new();
//!
//! // Configure a custom sine wave (880 Hz, stereo, 30% amplitude)
//! let config = SineWaveConfig {
//!     frequency: 880.0,    // Hz (A5 note)
//!     amplitude: 0.3,      // 0.0 to 1.0
//!     channels: 2,         // Stereo
//!     sample_rate: 48000,  // Hz
//!     buffer_size: 128,    // Samples per channel
//! };
//!
//! transport.enable_sine_wave_with_config(config);
//!
//! let packet = transport.receive_packet().unwrap().unwrap();
//! assert_eq!(packet.samples.len(), 256); // 128 samples * 2 channels
//! ```
//!
//! #### Integration with Audio Client
//!
//! ```rust,no_run
//! use jacktrip_web::audio::{AudioClient, MockTransport, Transport};
//!
//! let mut transport = MockTransport::new();
//! transport.enable_sine_wave();
//!
//! // Use the transport with an AudioClient for full pipeline testing
//! // The sine wave will flow through jitter buffers, audio processing, etc.
//! ```
//!
//! ### Other Features
//!
//! - **Packet Loss Simulation**: Use `set_packet_loss_rate()` to simulate network packet loss
//! - **Manual Packet Injection**: Use `simulate_receive()` to inject specific test packets
//! - **Queue Inspection**: Use `get_sent_packets()` to verify what was sent

use super::transport::{Transport, TransportState, TransportType, AudioBufferConfig};
use crate::audio::protocol::{AudioPacket, PacketHeader, DEFAULT_BUFFER_SIZE, DEFAULT_SAMPLE_RATE};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

/// Sine wave generator configuration
#[derive(Debug, Clone)]
pub struct SineWaveConfig {
    /// Frequency in Hz (default: 220 Hz - A3 note)
    pub frequency: f32,
    /// Amplitude (0.0 to 1.0, default: 0.3)
    pub amplitude: f32,
    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: u8,
    /// Sample rate in Hz (default: 48000)
    pub sample_rate: u32,
    /// Buffer size (samples per channel per packet, default: 128)
    pub buffer_size: u16,
}

impl Default for SineWaveConfig {
    fn default() -> Self {
        Self {
            frequency: 220.0,  // A3 - warmer, less harsh than A4
            amplitude: 0.1,    // Gentler amplitude
            channels: 1,
            sample_rate: DEFAULT_SAMPLE_RATE,
            buffer_size: DEFAULT_BUFFER_SIZE,
        }
    }
}

/// Sine wave generator state (requires interior mutability)
#[derive(Debug)]
struct SineWaveState {
    enabled: bool,
    config: SineWaveConfig,
    phase: f32,
    sequence: u16,
    timestamp: u64,
}

impl Default for SineWaveState {
    fn default() -> Self {
        Self {
            enabled: false,
            config: SineWaveConfig::default(),
            phase: 0.0,
            sequence: 0,
            timestamp: 0,
        }
    }
}

/// Mock transport for testing
pub struct MockTransport {
    state: TransportState,
    send_queue: Rc<RefCell<VecDeque<Vec<u8>>>>,
    receive_queue: Rc<RefCell<VecDeque<Vec<u8>>>>,
    
    // Callbacks
    #[allow(dead_code)]
    on_state_change: Option<js_sys::Function>,
    
    // Simulate packet loss
    packet_loss_rate: f32,
    
    // Sine wave generator (needs interior mutability for tick loop)
    sine_wave_state: Rc<RefCell<SineWaveState>>,
    
    // Audio buffer configuration for tick processing
    audio_buffers: Option<AudioBufferConfig>,
}

impl MockTransport {
    /// Create a new mock transport
    pub fn new() -> Self {
        Self {
            state: TransportState::Disconnected,
            send_queue: Rc::new(RefCell::new(VecDeque::new())),
            receive_queue: Rc::new(RefCell::new(VecDeque::new())),
            on_state_change: None,
            packet_loss_rate: 0.0,
            sine_wave_state: Rc::new(RefCell::new(SineWaveState::default())),
            audio_buffers: None,
        }
    }

    /// Set simulated packet loss rate (0.0 = no loss, 1.0 = 100% loss)
    pub fn set_packet_loss_rate(&mut self, rate: f32) {
        self.packet_loss_rate = rate.clamp(0.0, 1.0);
    }

    /// Enable sine wave generation with default configuration
    pub fn enable_sine_wave(&mut self) {
        let mut state = self.sine_wave_state.borrow_mut();
        state.enabled = true;
        state.config = SineWaveConfig::default();
        state.phase = 0.0;
        state.sequence = 0;
        state.timestamp = 0;
    }

    /// Enable sine wave generation with custom configuration
    pub fn enable_sine_wave_with_config(&mut self, config: SineWaveConfig) {
        let mut state = self.sine_wave_state.borrow_mut();
        state.enabled = true;
        state.config = config;
        state.phase = 0.0;
        state.sequence = 0;
        state.timestamp = 0;
    }

    /// Disable sine wave generation
    pub fn disable_sine_wave(&mut self) {
        self.sine_wave_state.borrow_mut().enabled = false;
    }

    /// Generate a sine wave packet with harmonics for a more musical tone
    fn generate_sine_wave_packet(state: &mut SineWaveState) -> AudioPacket {
        let config = &state.config;
        let samples_per_channel = config.buffer_size as usize;
        let total_samples = samples_per_channel * config.channels as usize;
        
        // Generate sine wave samples with harmonics for a richer, more pleasant tone
        let mut samples = Vec::with_capacity(total_samples);
        let phase_increment = 2.0 * std::f32::consts::PI * config.frequency / config.sample_rate as f32;
        
        // Harmonic amplitudes (fundamental + overtones create a more musical sound)
        // This creates a warm tone similar to a flute or soft organ
        let harmonics = [
            (1.0, 1.0),    // Fundamental frequency
            (2.0, 0.3),    // 2nd harmonic (octave)
            (3.0, 0.15),   // 3rd harmonic (perfect fifth)
            (4.0, 0.08),   // 4th harmonic (two octaves)
        ];
        
        for i in 0..samples_per_channel {
            let phase = state.phase + i as f32 * phase_increment;
            
            // Sum harmonics to create a richer tone
            let mut sample_value = 0.0;
            for (freq_mult, amplitude_mult) in harmonics.iter() {
                sample_value += (phase * freq_mult).sin() * amplitude_mult;
            }
            
            // Normalize by the sum of harmonic amplitudes to prevent clipping
            let harmonic_sum: f32 = harmonics.iter().map(|(_, amp)| amp).sum();
            sample_value = (sample_value / harmonic_sum) * config.amplitude;
            
            // For stereo, duplicate the sample for both channels
            for _ in 0..config.channels {
                samples.push(sample_value);
            }
        }
        
        // Update phase for next packet (keep it in [0, 2π] range)
        state.phase += samples_per_channel as f32 * phase_increment;
        state.phase %= 2.0 * std::f32::consts::PI;
        
        // Create packet header
        let mut header = PacketHeader::new(state.sequence, state.timestamp);
        header.buffer_size = config.buffer_size;
        header.num_incoming_channels = config.channels;
        header.num_outgoing_channels = config.channels;
        
        // Update sequence and timestamp for next packet
        state.sequence = state.sequence.wrapping_add(1);
        state.timestamp += config.buffer_size as u64;
        
        AudioPacket::new(header, samples)
    }

    /// Process one audio callback tick
    /// 
    /// Called by the session layer when the audio worklet's process() callback runs.
    /// Reads from the ring buffer (and optionally stores for testing) and
    /// generates sine wave packets to push to the jitter buffer.
    fn do_tick(&mut self) {
        // Only process if we have buffers configured
        let buffers = match self.audio_buffers {
            Some(config) => config,
            None => return,
        };

        let samples_needed = (buffers.buffer_size * buffers.channels as usize) as u32;
        
        // Safety: We're in single-threaded WASM, and these pointers are valid
        // for the lifetime of the session
        let ring_buffer = unsafe { &mut *buffers.local_to_network_ptr };
        let jitter_buffer = unsafe { &mut *buffers.network_to_local_ptr };
        
        // Read from ring buffer (simulates sending, stores for testing)
        if ring_buffer.available() >= samples_needed {
            let mut audio_buffer = vec![0.0; samples_needed as usize];
            let _ = ring_buffer.read(&mut audio_buffer);
            // Audio is read but not sent anywhere (mock transport)
        }
        
        // Generate sine wave packet if enabled
        let mut sine_state = self.sine_wave_state.borrow_mut();
        if sine_state.enabled {
            let packet = Self::generate_sine_wave_packet(&mut sine_state);
            
            // Push directly to jitter buffer
            jitter_buffer.push(packet.header.sequence_number, &packet.samples);
        }
    }


    /// Connect the mock transport (simulates instant connection)
    pub fn connect(&mut self) -> Result<(), JsValue> {
        self.state = TransportState::Connected;
        self.notify_state_change();
        Ok(())
    }

    /// Simulate receiving a packet (for testing)
    pub fn simulate_receive(&self, data: Vec<u8>) {
        self.receive_queue.borrow_mut().push_back(data);
    }

    /// Get packets from send queue (for testing/verification)
    pub fn get_sent_packets(&self) -> Vec<Vec<u8>> {
        let mut queue = self.send_queue.borrow_mut();
        let packets: Vec<_> = queue.drain(..).collect();
        packets
    }


    /// Set callback for state changes
    pub fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.on_state_change = Some(callback);
    }

    fn notify_state_change(&self) {
        if let Some(ref callback) = self.on_state_change {
            let state_str = match self.state {
                TransportState::Disconnected => "disconnected",
                TransportState::Connecting => "connecting",
                TransportState::Connected => "connected",
                TransportState::Failed => "failed",
                TransportState::Closed => "closed",
            };
            let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(state_str));
        }
    }
}

impl Transport for MockTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Mock
    }

    fn state(&self) -> TransportState {
        self.state
    }

    fn set_audio_buffers(&mut self, config: AudioBufferConfig) {
        self.audio_buffers = Some(config);
        web_sys::console::debug_1(&format!(
            "✅ Mock: Audio buffers configured ({}ch, {} samples)", 
            config.channels, 
            config.buffer_size
        ).into());
    }

    fn connect(
        &mut self,
        _server: &str,
        _port: u16,
        _use_tls: bool,
        _client_name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), JsValue>> + '_>> {
        // Mock transport doesn't need real connection - just mark as connected
        self.state = TransportState::Connected;
        
        Box::pin(async move {
            Ok(())
        })
    }

    fn tick(&mut self) {
        self.do_tick();
    }

    fn close(&mut self) {
        // Disable streaming on ring buffer
        if let Some(buffers) = self.audio_buffers {
            unsafe {
                (*buffers.local_to_network_ptr).set_streaming(false);
            }
        }
        
        self.state = TransportState::Closed;
        self.send_queue.borrow_mut().clear();
        self.receive_queue.borrow_mut().clear();
        self.notify_state_change();
    }
}

impl Default for MockTransport {
    fn default() -> Self {
        Self::new()
    }
}
