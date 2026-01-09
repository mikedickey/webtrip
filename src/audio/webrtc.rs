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
use js_sys::{Array, ArrayBuffer, Object, Reflect, Uint8Array};
use std::cell::RefCell;
use std::collections::VecDeque;
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
#[wasm_bindgen]
pub struct WebRtcTransport {
    config: TransportConfig,
    peer_connection: Option<RtcPeerConnection>,
    data_channel: Option<RtcDataChannel>,
    state: ConnectionState,
    /// Queue for received packets
    receive_queue: Rc<RefCell<VecDeque<Vec<u8>>>>,
    /// Callbacks stored as closures
    #[allow(clippy::type_complexity)]
    on_message_closure: Option<Closure<dyn FnMut(MessageEvent)>>,
    on_channel_open_closure: Option<Closure<dyn FnMut()>>,
    on_channel_close_closure: Option<Closure<dyn FnMut()>>,
    on_ice_candidate_closure: Option<Closure<dyn FnMut(web_sys::RtcPeerConnectionIceEvent)>>,
    on_data_channel_closure: Option<Closure<dyn FnMut(RtcDataChannelEvent)>>,
    /// JavaScript callbacks for signaling integration
    js_on_ice_candidate: Option<js_sys::Function>,
    js_on_state_change: Option<js_sys::Function>,
    /// Callback for when data is received (enables event-driven receive processing)
    js_on_data_received: Rc<RefCell<Option<js_sys::Function>>>,
}

#[wasm_bindgen]
impl WebRtcTransport {
    /// Create a new WebRTC transport
    #[wasm_bindgen(constructor)]
    pub fn new(config: Option<TransportConfig>) -> Result<WebRtcTransport, JsValue> {
        let config = config.unwrap_or_default();

        Ok(WebRtcTransport {
            config,
            peer_connection: None,
            data_channel: None,
            state: ConnectionState::Disconnected,
            receive_queue: Rc::new(RefCell::new(VecDeque::with_capacity(64))),
            on_message_closure: None,
            on_channel_open_closure: None,
            on_channel_close_closure: None,
            on_ice_candidate_closure: None,
            on_data_channel_closure: None,
            js_on_ice_candidate: None,
            js_on_state_change: None,
            js_on_data_received: Rc::new(RefCell::new(None)),
        })
    }

    /// Set callback for ICE candidates
    ///
    /// The callback receives (candidate: string, sdpMid: string, sdpMLineIndex: number)
    pub fn set_on_ice_candidate(&mut self, callback: js_sys::Function) {
        self.js_on_ice_candidate = Some(callback);
    }

    /// Set callback for connection state changes
    pub fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.js_on_state_change = Some(callback);
    }

    /// Set callback for when data is received
    /// 
    /// This enables event-driven receive processing - the callback is invoked
    /// immediately when a packet arrives, allowing the session to process it
    /// without waiting for the next polling interval.
    pub fn set_on_data_received(&mut self, callback: js_sys::Function) {
        *self.js_on_data_received.borrow_mut() = Some(callback);
    }

    /// Get current connection state
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Create an SDP offer (as the initiating peer)
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

    /// Handle an SDP answer from the hub server
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

    /// Add an ICE candidate from the hub server
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
        if let Some(channel) = self.data_channel.take() {
            channel.close();
        }
        if let Some(pc) = self.peer_connection.take() {
            pc.close();
        }
        self.state = ConnectionState::Closed;
        self.notify_state_change();

        // Clean up closures
        self.on_message_closure = None;
        self.on_channel_open_closure = None;
        self.on_channel_close_closure = None;
        self.on_ice_candidate_closure = None;
        self.on_data_channel_closure = None;
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
        let js_on_data_received_for_server = self.js_on_data_received.clone();

        let on_datachannel = Closure::wrap(Box::new(move |event: RtcDataChannelEvent| {
            let channel = event.channel();
            web_sys::console::log_1(&format!("📥 Server created data channel: {}", channel.label()).into());
            channel.set_binary_type(web_sys::RtcDataChannelType::Arraybuffer);

            // Set up message handler for incoming channel
            let queue = receive_queue.clone();
            let on_data_callback = js_on_data_received_for_server.clone();
            let on_message = Closure::wrap(Box::new(move |msg_event: MessageEvent| {
                if let Ok(buffer) = msg_event.data().dyn_into::<ArrayBuffer>() {
                    let array = Uint8Array::new(&buffer);
                    let mut data = vec![0u8; array.length() as usize];
                    array.copy_to(&mut data);
                    web_sys::console::log_1(&format!("📨 Received {} bytes on server-created channel", data.len()).into());
                    queue.borrow_mut().push_back(data);
                    
                    // Notify that data is available for immediate processing
                    if let Some(ref callback) = *on_data_callback.borrow() {
                        let _ = callback.call0(&JsValue::NULL);
                    }
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
        let on_data_received = self.js_on_data_received.clone();
        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Ok(buffer) = event.data().dyn_into::<ArrayBuffer>() {
                let array = Uint8Array::new(&buffer);
                let mut data = vec![0u8; array.length() as usize];
                array.copy_to(&mut data);
                receive_queue.borrow_mut().push_back(data);
                
                // Notify that data is available for immediate processing
                if let Some(ref callback) = *on_data_received.borrow() {
                    let _ = callback.call0(&JsValue::NULL);
                }
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
            web_sys::console::log_1(&format!("Data channel '{}' opened!", channel_label).into());
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
    /// Send an audio packet (internal use, not exposed to JS)
    pub fn send_packet(&self, packet: &AudioPacket) -> Result<(), JsValue> {
        let data = packet
            .serialize()
            .map_err(|e| JsValue::from_str(&format!("{}", e)))?;
        self.send_bytes(&data)
    }

    /// Receive and decode an audio packet (internal use, not exposed to JS)
    pub fn receive_packet(&self) -> Result<Option<AudioPacket>, JsValue> {
        if let Some(data) = self.receive_bytes() {
            let packet = AudioPacket::deserialize(&data)
                .map_err(|e| JsValue::from_str(&format!("{}", e)))?;
            Ok(Some(packet))
        } else {
            Ok(None)
        }
    }

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

impl Drop for WebRtcTransport {
    fn drop(&mut self) {
        self.close();
    }
}
