//! WebRTC Data Channel Transport Layer
//!
//! This module provides a WebRTC-based transport for sending and receiving
//! JackTrip audio packets. It uses unreliable, unordered data channels to
//! mimic UDP behavior for low-latency audio streaming.
//!
//! ## Integration with Hub Server
//!
//! This transport is designed to work with JackTrip hub servers that support
//! WebRTC data channels. The signaling is handled by `HubSignaling` which
//! communicates with the hub server over WebSocket.
//!
//! ## Data Channel Configuration
//!
//! For low-latency audio, the data channel is configured for:
//! - Unordered delivery (like UDP)
//! - No retransmissions (maxRetransmits = 0)
//! - Binary mode (ArrayBuffer)

use crate::audio::protocol::AudioPacket;
use crate::audio::signaling::HubSignaling;
use crate::audio::transport::{Transport, TransportState as CommonTransportState, TransportType, AudioBufferConfig};
use js_sys::{Array, ArrayBuffer, Object, Reflect, Uint8Array};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    MessageEvent, RtcConfiguration, RtcDataChannel, RtcDataChannelEvent, RtcDataChannelInit,
    RtcDataChannelState, RtcIceCandidate, RtcIceCandidateInit, RtcPeerConnection,
    RtcSdpType, RtcSessionDescriptionInit,
};

/// Data channel label for audio data
const AUDIO_CHANNEL_LABEL: &str = "jacktrip-audio";

/// Connection state
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not connected
    Disconnected,
    /// Creating offer/answer
    Connecting,
    /// Connected and ready to send/receive
    Connected,
    /// Connection failed
    Failed,
    /// Connection closed
    Closed,
}

/// WebRTC transport configuration
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct TransportConfig {
    /// STUN/TURN servers for NAT traversal
    ice_servers: Vec<String>,
    /// Use unreliable data channel (UDP-like)
    unreliable: bool,
    /// Maximum retransmits (0 for unreliable)
    max_retransmits: u16,
    /// Ordered delivery
    ordered: bool,
}

#[wasm_bindgen]
impl TransportConfig {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create config optimized for low latency (JackTrip style)
    ///
    /// This matches the hub server's data channel configuration:
    /// - Unordered delivery
    /// - No retransmissions
    pub fn low_latency() -> Self {
        Self {
            ice_servers: vec![
                "stun:stun.l.google.com:19302".to_string(),
                "stun:stun1.l.google.com:19302".to_string(),
            ],
            unreliable: true,
            max_retransmits: 0,
            ordered: false,
        }
    }

    /// Add an ICE server (STUN or TURN)
    pub fn add_ice_server(&mut self, url: String) {
        self.ice_servers.push(url);
    }

    /// Set ICE servers from a list
    pub fn set_ice_servers(&mut self, servers: Vec<String>) {
        self.ice_servers = servers;
    }
}

impl Default for TransportConfig {
    fn default() -> Self {
        Self::low_latency()
    }
}

/// WebRTC Data Channel Transport
///
/// Handles peer-to-peer connection establishment and audio data transmission.
/// Designed to work with JackTrip hub servers that support WebRTC.
/// 
/// ## Internal Signaling
/// 
/// This transport manages all WebRTC signaling internally via `connect_to_hub()`:
/// - Creates and manages HubSignaling (WebSocket connection)
/// - Handles SDP offer/answer exchange
/// - Manages ICE candidate exchange
/// - Sets up unreliable, unordered data channel for audio
///
/// ## Internal Tick Loop
/// 
/// WebRTC requires data channel operations to stay on the main JavaScript thread.
/// To work around this, the WebRTC transport manages its own internal tick loop:
/// - Reads from the ring buffer (audio to send)
/// - Writes to the jitter buffer (audio received)
/// - Uses Atomics.waitAsync for efficient wake-up
///
/// Other transports (like WebTransport) don't need this and can run entirely in workers.
#[wasm_bindgen]
pub struct WebRtcTransport {
    config: TransportConfig,
    peer_connection: Option<RtcPeerConnection>,
    data_channel: Option<RtcDataChannel>,
    state: ConnectionState,
    /// Queue for received packets (not used when buffers are provided)
    receive_queue: Rc<RefCell<VecDeque<Vec<u8>>>>,
    /// Hub signaling client (used when connecting to JackTrip hub)
    signaling: Option<HubSignaling>,
    /// Callbacks stored as closures
    #[allow(clippy::type_complexity)]
    on_message_closure: Option<Closure<dyn FnMut(MessageEvent)>>,
    on_channel_open_closure: Option<Closure<dyn FnMut()>>,
    on_channel_close_closure: Option<Closure<dyn FnMut()>>,
    on_ice_candidate_closure: Option<Closure<dyn FnMut(web_sys::RtcPeerConnectionIceEvent)>>,
    on_data_channel_closure: Option<Closure<dyn FnMut(RtcDataChannelEvent)>>,
    /// JavaScript callbacks for external integration (optional)
    js_on_state_change: Option<js_sys::Function>,
    /// Callback for ICE candidates (enables external ICE handling)
    js_on_ice_candidate: Option<js_sys::Function>,
    
