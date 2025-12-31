//! WebRTC Data Channel Transport Layer
//!
//! This module provides a WebRTC-based transport for sending and receiving
//! JackTrip audio packets. It uses unreliable, unordered data channels to
//! mimic UDP behavior for low-latency audio streaming.

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

/// Signaling message types for WebRTC negotiation
#[wasm_bindgen]
#[derive(Debug, Clone)]
pub struct SignalingMessage {
    msg_type: String,
    payload: String,
}

#[wasm_bindgen]
impl SignalingMessage {
    #[wasm_bindgen(getter)]
    pub fn msg_type(&self) -> String {
        self.msg_type.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn payload(&self) -> String {
        self.payload.clone()
    }

    pub fn offer(sdp: &str) -> SignalingMessage {
        SignalingMessage {
            msg_type: "offer".to_string(),
            payload: sdp.to_string(),
        }
    }

    pub fn answer(sdp: &str) -> SignalingMessage {
        SignalingMessage {
            msg_type: "answer".to_string(),
            payload: sdp.to_string(),
        }
    }

    pub fn ice_candidate(candidate: &str) -> SignalingMessage {
        SignalingMessage {
            msg_type: "ice".to_string(),
            payload: candidate.to_string(),
        }
    }
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

    /// Create config optimized for low latency
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

    /// Add an ICE server
    pub fn add_ice_server(&mut self, url: String) {
        self.ice_servers.push(url);
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
    /// JavaScript callbacks
    js_on_signaling: Option<js_sys::Function>,
    js_on_state_change: Option<js_sys::Function>,
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
            js_on_signaling: None,
            js_on_state_change: None,
        })
    }

    /// Set callback for signaling messages (offer/answer/ice)
    pub fn set_on_signaling(&mut self, callback: js_sys::Function) {
        self.js_on_signaling = Some(callback);
    }

    /// Set callback for connection state changes
    pub fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.js_on_state_change = Some(callback);
    }

    /// Get current connection state
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Initialize as the offering peer (caller)
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

        // Notify via callback
        if let Some(ref callback) = self.js_on_signaling {
            let _ = callback.call2(
                &JsValue::NULL,
                &JsValue::from_str("offer"),
                &JsValue::from_str(&offer_sdp),
            );
        }

        Ok(offer_sdp)
    }

    /// Handle an incoming offer (as answering peer)
    pub async fn handle_offer(&mut self, offer_sdp: &str) -> Result<String, JsValue> {
        self.create_peer_connection()?;
        self.state = ConnectionState::Connecting;
        self.notify_state_change();

        let pc = self.peer_connection.as_ref().unwrap();

        // Set remote description
        let desc = RtcSessionDescriptionInit::new(RtcSdpType::Offer);
        desc.set_sdp(offer_sdp);
        JsFuture::from(pc.set_remote_description(&desc)).await?;

        // Create answer
        let answer = JsFuture::from(pc.create_answer()).await?;
        let answer_sdp = Reflect::get(&answer, &"sdp".into())?
            .as_string()
            .ok_or("No SDP in answer")?;

        // Set local description
        let local_desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        local_desc.set_sdp(&answer_sdp);
        JsFuture::from(pc.set_local_description(&local_desc)).await?;

        // Notify via callback
        if let Some(ref callback) = self.js_on_signaling {
            let _ = callback.call2(
                &JsValue::NULL,
                &JsValue::from_str("answer"),
                &JsValue::from_str(&answer_sdp),
            );
        }

        Ok(answer_sdp)
    }

    /// Handle an incoming answer
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

    /// Add an ICE candidate from the remote peer
    pub async fn add_ice_candidate(&mut self, candidate_str: &str) -> Result<(), JsValue> {
        let pc = self
            .peer_connection
            .as_ref()
            .ok_or("No peer connection")?;

        if candidate_str.is_empty() {
            // Empty string signals end of candidates
            return Ok(());
        }

        // Parse the candidate JSON and create init dict
        let candidate_obj = js_sys::JSON::parse(candidate_str)?;
        let candidate_init: RtcIceCandidateInit = candidate_obj.unchecked_into();
        let candidate = RtcIceCandidate::new(&candidate_init)?;
        
        JsFuture::from(
            pc.add_ice_candidate_with_opt_rtc_ice_candidate(Some(&candidate)),
        )
        .await?;

        Ok(())
    }

    /// Send raw bytes over the data channel
    pub fn send_bytes(&self, data: &[u8]) -> Result<(), JsValue> {
        let channel = self.data_channel.as_ref().ok_or("No data channel")?;
        
        if channel.ready_state() != RtcDataChannelState::Open {
            return Err("Data channel not open".into());
        }

        let array = Uint8Array::from(data);
        channel.send_with_array_buffer_view(&array)?;
        
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
    }

    /// Check if connected and ready to send
    pub fn is_connected(&self) -> bool {
        self.state == ConnectionState::Connected
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
        let js_on_signaling = self.js_on_signaling.clone();
        let on_ice_candidate = Closure::wrap(Box::new(
            move |event: web_sys::RtcPeerConnectionIceEvent| {
                if let Some(candidate) = event.candidate() {
                    if let Some(ref callback) = js_on_signaling {
                        // Serialize the candidate
                        let candidate_json = js_sys::JSON::stringify(&candidate).ok();
                        if let Some(json) = candidate_json {
                            let _ = callback.call2(
                                &JsValue::NULL,
                                &JsValue::from_str("ice"),
                                &json,
                            );
                        }
                    }
                }
            },
        ) as Box<dyn FnMut(_)>);
        
        pc.set_onicecandidate(Some(on_ice_candidate.as_ref().unchecked_ref()));
        self.on_ice_candidate_closure = Some(on_ice_candidate);

        // Set up data channel handler for answering peer
        let receive_queue = self.receive_queue.clone();
        let js_on_state_change = self.js_on_state_change.clone();
        
        let on_datachannel = Closure::wrap(Box::new(move |event: RtcDataChannelEvent| {
            let channel = event.channel();
            
            // Set up message handler
            let queue = receive_queue.clone();
            let on_message = Closure::wrap(Box::new(move |msg_event: MessageEvent| {
                if let Ok(buffer) = msg_event.data().dyn_into::<ArrayBuffer>() {
                    let array = Uint8Array::new(&buffer);
                    let mut data = vec![0u8; array.length() as usize];
                    array.copy_to(&mut data);
                    queue.borrow_mut().push_back(data);
                }
            }) as Box<dyn FnMut(_)>);
            
            channel.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
            on_message.forget(); // Leak the closure (it needs to live forever)
            
            // Notify state change
            if let Some(ref callback) = js_on_state_change {
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str("connected"));
            }
        }) as Box<dyn FnMut(_)>);
        
        pc.set_ondatachannel(Some(on_datachannel.as_ref().unchecked_ref()));
        on_datachannel.forget();

        self.peer_connection = Some(pc);
        Ok(())
    }

    fn create_data_channel(&mut self) -> Result<(), JsValue> {
        let pc = self
            .peer_connection
            .as_ref()
            .ok_or("No peer connection")?;

        // Configure for low latency
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
            }
        }) as Box<dyn FnMut(_)>);
        channel.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        self.on_message_closure = Some(on_message);

        // Set up open handler
        let js_on_state_change = self.js_on_state_change.clone();
        let on_open = Closure::wrap(Box::new(move || {
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
}

impl Drop for WebRtcTransport {
    fn drop(&mut self) {
        self.close();
    }
}

/// Helper to create a simple signaling channel using a WebSocket
/// This is a minimal implementation - in production you'd use a proper signaling server
#[wasm_bindgen]
pub struct WebSocketSignaling {
    url: String,
    socket: Option<web_sys::WebSocket>,
    on_offer: Option<js_sys::Function>,
    on_answer: Option<js_sys::Function>,
    on_ice: Option<js_sys::Function>,
}

#[wasm_bindgen]
impl WebSocketSignaling {
    #[wasm_bindgen(constructor)]
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            socket: None,
            on_offer: None,
            on_answer: None,
            on_ice: None,
        }
    }

    pub fn set_on_offer(&mut self, callback: js_sys::Function) {
        self.on_offer = Some(callback);
    }

    pub fn set_on_answer(&mut self, callback: js_sys::Function) {
        self.on_answer = Some(callback);
    }

    pub fn set_on_ice(&mut self, callback: js_sys::Function) {
        self.on_ice = Some(callback);
    }

    pub fn connect(&mut self) -> Result<(), JsValue> {
        let ws = web_sys::WebSocket::new(&self.url)?;
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let on_offer = self.on_offer.clone();
        let on_answer = self.on_answer.clone();
        let on_ice = self.on_ice.clone();

        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Ok(text) = event.data().dyn_into::<js_sys::JsString>() {
                let text: String = text.into();
                // Parse JSON message
                if let Ok(obj) = js_sys::JSON::parse(&text) {
                    let msg_type = Reflect::get(&obj, &"type".into())
                        .ok()
                        .and_then(|v| v.as_string());
                    let payload = Reflect::get(&obj, &"payload".into())
                        .ok()
                        .and_then(|v| v.as_string());

                    if let (Some(t), Some(p)) = (msg_type, payload) {
                        match t.as_str() {
                            "offer" => {
                                if let Some(ref cb) = on_offer {
                                    let _ = cb.call1(&JsValue::NULL, &JsValue::from_str(&p));
                                }
                            }
                            "answer" => {
                                if let Some(ref cb) = on_answer {
                                    let _ = cb.call1(&JsValue::NULL, &JsValue::from_str(&p));
                                }
                            }
                            "ice" => {
                                if let Some(ref cb) = on_ice {
                                    let _ = cb.call1(&JsValue::NULL, &JsValue::from_str(&p));
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        }) as Box<dyn FnMut(_)>);

        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        on_message.forget();

        self.socket = Some(ws);
        Ok(())
    }

    pub fn send(&self, msg_type: &str, payload: &str) -> Result<(), JsValue> {
        let socket = self.socket.as_ref().ok_or("Not connected")?;
        let msg = format!(r#"{{"type":"{}","payload":"{}"}}"#, msg_type, payload.replace('"', "\\\""));
        socket.send_with_str(&msg)?;
        Ok(())
    }

    pub fn close(&mut self) {
        if let Some(ws) = self.socket.take() {
            let _ = ws.close().ok();
        }
    }
}
