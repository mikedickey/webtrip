//! High-Level JackTrip Client API
//!
//! This module provides a simple, high-level interface for JackTrip-style
//! audio streaming over WebRTC. The client handles:
//! - WebRTC connection setup and signaling
//! - Audio packet encoding/decoding
//! - Stream statistics
//!
//! Note: The jitter buffer is separate and managed globally. The worklet
//! reads directly from the jitter buffer for minimal latency.

use crate::audio::protocol::{AudioFormat, AudioPacket, StreamStats};
use crate::audio::webrtc::{ConnectionState, TransportConfig, WebRtcTransport};
use wasm_bindgen::prelude::*;

/// JackTrip client configuration
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct JackTripConfig {
    /// Audio format settings
    pub sample_rate: u32,
    pub channels: u8,
    pub buffer_size: u16,
    /// Network settings
    jitter_buffer_ms: f32,
    /// Whether this client initiates the connection
    is_initiator: bool,
}

#[wasm_bindgen]
impl JackTripConfig {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a low-latency configuration
    pub fn low_latency() -> Self {
        Self {
            sample_rate: 48000,
            channels: 1,
            buffer_size: 64,
            jitter_buffer_ms: 10.0,
            is_initiator: false,
        }
    }

    /// Create a stable configuration (higher latency, better quality)
    pub fn stable() -> Self {
        Self {
            sample_rate: 48000,
            channels: 1,
            buffer_size: 128,
            jitter_buffer_ms: 40.0,
            is_initiator: false,
        }
    }

    /// Set whether this client should initiate the connection
    pub fn set_initiator(&mut self, initiator: bool) {
        self.is_initiator = initiator;
    }

    /// Set jitter buffer size in milliseconds
    pub fn set_jitter_buffer_ms(&mut self, ms: f32) {
        self.jitter_buffer_ms = ms;
    }

    /// Set to mono audio
    pub fn set_mono(&mut self) {
        self.channels = 1;
    }

    /// Set to stereo audio
    pub fn set_stereo(&mut self) {
        self.channels = 2;
    }
}

impl Default for JackTripConfig {
    fn default() -> Self {
        Self {
            sample_rate: 48000,
            channels: 1,
            buffer_size: 128,
            jitter_buffer_ms: 20.0,
            is_initiator: false,
        }
    }
}

/// JackTrip client state
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientState {
    /// Not connected
    Idle,
    /// Establishing WebRTC connection
    Connecting,
    /// Connected, buffering audio
    Buffering,
    /// Fully operational, streaming audio
    Streaming,
    /// Connection error
    Error,
    /// Disconnected
    Disconnected,
}

/// High-level JackTrip client
///
/// This client manages WebRTC connections and audio packet encoding/decoding.
/// It does NOT manage the jitter buffer - that's handled separately by the
/// global LockFreeJitterBuffer which the AudioWorklet reads from directly.
///
/// Usage:
/// ```javascript
/// const client = new AudioClient(JackTripConfig.low_latency());
///
/// // Set up callbacks
/// client.set_on_signaling((type, payload) => {
///     signalingServer.send({ type, payload });
/// });
///
/// client.set_on_state_change((state) => console.log('State:', state));
///
/// // Create offer or handle incoming offer
/// const sdp = await client.connect();  // or client.handle_offer(remoteSdp)
///
/// // In your network loop:
/// client.send_audio(localSamples);
/// const remoteSamples = client.receive_audio();
/// if (remoteSamples) {
///     jitter_buffer_push(jitterPtr, sequence++, remoteSamples);
/// }
/// ```
#[wasm_bindgen]
pub struct AudioClient {
    config: JackTripConfig,
    transport: WebRtcTransport,
    state: ClientState,
    /// Current sequence number for outgoing packets
    sequence_number: u64,
    /// Current timestamp (in samples)
    timestamp: u64,
    /// Stream statistics
    stats: StreamStats,
    /// JavaScript callbacks
    js_on_state_change: Option<js_sys::Function>,
}

#[wasm_bindgen]
impl AudioClient {
    /// Create a new JackTrip client
    #[wasm_bindgen(constructor)]
    pub fn new(config: Option<JackTripConfig>) -> Result<AudioClient, JsValue> {
        let config = config.unwrap_or_default();
        
        let transport_config = TransportConfig::low_latency();
        let transport = WebRtcTransport::new(Some(transport_config))?;
        
        Ok(AudioClient {
            config,
            transport,
            state: ClientState::Idle,
            sequence_number: 0,
            timestamp: 0,
            stats: StreamStats::new(),
            js_on_state_change: None,
        })
    }

    /// Set callback for signaling messages
    /// callback(type: string, payload: string)
    pub fn set_on_signaling(&mut self, callback: js_sys::Function) {
        self.transport.set_on_signaling(callback);
    }

    /// Set callback for state changes
    /// callback(state: string)
    pub fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.js_on_state_change = Some(callback.clone());
        
        // Also update transport state callback
        let state_callback = callback.clone();
        let transport_callback = Closure::wrap(Box::new(move |state: JsValue| {
            let _ = state_callback.call1(&JsValue::NULL, &state);
        }) as Box<dyn FnMut(_)>);
        