    // Audio buffer configuration for tick processing
    audio_buffers: Option<AudioBufferConfig>,
    /// Sequence number for outgoing packets
    sequence_number: u16,
    /// Timestamp for outgoing packets
    timestamp: u64,
    /// Buffers for packet processing (reused to avoid allocations)
    audio_to_send_buffer: Vec<f32>,
    packet_serialize_buffer: Vec<u8>,
}

#[wasm_bindgen]
impl WebRtcTransport {
    /// Create a new WebRTC transport
    #[wasm_bindgen(constructor)]
    pub fn new(config: Option<TransportConfig>) -> Result<WebRtcTransport, JsValue> {
        let config = config.unwrap_or_default();
        
        // Pre-allocate buffers for tick loop (will be resized based on buffer config)
        let buffer_size = 128; // Default, will be updated
        let channels = 2; // Default, will be updated
        let audio_to_send_buffer = vec![0.0; buffer_size * channels];
        let max_packet_bytes = 16 + (buffer_size * channels * 4);
        let packet_serialize_buffer = vec![0u8; max_packet_bytes];

        Ok(WebRtcTransport {
            config,
            peer_connection: None,
            data_channel: None,
            state: ConnectionState::Disconnected,
            receive_queue: Rc::new(RefCell::new(VecDeque::with_capacity(64))),
            signaling: None,
            on_message_closure: None,
            on_channel_open_closure: None,
            on_channel_close_closure: None,
            on_ice_candidate_closure: None,
            on_data_channel_closure: None,
            js_on_state_change: None,
            js_on_ice_candidate: None,
            audio_buffers: None,
            sequence_number: 0,
            timestamp: 0,
            audio_to_send_buffer,
            packet_serialize_buffer,
        })
    }

