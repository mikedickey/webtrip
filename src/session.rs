//! JackTrip Session Manager
//!
//! High-level session management that coordinates all components:
//! - AudioEngine (capture/playback via AudioWorklet)
//! - HubSignaling (WebSocket signaling with hub server)
//! - WebRtcTransport (data channel for audio packets)
//! - RingBuffer (send path: worklet -> network)
//! - JitterBuffer (receive path: network -> worklet)
//!
//! ## Connection Flow
//!
//! ```text
//! 1. connect_to_studio(server_host, port)
//!    └─> Stores audio capture parameters for later
//!    └─> HubSignaling connects via WebSocket
//!    └─> Sends protocol handshake {"protocol":"webrtc",...}
//!
//! 2. WebRtcTransport creates SDP offer
//!    └─> HubSignaling sends offer to server
//!
//! 3. Server responds with SDP answer
//!    └─> WebRtcTransport handles answer
//!    └─> Audio capture starts (after connection established)
//!
//! 4. ICE candidates exchanged
//!    └─> Data channel established
//!
//! 5. Audio streaming begins
//!    └─> tick() loop sends/receives packets
//! ```

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::audio::engine::AudioEngine;
use crate::audio::jitter_buffer::LockFreeJitterBuffer;
use crate::audio::params::AudioParams;
use crate::audio::ring_buffer::RingBuffer;
use crate::audio::signaling::HubSignaling;
use crate::audio::webrtc::{TransportConfig, WebRtcTransport};

/// Default signaling port (same as JackTrip TCP port)
pub const DEFAULT_SIGNALING_PORT: u16 = 4464;

/// Session state
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Disconnected - no audio or network activity
    Idle,
    /// Connecting to hub server (signaling)
    Connecting,
    /// WebRTC negotiation in progress
    Negotiating,
    /// Fully connected and transmitting audio
    Connected,
    /// Error state
    Error,
}

/// Session statistics
#[wasm_bindgen]
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub jitter_depth: u32,
    pub jitter_latency_ms: f32,
    pub send_buffer_available: u32,
    /// Number of ring buffer writes from audio callback
    pub ring_buffer_writes: u64,
    /// Total samples written to ring buffer
    pub ring_buffer_samples_written: u64,
    /// Ring buffer overruns (writes that failed due to full buffer)
    pub ring_buffer_overruns: u64,
    /// Jitter buffer: packets that arrived too late (already played that sequence)
    pub jitter_late_packets: u64,
    /// Jitter buffer: packets lost (slot wasn't ready when needed)
    pub jitter_lost_packets: u64,
    /// Jitter buffer: underruns (depth hit zero)
    pub jitter_underruns: u64,
    /// Jitter buffer: packets successfully played
    pub jitter_played: u64,
    /// Jitter buffer: current target depth (grows on underrun)
    pub jitter_target_depth: u32,
    /// Jitter buffer: whether currently buffering (waiting to reach target)
    pub jitter_buffering: bool,
}

#[wasm_bindgen]
impl SessionStats {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }
}

/// JackTrip Session - coordinates all audio and network components
#[wasm_bindgen]
pub struct JackTripSession {
    // Audio components
    audio_params_ptr: *const AudioParams,
    audio_engine: Option<AudioEngine>,

    // Owned buffers (shared with AudioWorklet via pointers)
    local_to_network_buffer: Box<RingBuffer>,
    network_to_local_buffer: Box<LockFreeJitterBuffer>,

    // Network components
    signaling: Option<HubSignaling>,
    transport: Option<WebRtcTransport>,

    // State
    state: SessionState,
    sequence_number: u16,
    timestamp: u64,

    // Configuration
    sample_rate: u32,
    buffer_size: usize,
    channels: u8,

    // Buffers for network processing
    audio_to_send_buffer: Vec<f32>,

    // Callbacks
    on_state_change: Option<js_sys::Function>,

    // Network loop handle
    interval_handle: Option<i32>,

    // Stats tracking
    packets_sent: u64,
    packets_received: u64,