        self.transport.set_on_state_change(
            transport_callback.as_ref().unchecked_ref::<js_sys::Function>().clone()
        );
        transport_callback.forget();
    }

    /// Get current client state
    pub fn state(&self) -> ClientState {
        self.state
    }

    /// Get connection state
    pub fn connection_state(&self) -> ConnectionState {
        self.transport.state()
    }

    /// Get stream statistics
    pub fn get_stats(&self) -> StreamStats {
        self.stats.clone()
    }

    /// Get audio format info
    pub fn get_format(&self) -> AudioFormat {
        AudioFormat {
            sample_rate: self.config.sample_rate,
            channels: self.config.channels,
            buffer_size: self.config.buffer_size,
            bit_depth: 32,
        }
    }

    /// Initialize connection as the calling peer
    /// Returns the SDP offer to send to the remote peer
    pub async fn connect(&mut self) -> Result<String, JsValue> {
        self.set_state(ClientState::Connecting);
        let offer = self.transport.create_offer().await?;
        Ok(offer)
    }

    /// Handle an incoming offer from a remote peer
    /// Returns the SDP answer to send back
    pub async fn handle_offer(&mut self, offer_sdp: &str) -> Result<String, JsValue> {
        self.set_state(ClientState::Connecting);
        let answer = self.transport.handle_offer(offer_sdp).await?;
        Ok(answer)
    }

    /// Handle an incoming answer from the remote peer
    pub async fn handle_answer(&mut self, answer_sdp: &str) -> Result<(), JsValue> {
        self.transport.handle_answer(answer_sdp).await?;
        self.set_state(ClientState::Buffering);
        Ok(())
    }

    /// Add an ICE candidate from the remote peer
    pub async fn add_ice_candidate(&mut self, candidate: &str) -> Result<(), JsValue> {
        self.transport.add_ice_candidate(candidate).await
    }

    /// Send local audio samples to the remote peer
    /// samples should be interleaved if stereo
    pub fn send_audio(&mut self, samples: &[f32]) -> Result<(), JsValue> {
        if !self.transport.is_connected() {
            return Err("Not connected".into());
        }

        // Create packet
        let packet = if self.config.channels == 1 {
            AudioPacket::mono(self.sequence_number, self.timestamp, samples.to_vec())
        } else {
            AudioPacket::stereo(self.sequence_number, self.timestamp, samples.to_vec())
        };

        // Send
        self.transport.send_packet(&packet)?;

        // Update counters
        self.sequence_number += 1;
        self.timestamp += samples.len() as u64 / self.config.channels as u64;
        self.stats.packets_sent += 1;

        Ok(())
    }

    /// Receive audio samples from the remote peer
    /// Returns None if no packet is available
    /// 
    /// IMPORTANT: The caller should push received samples to the jitter buffer!
    /// The worklet reads directly from the jitter buffer for minimal latency.
    pub fn receive_audio(&mut self) -> Result<Vec<f32>, JsValue> {
        // Try to get a packet from the transport
        if let Ok(Some(packet)) = self.transport.receive_packet() {
            self.stats.packets_received += 1;
            
            // Update state if we're still buffering
            if self.state == ClientState::Buffering {
                self.set_state(ClientState::Streaming);
            }
            
            return Ok(packet.samples);
        }
        
        // No packet available
        Ok(Vec::new())
    }

    /// Send a heartbeat packet (keeps connection alive)
    pub fn send_heartbeat(&mut self) -> Result<(), JsValue> {
        if !self.transport.is_connected() {
            return Ok(());
        }

        let packet = AudioPacket::heartbeat(self.sequence_number);
        self.transport.send_packet(&packet)?;
        self.sequence_number += 1;
        
        Ok(())
    }

    /// Disconnect and clean up
    pub fn disconnect(&mut self) {
        self.transport.close();
        self.set_state(ClientState::Disconnected);
    }

    /// Check if client is ready to send/receive audio
    pub fn is_streaming(&self) -> bool {
        self.state == ClientState::Streaming
    }

    /// Check if connected (but possibly still buffering)
    pub fn is_connected(&self) -> bool {
        matches!(self.state, ClientState::Buffering | ClientState::Streaming)
    }

    /// Reset statistics
    pub fn reset_stats(&mut self) {
        self.stats = StreamStats::new();
    }

    // Private methods

    fn set_state(&mut self, state: ClientState) {
        if self.state != state {
            self.state = state;
            self.notify_state_change();
        }
    }

    fn notify_state_change(&self) {
        if let Some(ref callback) = self.js_on_state_change {
            let state_str = match self.state {
                ClientState::Idle => "idle",
                ClientState::Connecting => "connecting",
                ClientState::Buffering => "buffering",
                ClientState::Streaming => "streaming",
                ClientState::Error => "error",
                ClientState::Disconnected => "disconnected",
            };
            let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(state_str));
        }
    }
}

impl Drop for AudioClient {
    fn drop(&mut self) {
        self.disconnect();
    }
}

/// Utility function to calculate optimal buffer size for a target latency
#[wasm_bindgen]
pub fn calculate_buffer_size(sample_rate: u32, target_latency_ms: f32) -> u16 {
    let samples = (sample_rate as f32 * target_latency_ms / 1000.0) as u16;
    // Round to power of 2 for efficiency
    let mut size = 32u16;
    while size < samples && size < 4096 {
        size *= 2;
    }
    size
}

/// Get recommended configuration based on network quality
#[wasm_bindgen]
pub fn recommended_config(network_quality: &str) -> JackTripConfig {
    match network_quality {
        "excellent" => JackTripConfig {
            sample_rate: 48000,
            channels: 1,
            buffer_size: 64,
            jitter_buffer_ms: 10.0,
            is_initiator: false,
        },
        "good" => JackTripConfig {
            sample_rate: 48000,
            channels: 1,
            buffer_size: 128,
            jitter_buffer_ms: 20.0,
            is_initiator: false,
        },
        "fair" => JackTripConfig {
            sample_rate: 48000,
            channels: 1,
            buffer_size: 256,
            jitter_buffer_ms: 40.0,
            is_initiator: false,
        },
        _ => JackTripConfig::stable(),
    }
}