    /// Set callback for connection state changes (optional, for external monitoring)
    pub fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.js_on_state_change = Some(callback);
    }

    /// Set callback for ICE candidates (optional, for external signaling)
    pub fn set_on_ice_candidate(&mut self, callback: js_sys::Function) {
        self.js_on_ice_candidate = Some(callback);
    }

    /// Process one audio callback tick
    /// 
    /// Called by the session layer when the audio worklet's process() callback runs.
    /// Reads from the ring buffer and writes to the jitter buffer.
    fn do_tick(&mut self) {
        // Only process if we have buffers configured and we're connected
        let buffers = match self.audio_buffers {
            Some(config) => config,
            None => return,
        };

        if !self.is_connected() {
            return;
        }

        let samples_needed = (buffers.buffer_size * buffers.channels as usize) as u32;
        
        // Safety: We're in single-threaded WASM, and these pointers are valid
        // for the lifetime of the session
        let ring_buffer = unsafe { &mut *buffers.local_to_network_ptr };
        let jitter_buffer = unsafe { &mut *buffers.network_to_local_ptr };
        
        // Interleaved send/receive for better latency balance
        loop {
            let mut processed_send = false;
            let mut processed_receive = false;
            
            // Try to process one send packet
            if ring_buffer.available() >= samples_needed {
                if ring_buffer.read(&mut self.audio_to_send_buffer) {
                    // Serialize directly into reusable buffer
                    match crate::audio::protocol::AudioPacket::serialize_samples_into(
                        self.sequence_number,
                        self.timestamp,
                        &self.audio_to_send_buffer,
                        buffers.channels,
                        &mut self.packet_serialize_buffer,
                    ) {
                        Ok(bytes_written) => {
                            if let Some(ref channel) = self.data_channel {
                                if channel.ready_state() == RtcDataChannelState::Open {
                                    let array = Uint8Array::from(&self.packet_serialize_buffer[..bytes_written]);
                                    if let Ok(()) = channel.send_with_array_buffer_view(&array) {
                                        self.sequence_number = self.sequence_number.wrapping_add(1);
                                        self.timestamp += buffers.buffer_size as u64;
                                        processed_send = true;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            web_sys::console::error_1(&format!("❌ WebRTC serialize failed: {:?}", e).into());
                        }
                    }
                }
            }
            
            // Try to process one receive packet
            if let Some(data) = self.receive_queue.borrow_mut().pop_front() {
                match AudioPacket::deserialize(&data) {
                    Ok(packet) => {
                        jitter_buffer.push(packet.header.sequence_number, &packet.samples);
                        processed_receive = true;
                    }
                    Err(e) => {
                        web_sys::console::error_1(&format!("❌ WebRTC failed to deserialize: {:?}", e).into());
                    }
                }
            }
            
            // Exit loop if neither direction had data to process
            if !processed_send && !processed_receive {
                break;
            }
        }
    }


    /// Start connection to JackTrip hub server
    ///
    /// This method handles all WebRTC signaling internally:
    /// - Creates and manages HubSignaling
    /// - Creates SDP offer
    /// - Handles SDP answer from server
    /// - Exchanges ICE candidates
    /// - Sets up data channel
    ///
    /// Returns when the connection is fully established and ready to send/receive.
    async fn connect_to_hub(
        &mut self,
        server: &str,
        port: u16,
        client_name: &str,
    ) -> Result<(), JsValue> {
        web_sys::console::log_1(
            &format!("🔌 Connecting to hub: {}:{} (signaling: wss://.../webrtc)", server, port).into(),
        );
        
        self.state = ConnectionState::Connecting;
        self.notify_state_change();

        // Create a Promise that will resolve when connection is established
        let (promise, resolve, reject) = {
            let mut resolve_func = None;
            let mut reject_func = None;
            let promise = js_sys::Promise::new(&mut |resolve, reject_val| {
                resolve_func = Some(resolve);
                reject_func = Some(reject_val);
            });
            (promise, resolve_func.unwrap(), reject_func.unwrap())
        };

        // Wrap resolve/reject in Rc for sharing with closures
        let resolve_rc = Rc::new(RefCell::new(Some(resolve)));
        let reject_rc = Rc::new(RefCell::new(Some(reject)));
        let resolve_for_answer = resolve_rc.clone();
        let reject_for_answer = reject_rc.clone();

        // Create signaling client wrapped in Rc<RefCell<>> for safe shared ownership
        let mut signaling = HubSignaling::new(server, port, client_name);
        
        // Set up signaling state change callback to propagate WebSocket closure.
        // If the WebSocket closes BEFORE the SDP answer arrives (reject_rc is still Some),
        // reject the connection promise directly so connect_to_hub returns an Err promptly.
        // If it closes AFTER connection is established (reject_rc is None), propagate as
        // "disconnected" for normal post-connection teardown handling.
        //
        // The state string from HubSignaling uses the format "closed:CODE" (e.g. "closed:1006")
        // so we can give specific error messages — notably for code 1006 (TLS cert not trusted).
        let server_for_err = server.to_string();
        let port_for_err = port;
        if let Some(ref callback) = self.js_on_state_change {
            let callback_clone = callback.clone();
            let reject_for_close = reject_rc.clone();
            let signaling_state_cb = Closure::wrap(Box::new(move |state: String| {
                let is_closed = state.starts_with("closed") || state == "error";
                if is_closed {
                    if let Some(reject_fn) = reject_for_close.borrow_mut().take() {
                        // Promise still pending — WebSocket closed before connection established.
                        // Build an actionable error message based on the close code.
                        let close_code: u16 = state
                            .strip_prefix("closed:")
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0);

                        let err_msg = if close_code > 0 {
                            format!(
                                "Could not connect to {}:{} (WebSocket closed with code {}).",
                                server_for_err, port_for_err, close_code
                            )
                        } else {
                            format!(
                                "Could not connect to {}:{}.",
                                server_for_err, port_for_err
                            )
                        };
                        let _ = reject_fn.call1(&JsValue::NULL, &JsValue::from_str(&err_msg));
                    } else {
                        // Promise already resolved — connection was established and is now closing.
                        let _ = callback_clone.call1(&JsValue::NULL, &JsValue::from_str("disconnected"));
                    }
                }
            }) as Box<dyn FnMut(String)>);
            
            signaling.set_on_state_change(signaling_state_cb.as_ref().unchecked_ref::<js_sys::Function>().clone());
            signaling_state_cb.forget(); // Keep callback alive
        }
        
        self.signaling = Some(signaling);

        // Wrap signaling in Rc<RefCell<>> for safe sharing with closures
        let signaling_rc = Rc::new(RefCell::new(self.signaling.take()));
        let signaling_for_ice = signaling_rc.clone();
        
        let ice_js_callback = Closure::wrap(Box::new(move |candidate: JsValue, sdp_mid: JsValue, sdp_m_line_index: JsValue| {
            let candidate_str = candidate.as_string().unwrap_or_default();
            let sdp_mid_str = sdp_mid.as_string().unwrap_or_else(|| "0".to_string());
            let index = sdp_m_line_index.as_f64().unwrap_or(0.0) as u16;
            
            if let Some(ref sig) = *signaling_for_ice.borrow() {
                if let Err(e) = sig.send_ice_candidate(&candidate_str, &sdp_mid_str, index) {
                    web_sys::console::error_1(&format!("❌ Failed to send ICE candidate: {:?}", e).into());
                }
            }
        }) as Box<dyn FnMut(JsValue, JsValue, JsValue)>);
        
        // Store the JS callback for ICE candidates
        self.set_on_ice_candidate(ice_js_callback.as_ref().unchecked_ref::<js_sys::Function>().clone());
        ice_js_callback.forget();

        // For answer and ICE candidate from server, we need access to peer_connection
        // which is already stored in self. Clone the Rc so callbacks can access it.
        let peer_conn_ref = Rc::new(RefCell::new(None::<RtcPeerConnection>));
        let peer_conn_for_answer = peer_conn_ref.clone();
        let peer_conn_for_ice = peer_conn_ref.clone();
        
        // Share connection state between self and closures
        let shared_state = Rc::new(RefCell::new(ConnectionState::Connecting));
        let shared_state_for_answer = shared_state.clone();
        
        // Also need access to state callback
        let state_callback = self.js_on_state_change.clone();
        let state_callback_for_answer = state_callback.clone();
        
        let answer_callback = Closure::wrap(Box::new(move |sdp: JsValue| {
            web_sys::console::debug_1(&"📥 WebRTC: Received answer from server!".into());
            let sdp_str = sdp.as_string().unwrap_or_default();
            let peer_conn = peer_conn_for_answer.clone();
            let state_cb = state_callback_for_answer.clone();
            let state_ref = shared_state_for_answer.clone();
            let resolve = resolve_for_answer.clone();
            let reject = reject_for_answer.clone();
            
            wasm_bindgen_futures::spawn_local(async move {
                web_sys::console::debug_1(&"🔄 WebRTC: Processing answer...".into());
                // Get peer connection from shared ref
                let pc_opt = peer_conn.borrow().clone();
                if let Some(pc) = pc_opt {
                    let desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
                    desc.set_sdp(&sdp_str);
                    
                    match JsFuture::from(pc.set_remote_description(&desc)).await {
                        Ok(_) => {
                            web_sys::console::debug_1(&"✅ WebRTC connection established".into());
                            
                            // Update shared state
                            *state_ref.borrow_mut() = ConnectionState::Connected;
                            
                            // Notify state change
                            if let Some(ref callback) = state_cb {
                                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str("connected"));
                            }
                            
                            // Resolve the promise to signal connection is complete.
                            // Also discard the reject so signaling_state_cb knows we're past
                            // the setup phase — any subsequent WebSocket close should propagate
                            // as "disconnected" rather than rejecting a non-existent promise.
                            if let Some(resolve_fn) = resolve.borrow_mut().take() {
                                let _ = resolve_fn.call0(&JsValue::NULL);
                            }
                            reject.borrow_mut().take(); // Mark connection as established
                        }
                        Err(e) => {
                            web_sys::console::error_1(&format!("❌ Failed to handle answer: {:?}", e).into());
                            
                            // Update shared state
                            *state_ref.borrow_mut() = ConnectionState::Failed;
                            
                            // Notify state change
                            if let Some(ref callback) = state_cb {
                                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str("failed"));
                            }
                            
                            // Reject the promise
                            if let Some(reject_fn) = reject.borrow_mut().take() {
                                let _ = reject_fn.call1(&JsValue::NULL, &e);
                            }
                        }
                    }
                } else {
                    web_sys::console::error_1(&"❌ Peer connection not available in answer callback".into());
                    if let Some(reject_fn) = reject.borrow_mut().take() {
                        let _ = reject_fn.call1(&JsValue::NULL, &"Peer connection not available".into());
                    }
                }
            });
        }) as Box<dyn FnMut(JsValue)>);

        // Set up ICE candidate callback from server
        let ice_callback = Closure::wrap(Box::new(move |candidate_json: JsValue| {
            let json_str = candidate_json.as_string().unwrap_or_default();
            let peer_conn = peer_conn_for_ice.clone();
            
            wasm_bindgen_futures::spawn_local(async move {
                if json_str.is_empty() {
                    return;
                }
                
                // Get peer connection from shared ref
                let pc_opt = peer_conn.borrow().clone();
                if let Some(pc) = pc_opt {
                    match js_sys::JSON::parse(&json_str) {
                        Ok(candidate_obj) => {
                            let candidate_init: RtcIceCandidateInit = candidate_obj.unchecked_into();
                            match RtcIceCandidate::new(&candidate_init) {
                                Ok(candidate) => {
                                    if let Err(e) = JsFuture::from(
                                        pc.add_ice_candidate_with_opt_rtc_ice_candidate(Some(&candidate))
                                    ).await {
                                        web_sys::console::error_1(&format!("❌ Failed to add ICE candidate: {:?}", e).into());
                                    }
                                }
                                Err(e) => {
                                    web_sys::console::error_1(&format!("❌ Failed to create ICE candidate: {:?}", e).into());
                                }
                            }
                        }
                        Err(e) => {
                            web_sys::console::error_1(&format!("❌ Failed to parse ICE candidate JSON: {:?}", e).into());
                        }
                    }
                } else {
                    web_sys::console::error_1(&"❌ Peer connection not available in ICE callback".into());
                }
            });
        }) as Box<dyn FnMut(JsValue)>);

        // Register callbacks with signaling BEFORE connecting
        if let Some(ref mut sig) = *signaling_rc.borrow_mut() {
            sig.set_on_answer(
                answer_callback.as_ref().unchecked_ref::<js_sys::Function>().clone()
            );
            sig.set_on_ice(
                ice_callback.as_ref().unchecked_ref::<js_sys::Function>().clone()
            );
        }
        
        // Prevent closures from being dropped
        answer_callback.forget();
        ice_callback.forget();

        // Create SDP offer (this will set up the peer connection with ICE handling)
        let offer_sdp = self.create_offer().await?;
        
        // Store peer connection in the shared ref for callbacks
        *peer_conn_ref.borrow_mut() = self.peer_connection.clone();

        // NOW connect signaling WebSocket (callbacks are already registered)
        if let Some(ref mut sig) = *signaling_rc.borrow_mut() {
            web_sys::console::debug_1(&"🔌 WebRTC: Starting WebSocket connection...".into());
            sig.connect()?;
            web_sys::console::debug_1(&"📤 WebRTC: Sending offer to hub...".into());
            // Send offer to hub (will be queued if WebSocket not open yet)
            sig.send_offer(&offer_sdp)?;
            web_sys::console::debug_1(&"⏳ WebRTC: Waiting for answer from server...".into());
        }
        
        // Wait for the connection to be established
        // The promise will be resolved by the answer callback when the connection is ready
        // NOTE: Must wait before taking signaling so callbacks can still access it
        web_sys::console::debug_1(&"⏳ WebRTC: Waiting for connection promise to resolve...".into());
        match JsFuture::from(promise).await {
            Err(e) => {
                web_sys::console::error_1(&format!("❌ WebRTC: Connection promise rejected: {:?}", e).into());
                // Clear the state change callback before returning so that
                // WebRtcTransport::drop() -> close() -> notify_state_change("closed") does not
                // fire a spurious "error" transition in the session for a connection that never
                // succeeded. The error surfaces cleanly via the Err return value instead.
                self.js_on_state_change = None;
                return Err(e);
            }
            Ok(_) => {}
        }

        web_sys::console::debug_1(&"✅ WebRTC: Connection promise resolved!".into());
        
        // NOW restore signaling back to self (after connection is complete)
        self.signaling = signaling_rc.borrow_mut().take();
        
        // Update our state now that we're connected
        self.state = ConnectionState::Connected;
        
        // NOTE: Do NOT start event loop here - it must be started after the transport
        // is in its final memory location (after being boxed by the session).
        // The session will call start_streaming() after setup is complete.
        
        Ok(())
    }


    /// Get current connection state
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Create an SDP offer (as the initiating peer) - WebRTC-specific
    ///
    /// For JackTrip hub connections, the client always initiates.
    /// Send the returned SDP to the hub server via HubSignaling.
    pub async fn create_offer(&mut self) -> Result<String, JsValue> {
        self.create_peer_connection()?;
        self.create_data_channel()?;
        self.state = ConnectionState::Connecting;
        self.notify_state_change();

        let pc = self.peer_connection.as_ref().unwrap();

        // Create offer
        let offer = JsFuture::from(pc.create_offer()).await?;
        let offer_sdp = Reflect::get(&offer, &"sdp".into())?
            .as_string()
            .ok_or("No SDP in offer")?;

        // Set local description
        let desc = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
        desc.set_sdp(&offer_sdp);
        JsFuture::from(pc.set_local_description(&desc)).await?;

        Ok(offer_sdp)
    }

    /// Handle an SDP answer from the hub server - WebRTC-specific
    pub async fn handle_answer(&mut self, answer_sdp: &str) -> Result<(), JsValue> {
        let pc = self
            .peer_connection
            .as_ref()
            .ok_or("No peer connection")?;

        let desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        desc.set_sdp(answer_sdp);
        JsFuture::from(pc.set_remote_description(&desc)).await?;

        Ok(())
    }

    /// Add an ICE candidate from the hub server - WebRTC-specific
    pub async fn add_ice_candidate(&mut self, candidate_json: &str) -> Result<(), JsValue> {
        let pc = self
            .peer_connection
            .as_ref()
            .ok_or("No peer connection")?;

        if candidate_json.is_empty() {
            return Ok(());
        }

        // Parse the candidate JSON and create init dict
        let candidate_obj = js_sys::JSON::parse(candidate_json)?;
        let candidate_init: RtcIceCandidateInit = candidate_obj.unchecked_into();
        let candidate = RtcIceCandidate::new(&candidate_init)?;

        JsFuture::from(
            pc.add_ice_candidate_with_opt_rtc_ice_candidate(Some(&candidate)),
        )
        .await?;

        Ok(())
    }

    /// Add an ICE candidate with explicit parameters
    pub async fn add_ice_candidate_explicit(
        &mut self,
        candidate: &str,
        sdp_mid: &str,
        sdp_m_line_index: u16,
    ) -> Result<(), JsValue> {
        let pc = self
            .peer_connection
            .as_ref()
            .ok_or("No peer connection")?;

        if candidate.is_empty() {
            return Ok(());
        }

        let candidate_init = RtcIceCandidateInit::new(candidate);
        candidate_init.set_sdp_mid(Some(sdp_mid));
        candidate_init.set_sdp_m_line_index(Some(sdp_m_line_index));

        let ice_candidate = RtcIceCandidate::new(&candidate_init)?;

        JsFuture::from(
            pc.add_ice_candidate_with_opt_rtc_ice_candidate(Some(&ice_candidate)),
        )
        .await?;

        Ok(())
    }

    /// Send raw bytes over the data channel
    pub fn send_bytes(&self, data: &[u8]) -> Result<(), JsValue> {
        let channel = self.data_channel.as_ref().ok_or("No data channel")?;

        let state = channel.ready_state();
        if state != RtcDataChannelState::Open {
            return Err(format!("Data channel not open (state: {:?})", state).into());
        }

        let array = Uint8Array::from(data);
        channel.send_with_array_buffer_view(&array)
            .map_err(|e| {
                web_sys::console::error_1(&format!("send_with_array_buffer_view failed: {:?}", e).into());
                e
            })?;

        Ok(())
    }

    /// Receive next available packet (non-blocking)
    pub fn receive_bytes(&self) -> Option<Vec<u8>> {
        self.receive_queue.borrow_mut().pop_front()
    }

    /// Check if there are packets available to receive
    pub fn has_pending_data(&self) -> bool {
        !self.receive_queue.borrow().is_empty()
    }

    /// Get number of pending packets
    pub fn pending_count(&self) -> usize {
        self.receive_queue.borrow().len()
    }

    /// Close the connection
    pub fn close(&mut self) {
        // Disable streaming on ring buffer
        if let Some(buffers) = self.audio_buffers {
            unsafe {
                (*buffers.local_to_network_ptr).set_streaming(false);
            }
        }
        
        // Disconnect signaling
        if let Some(mut sig) = self.signaling.take() {
            sig.disconnect();
        }
        
        // Remove event handlers BEFORE dropping closures to prevent "closure invoked after being dropped" errors
        if let Some(ref channel) = self.data_channel {
            channel.set_onmessage(None);
            channel.set_onopen(None);
            channel.set_onclose(None);
            channel.set_onerror(None);
            channel.close();
        }
        
        if let Some(ref pc) = self.peer_connection {
            pc.set_onicecandidate(None);
            pc.set_ondatachannel(None);
            pc.set_oniceconnectionstatechange(None);
            pc.close();
        }
        
        // Now it's safe to drop the closures
        self.on_message_closure = None;
        self.on_channel_open_closure = None;
        self.on_channel_close_closure = None;
        self.on_ice_candidate_closure = None;
        self.on_data_channel_closure = None;
        
        // Finally, take ownership to drop
        self.data_channel = None;
        self.peer_connection = None;
        
        self.state = ConnectionState::Closed;
        self.notify_state_change();
    }

    /// Check if connected and ready to send
    pub fn is_connected(&self) -> bool {
        // Check if data channel is actually open
        if let Some(ref channel) = self.data_channel {
            channel.ready_state() == RtcDataChannelState::Open
        } else {
            false
        }
    }

    // Private methods

    fn create_peer_connection(&mut self) -> Result<(), JsValue> {
        // Build ICE server configuration
        let ice_servers = Array::new();
        for url in &self.config.ice_servers {
            let server = Object::new();
            let urls = Array::new();
            urls.push(&JsValue::from_str(url));
            Reflect::set(&server, &"urls".into(), &urls)?;
            ice_servers.push(&server);
        }

        let rtc_config = RtcConfiguration::new();
        rtc_config.set_ice_servers(&ice_servers);

        let pc = RtcPeerConnection::new_with_configuration(&rtc_config)?;

        // Set up ICE candidate handler
        let js_on_ice_candidate = self.js_on_ice_candidate.clone();
        let on_ice_candidate = Closure::wrap(Box::new(
            move |event: web_sys::RtcPeerConnectionIceEvent| {
                if let Some(candidate) = event.candidate() {
                    let candidate_str = candidate.candidate();
                    let sdp_mid = candidate.sdp_mid().unwrap_or_default();
                    let sdp_m_line_index = candidate.sdp_m_line_index().unwrap_or(0);
                    
                    if let Some(ref callback) = js_on_ice_candidate {
                        let _ = callback.call3(
                            &JsValue::NULL,
                            &JsValue::from_str(&candidate_str),
                            &JsValue::from_str(&sdp_mid),
                            &JsValue::from_f64(sdp_m_line_index as f64),
                        );
                    }
                }
            },
        ) as Box<dyn FnMut(_)>);

        pc.set_onicecandidate(Some(on_ice_candidate.as_ref().unchecked_ref()));
        self.on_ice_candidate_closure = Some(on_ice_candidate);

        // Set up data channel handler for when server creates a data channel
        // (In practice, for JackTrip we create the channel, but this handles the reverse case)
        let receive_queue = self.receive_queue.clone();
        let js_on_state_change = self.js_on_state_change.clone();

        let on_datachannel = Closure::wrap(Box::new(move |event: RtcDataChannelEvent| {
            let channel = event.channel();
            web_sys::console::debug_1(&format!("📥 Server created data channel: {}", channel.label()).into());
            channel.set_binary_type(web_sys::RtcDataChannelType::Arraybuffer);

            // Set up message handler for incoming channel
            let queue = receive_queue.clone();
            let on_message = Closure::wrap(Box::new(move |msg_event: MessageEvent| {
                if let Ok(buffer) = msg_event.data().dyn_into::<ArrayBuffer>() {
                    let array = Uint8Array::new(&buffer);
                    let mut data = vec![0u8; array.length() as usize];
                    array.copy_to(&mut data);
                    web_sys::console::debug_1(&format!("📨 Received {} bytes on server-created channel", data.len()).into());
                    queue.borrow_mut().push_back(data);
                } else {
                    web_sys::console::error_1(&"❌ Received non-ArrayBuffer message".into());
                }
            }) as Box<dyn FnMut(_)>);

            channel.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            on_message.forget();

            // Notify state change
            if let Some(ref callback) = js_on_state_change {
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str("connected"));
            }
        }) as Box<dyn FnMut(_)>);

        pc.set_ondatachannel(Some(on_datachannel.as_ref().unchecked_ref()));
        self.on_data_channel_closure = Some(on_datachannel);

        self.peer_connection = Some(pc);
        Ok(())
    }

    fn create_data_channel(&mut self) -> Result<(), JsValue> {
        let pc = self
            .peer_connection
            .as_ref()
            .ok_or("No peer connection")?;

        // Configure for low latency (matches hub server config)
        let dc_init = RtcDataChannelInit::new();
        dc_init.set_ordered(self.config.ordered);

        if self.config.unreliable {
            dc_init.set_max_retransmits(self.config.max_retransmits);
        }

        let channel = pc.create_data_channel_with_data_channel_dict(AUDIO_CHANNEL_LABEL, &dc_init);
        channel.set_binary_type(web_sys::RtcDataChannelType::Arraybuffer);

        // Set up message handler
        let receive_queue = self.receive_queue.clone();
        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Ok(buffer) = event.data().dyn_into::<ArrayBuffer>() {
                let array = Uint8Array::new(&buffer);
                let mut data = vec![0u8; array.length() as usize];
                array.copy_to(&mut data);
                receive_queue.borrow_mut().push_back(data);
            } else {
                web_sys::console::error_1(&"❌ Received non-ArrayBuffer message on client channel".into());
            }
        }) as Box<dyn FnMut(_)>);
        channel.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        self.on_message_closure = Some(on_message);

        // Set up open handler
        let js_on_state_change = self.js_on_state_change.clone();
        let channel_label = channel.label();
        let on_open = Closure::wrap(Box::new(move || {
            web_sys::console::debug_1(&format!("Data channel '{}' opened!", channel_label).into());
            if let Some(ref callback) = js_on_state_change {
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str("connected"));
            }
        }) as Box<dyn FnMut()>);
        channel.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        self.on_channel_open_closure = Some(on_open);

        // Set up close handler
        let js_on_state_change2 = self.js_on_state_change.clone();
        let on_close = Closure::wrap(Box::new(move || {
            if let Some(ref callback) = js_on_state_change2 {
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str("disconnected"));
            }
        }) as Box<dyn FnMut()>);
        channel.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        self.on_channel_close_closure = Some(on_close);

        self.data_channel = Some(channel);
        Ok(())
    }

    fn notify_state_change(&self) {
        if let Some(ref callback) = self.js_on_state_change {
            let state_str = match self.state {
                ConnectionState::Disconnected => "disconnected",
                ConnectionState::Connecting => "connecting",
                ConnectionState::Connected => "connected",
                ConnectionState::Failed => "failed",
                ConnectionState::Closed => "closed",
            };
            let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(state_str));
        }
    }
}