    // Pending audio capture parameters (to start after connection)
    pending_capture_params: Option<PendingCaptureParams>,
}

/// Parameters for starting audio capture after connection
struct PendingCaptureParams {
    device_id: Option<String>,
    auto_gain_control: bool,
    echo_cancellation: bool,
    noise_suppression: bool,
}

#[wasm_bindgen]
impl JackTripSession {
    /// Create a new session
    #[wasm_bindgen(constructor)]
    pub fn new(audio_params_ptr: *const AudioParams) -> Result<JackTripSession, JsValue> {
        let buffer_size = 128;
        let sample_rate = 48000;
        let channels = 2; // Default to stereo

        // Sync channels to AudioParams so processor knows to duplicate mono to stereo
        if !audio_params_ptr.is_null() {
            unsafe {
                (*audio_params_ptr).set_output_channels(channels as u32);
            }
        }

        // Create owned buffers
        let local_to_network_buffer = Box::new(RingBuffer::new());
        let network_to_local_buffer = Box::new(LockFreeJitterBuffer::new());

        // Configure jitter buffer with samples per packet = buffer_size * channels
        let samples_per_packet = buffer_size * channels as usize;
        network_to_local_buffer.configure(samples_per_packet as u32, 4);

        Ok(JackTripSession {
            audio_params_ptr,
            audio_engine: None,
            local_to_network_buffer,
            network_to_local_buffer,
            signaling: None,
            transport: None,
            state: SessionState::Idle,
            sequence_number: 0,
            timestamp: 0,
            sample_rate,
            buffer_size,
            channels,
            audio_to_send_buffer: vec![0.0; buffer_size * channels as usize],
            on_state_change: None,
            interval_handle: None,
            packets_sent: 0,
            packets_received: 0,
            pending_capture_params: None,
        })
    }

    /// Set callback for state changes
    pub fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.on_state_change = Some(callback);
    }

    /// Get current state
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Get current statistics
    pub fn get_stats(&self) -> SessionStats {
        let jitter_stats = self.network_to_local_buffer.stats();
        SessionStats {
            packets_sent: self.packets_sent,
            packets_received: self.packets_received,
            jitter_depth: self.network_to_local_buffer.depth(),
            jitter_latency_ms: self.network_to_local_buffer.latency_ms(self.sample_rate),
            send_buffer_available: self.local_to_network_buffer.available(),
            ring_buffer_writes: self.local_to_network_buffer.writes(),
            ring_buffer_samples_written: self.local_to_network_buffer.samples_written(),
            ring_buffer_overruns: self.local_to_network_buffer.overruns(),
            jitter_late_packets: jitter_stats.packets_late,
            jitter_lost_packets: jitter_stats.packets_lost,
            jitter_underruns: jitter_stats.underruns,
            jitter_played: jitter_stats.packets_played,
            jitter_target_depth: jitter_stats.target_depth,
            jitter_buffering: jitter_stats.is_buffering,
        }
    }

    /// Set the number of audio channels (1 for mono, 2 for stereo)
    /// Must be called before connecting
    #[wasm_bindgen(js_name = setChannels)]
    pub fn set_channels(&mut self, channels: u8) {
        if channels >= 1 && channels <= 8 {
            self.channels = channels;
            // Update audio buffer size
            self.audio_to_send_buffer = vec![0.0; self.buffer_size * channels as usize];
            // Sync to AudioParams so processor knows to duplicate mono to stereo
            if !self.audio_params_ptr.is_null() {
                unsafe {
                    (*self.audio_params_ptr).set_output_channels(channels as u32);
                }
            }
            // Update jitter buffer configuration with new samples per packet
            let samples_per_packet = self.buffer_size * channels as usize;
            self.network_to_local_buffer.configure(samples_per_packet as u32, 4);
        }
    }

    /// Get the current channel count
    #[wasm_bindgen(js_name = getChannels)]
    pub fn get_channels(&self) -> u8 {
        self.channels
    }

    /// Start audio capture (internal use only)
    async fn start_capture(
        &mut self,
        device_id: Option<String>,
        auto_gain_control: bool,
        echo_cancellation: bool,
        noise_suppression: bool,
    ) -> Result<(), JsValue> {
        // Get raw pointers to owned buffers
        let local_to_network_ptr = &mut *self.local_to_network_buffer as *mut RingBuffer;
        let network_to_local_ptr = &*self.network_to_local_buffer as *const LockFreeJitterBuffer;

        // Create audio engine with network support
        let engine = AudioEngine::create_with_network(
            self.audio_params_ptr,
            local_to_network_ptr,
            network_to_local_ptr,
        )
        .await?;

        self.audio_engine = Some(engine);

        // Start capture
        if let Some(ref mut engine) = self.audio_engine {
            engine
                .start_capture(
                    device_id,
                    auto_gain_control,
                    echo_cancellation,
                    noise_suppression,
                )
                .await?;
        }

        Ok(())
    }

    /// Stop audio capture (internal use only)
    fn stop_capture(&mut self) {
        self.stop_network_loop();

        // IMPORTANT: Stop audio engine before dropping
        // This ensures AudioWorklet stops using buffer pointers
        if let Some(ref mut engine) = self.audio_engine {
            engine.stop_capture();
        }
        self.audio_engine = None;

        self.set_state(SessionState::Idle);
    }


    // ========== Studio Connection Methods ==========

    /// Connect to a JackTrip hub server
    ///
    /// This is the main entry point for connecting to a studio. It:
    /// 1. Establishes a WebSocket connection for signaling
    /// 2. Creates a WebRTC peer connection
    /// 3. Exchanges SDP offer/answer
    /// 4. Establishes the data channel for audio transmission
    /// 5. Starts audio capture (after connection is established)
    ///
    /// Note: Audio capture is deferred until after the server connection
    /// completes to avoid capturing audio before a connection is ready.
    ///
    /// # Arguments
    /// * `server_host` - The hub server hostname (from studio.server_host)
    /// * `port` - The signaling port (default 4464)
    /// * `use_tls` - Whether to use secure WebSocket (wss://)
    /// * `device_id` - Optional input device ID
    /// * `auto_gain_control` - Enable AGC
    /// * `echo_cancellation` - Enable echo cancellation
    /// * `noise_suppression` - Enable noise suppression
    #[wasm_bindgen(js_name = connectToStudio)]
    pub async fn connect_to_studio(
        &mut self,
        server_host: String,
        port: Option<u16>,
        use_tls: Option<bool>,
        device_id: Option<String>,
        auto_gain_control: bool,
        echo_cancellation: bool,
        noise_suppression: bool,
    ) -> Result<(), JsValue> {
        // Store audio capture parameters to start after connection
        self.pending_capture_params = Some(PendingCaptureParams {
            device_id,
            auto_gain_control,
            echo_cancellation,
            noise_suppression,
        });

        let port = port.unwrap_or(DEFAULT_SIGNALING_PORT);
        let use_tls = use_tls.unwrap_or(false);

        self.set_state(SessionState::Connecting);

        // Create signaling client
        let signaling = HubSignaling::new(&server_host, port, use_tls, "jacktrip-web");

        // Create WebRTC transport and SDP offer
        let mut transport = WebRtcTransport::new(Some(TransportConfig::low_latency()))?;
        let offer_sdp = transport.create_offer().await?;

        // Set up ICE candidate callback on the transport
        let session_ptr = self as *mut JackTripSession;
        let ice_callback = Closure::wrap(Box::new(move |candidate: JsValue, sdp_mid: JsValue, sdp_m_line_index: JsValue| {
            let candidate_str = candidate.as_string().unwrap_or_default();
            let sdp_mid_str = sdp_mid.as_string().unwrap_or_else(|| "0".to_string());
            let index = sdp_m_line_index.as_f64().unwrap_or(0.0) as u16;
            
            unsafe {
                if !session_ptr.is_null() {
                    let _ = (*session_ptr).send_ice_candidate(candidate_str, sdp_mid_str, index);
                }
            }
        }) as Box<dyn FnMut(JsValue, JsValue, JsValue)>);
        
        transport.set_on_ice_candidate(ice_callback.as_ref().unchecked_ref::<js_sys::Function>().clone());
        ice_callback.forget();

        // Store components before setting up signaling callbacks
        self.signaling = Some(signaling);
        self.transport = Some(transport);

        // Set up signaling callbacks to handle server responses
        let answer_session_ptr = self as *mut JackTripSession;
        let answer_callback = Closure::wrap(Box::new(move |sdp: JsValue| {
            let sdp_str = sdp.as_string().unwrap_or_default();
            
            wasm_bindgen_futures::spawn_local(async move {
                unsafe {
                    if !answer_session_ptr.is_null() {
                        let _ = (*answer_session_ptr).handle_server_answer(sdp_str).await;
                    }
                }
            });
        }) as Box<dyn FnMut(JsValue)>);

        let ice_session_ptr = self as *mut JackTripSession;
        let ice_server_callback = Closure::wrap(Box::new(move |candidate_json: JsValue| {
            let json_str = candidate_json.as_string().unwrap_or_default();
            
            wasm_bindgen_futures::spawn_local(async move {
                unsafe {
                    if !ice_session_ptr.is_null() {
                        let _ = (*ice_session_ptr).handle_server_ice_candidate(json_str).await;
                    }
                }
            });
        }) as Box<dyn FnMut(JsValue)>);

        // Register the callbacks with signaling
        if let Some(ref mut sig) = self.signaling {
            sig.set_on_answer(answer_callback.as_ref().unchecked_ref::<js_sys::Function>().clone());
            sig.set_on_ice(ice_server_callback.as_ref().unchecked_ref::<js_sys::Function>().clone());
        }
        answer_callback.forget();
        ice_server_callback.forget();

        // Connect signaling WebSocket
        if let Some(ref mut sig) = self.signaling {
            sig.connect()?;
        }

        self.set_state(SessionState::Negotiating);

        // Send the offer
        if let Some(ref sig) = self.signaling {
            sig.send_offer(&offer_sdp)?;
        }

        // Start network loop to handle audio
        self.start_network_loop();

        Ok(())
    }

    /// Handle an SDP answer from the hub server (called from JS signaling callback)
    #[wasm_bindgen(js_name = handleServerAnswer)]
    pub async fn handle_server_answer(&mut self, answer_sdp: String) -> Result<(), JsValue> {
        if let Some(ref mut transport) = self.transport {
            transport.handle_answer(&answer_sdp).await?;
            
            // Start audio capture now that connection is established
            if let Some(params) = self.pending_capture_params.take() {
                web_sys::console::log_1(&"🎙️ Starting audio capture after server connection established".into());
                self.start_capture(
                    params.device_id,
                    params.auto_gain_control,
                    params.echo_cancellation,
                    params.noise_suppression,
                ).await?;
            }
            
            // Transition to Connected when data channel is ready
            self.set_state(SessionState::Connected);
        }
        Ok(())
    }

    /// Handle an ICE candidate from the hub server (called from JS signaling callback)
    #[wasm_bindgen(js_name = handleServerIceCandidate)]
    pub async fn handle_server_ice_candidate(&mut self, candidate_json: String) -> Result<(), JsValue> {
        if let Some(ref mut transport) = self.transport {
            transport.add_ice_candidate(&candidate_json).await?;
        }
        Ok(())
    }

    /// Send a local ICE candidate to the hub server
    #[wasm_bindgen(js_name = sendIceCandidate)]
    pub fn send_ice_candidate(
        &self,
        candidate: String,
        sdp_mid: String,
        sdp_m_line_index: u16,
    ) -> Result<(), JsValue> {
        if let Some(ref sig) = self.signaling {
            sig.send_ice_candidate(&candidate, &sdp_mid, sdp_m_line_index)?;
        }
        Ok(())
    }

    /// Disconnect from the hub server
    pub fn disconnect(&mut self) {
        self.stop_network_loop();

        if let Some(ref mut signaling) = self.signaling {
            signaling.disconnect();
        }
        self.signaling = None;

        if let Some(ref mut transport) = self.transport {
            transport.close();
        }
        self.transport = None;

        // Stop audio capture when disconnecting
        self.stop_capture();

        // Clear any pending capture parameters
        self.pending_capture_params = None;

        // Reset network-to-local buffer
        self.network_to_local_buffer.reset();

        self.sequence_number = 0;
        self.timestamp = 0;
        self.packets_sent = 0;
        self.packets_received = 0;

        self.set_state(SessionState::Idle);
    }

    /// Check if connected to hub server
    #[wasm_bindgen(js_name = isConnected)]
    pub fn is_connected(&self) -> bool {
        self.transport
            .as_ref()
            .map(|t| t.is_connected())
            .unwrap_or(false)
    }

    // ========== Legacy Manual Connection Methods ==========
    // These are kept for testing/debugging but the preferred method is connect_to_studio()

    /// Create an offer (for manual connection flow)
    #[wasm_bindgen(js_name = createOffer)]
    pub async fn create_offer(&mut self) -> Result<String, JsValue> {
        self.set_state(SessionState::Negotiating);

        let mut transport = WebRtcTransport::new(Some(TransportConfig::low_latency()))?;
        let offer = transport.create_offer().await?;

        self.transport = Some(transport);
        Ok(offer)
    }

    /// Handle an incoming answer (for manual connection flow)
    #[wasm_bindgen(js_name = handleAnswer)]
    pub async fn handle_answer(&mut self, answer_sdp: String) -> Result<(), JsValue> {
        if let Some(ref mut transport) = self.transport {
            transport.handle_answer(&answer_sdp).await?;
            self.set_state(SessionState::Connected);
            self.start_network_loop();
        }
        Ok(())
    }

    /// Add an ICE candidate (for manual connection flow)
    #[wasm_bindgen(js_name = addIceCandidate)]
    pub async fn add_ice_candidate(&mut self, candidate: String) -> Result<(), JsValue> {
        if let Some(ref mut transport) = self.transport {
            transport.add_ice_candidate(&candidate).await?;
        }
        Ok(())
    }

    /// Process one network tick (called by interval)
    pub fn tick(&mut self) {
        // Check if transport is connected
        let is_connected = self
            .transport
            .as_ref()
            .map(|t| t.is_connected())
            .unwrap_or(false);

        if !is_connected {
            return;
        }

        // Update state if we just connected
        if self.state == SessionState::Negotiating || self.state == SessionState::Connecting {
            self.set_state(SessionState::Connected);
        }

        // === SEND PATH: local audio → network ===
        // Send all available packets immediately
        // The jitter buffer on the receiving end handles missing packets with zeros
        let samples_needed = (self.buffer_size * self.channels as usize) as u32;
        while self.local_to_network_buffer.available() >= samples_needed {
            if self.local_to_network_buffer.read(&mut self.audio_to_send_buffer) {
                let packet = if self.channels == 1 {
                    crate::audio::protocol::AudioPacket::mono(
                        self.sequence_number,
                        self.timestamp,
                        self.audio_to_send_buffer.clone(),
                    )
                } else {
                    crate::audio::protocol::AudioPacket::stereo(
                        self.sequence_number,
                        self.timestamp,
                        self.audio_to_send_buffer.clone(),
                    )
                };

                // Log first packet details
                if self.packets_sent == 0 {
                    web_sys::console::log_1(&format!(
                        "📤 First packet: seq={}, timestamp={}, samples={}, buffer_size in header={}, session buffer_size={}, channels={}, num_incoming={}, num_outgoing={}", 
                        self.sequence_number,
                        self.timestamp,
                        packet.samples.len(),
                        packet.header.buffer_size,
                        self.buffer_size,
                        self.channels,
                        packet.header.num_incoming_channels,
                        packet.header.num_outgoing_channels
                    ).into());
                }

                if let Some(ref transport) = self.transport {
                    match transport.send_packet(&packet) {
                        Ok(_) => {
                            if self.packets_sent == 0 {
                                web_sys::console::log_1(&"✅ First packet sent successfully!".into());
                            }
                            self.sequence_number = self.sequence_number.wrapping_add(1);
                            self.timestamp += self.buffer_size as u64;
                            self.packets_sent += 1;
                        }
                        Err(e) => {
                            web_sys::console::error_1(&format!("❌ Send failed: {:?}", e).into());
                            break; // Stop if send fails
                        }
                    }
                }
            } else {
                break; // No more data to read
            }
        }
        
        // === RECEIVE PATH: network → local audio ===
        // Collect packets first to avoid borrow issues
        let mut received_packets = Vec::new();
        if let Some(ref transport) = self.transport {
            while transport.has_pending_data() {
                match transport.receive_packet() {
                    Ok(Some(packet)) => {
                        if self.packets_received == 0 {
                            web_sys::console::log_1(&"✅ First packet decoded successfully!".into());
                        }
                        received_packets.push(packet);
                    }
                    Ok(None) => break,
                    Err(e) => {
                        web_sys::console::error_1(&format!("❌ Failed to decode packet: {:?}", e).into());
                        break;
                    }
                }
            }
        }

        // Process collected packets
        for packet in received_packets {
            if self.packets_received == 0 {
                web_sys::console::log_1(&format!(
                    "🎵 Processing first packet: seq={}, samples={}", 
                    packet.header.sequence_number,
                    packet.samples.len()
                ).into());
            }
            self.network_to_local_buffer
                .push(packet.header.sequence_number as u64, &packet.samples);
            self.packets_received += 1;
        }
    }

    // ========== Private Methods ==========

    fn set_state(&mut self, state: SessionState) {
        if self.state != state {
            self.state = state;

            if let Some(ref callback) = self.on_state_change {
                let state_str = match state {
                    SessionState::Idle => "idle",
                    SessionState::Connecting => "connecting",
                    SessionState::Negotiating => "negotiating",
                    SessionState::Connected => "connected",
                    SessionState::Error => "error",
                };
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(state_str));
            }
        }
    }

    fn start_network_loop(&mut self) {
        if self.interval_handle.is_some() {
            return;
        }

        // Enable streaming on the local-to-network buffer
        self.local_to_network_buffer.set_streaming(true);

        // Reset sequence number and timestamp
        self.sequence_number = 0;
        self.timestamp = 0;

        // Reset network-to-local buffer
        self.network_to_local_buffer.reset();

        // Set up interval using web-sys
        let window = web_sys::window().expect("no global window");

        // We need to create a closure that calls tick()
        // Since we can't capture self, we use a raw pointer approach
        // This is safe because the interval is cleared before the session is dropped
        let session_ptr = self as *mut JackTripSession;

        let closure = Closure::wrap(Box::new(move || {
            unsafe {
                if !session_ptr.is_null() {
                    (*session_ptr).tick();
                }
            }
        }) as Box<dyn FnMut()>);

        let handle = window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                closure.as_ref().unchecked_ref(),
                5, // 5ms interval for low latency
            )
            .expect("failed to set interval");

        closure.forget(); // Leak the closure so it stays alive

        self.interval_handle = Some(handle);
    }

    fn stop_network_loop(&mut self) {
        if let Some(handle) = self.interval_handle.take() {
            if let Some(window) = web_sys::window() {
                window.clear_interval_with_handle(handle);
            }
        }

        // Disable streaming on the local-to-network buffer
        self.local_to_network_buffer.set_streaming(false);
    }
}

impl Drop for JackTripSession {
    fn drop(&mut self) {
        // Critical: Stop in correct order to ensure buffer pointers aren't used after drop
        self.stop_network_loop();
        self.disconnect();
        self.stop_capture();
    }
}