impl WebRtcTransport {
    /// Mark the connection as connected
    /// Call this after receiving confirmation that the data channel is open
    pub fn set_connected(&mut self) {
        if self.state != ConnectionState::Connected {
            self.state = ConnectionState::Connected;
            self.notify_state_change();
        }
    }

    /// Mark the connection as failed
    pub fn set_failed(&mut self) {
        if self.state != ConnectionState::Failed {
            self.state = ConnectionState::Failed;
            self.notify_state_change();
        }
    }
}

// Implement the Transport trait for WebRtcTransport
impl Transport for WebRtcTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::WebRTC
    }

    fn state(&self) -> CommonTransportState {
        match self.state {
            ConnectionState::Disconnected => CommonTransportState::Disconnected,
            ConnectionState::Connecting => CommonTransportState::Connecting,
            ConnectionState::Connected => CommonTransportState::Connected,
            ConnectionState::Failed => CommonTransportState::Failed,
            ConnectionState::Closed => CommonTransportState::Closed,
        }
    }

    fn set_audio_buffers(&mut self, config: AudioBufferConfig) {
        // Store buffer configuration
        self.audio_buffers = Some(config);
        
        // Resize internal buffers based on configuration
        self.audio_to_send_buffer.resize(config.buffer_size * config.channels as usize, 0.0);
        let max_packet_bytes = 16 + (config.buffer_size * config.channels as usize * 4);
        self.packet_serialize_buffer.resize(max_packet_bytes, 0);
        
        web_sys::console::debug_1(&format!(
            "✅ WebRTC: Audio buffers configured ({}ch, {} samples)", 
            config.channels, 
            config.buffer_size
        ).into());
    }

    fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.js_on_state_change = Some(callback);
    }

    fn connect(
        &mut self,
        server: &str,
        port: u16,
        client_name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), JsValue>> + '_>> {
        // Convert to owned strings for the async block
        let server = server.to_string();
        let client_name = client_name.to_string();

        // Call the existing connect_to_hub implementation
        Box::pin(async move {
            self.connect_to_hub(&server, port, &client_name).await
        })
    }

    fn is_connected(&self) -> bool {
        // Check if data channel is actually open
        if let Some(ref channel) = self.data_channel {
            channel.ready_state() == RtcDataChannelState::Open
        } else {
            false
        }
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
        
        // Disconnect signaling
        if let Some(mut sig) = self.signaling.take() {
            sig.disconnect();
        }
        
        // Remove event handlers BEFORE dropping closures to prevent "closure invoked after being dropped" errors
        if let Some(ref channel) = self.data_channel {
            channel.set_onmessage(None);
            channel.set_onopen(None);
            channel.set_onclose(None);
            channel.set_onerror(None);
            channel.close();
        }
        
        if let Some(ref pc) = self.peer_connection {
            pc.set_onicecandidate(None);
            pc.set_ondatachannel(None);
            pc.set_oniceconnectionstatechange(None);
            pc.close();
        }
        
        // Now it's safe to drop the closures
        self.on_message_closure = None;
        self.on_channel_open_closure = None;
        self.on_channel_close_closure = None;
        self.on_ice_candidate_closure = None;
        self.on_data_channel_closure = None;
        
        // Finally, take ownership to drop
        self.data_channel = None;
        self.peer_connection = None;
        
        self.state = ConnectionState::Closed;
        self.notify_state_change();
    }
}

impl Drop for WebRtcTransport {
    fn drop(&mut self) {
        self.close();
    }
}
