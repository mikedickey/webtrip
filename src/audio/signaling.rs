//! JackTrip Hub Server Signaling Protocol
//!
//! This module implements the WebRTC signaling protocol for connecting to
//! JackTrip hub servers. The signaling happens over WebSocket on the same
//! port as traditional TCP signaling (default 4464).
//!
//! ## Protocol Flow
//!
//! ```text
//! Client                                    Hub Server
//!   │                                           │
//!   │  1. WebSocket Connect (wss://host:4464/webrtc?name=ClientName)
//!   │───────────────────────────────────────────▶
//!   │                                           │
//!   │  2. {"protocol":"webrtc", "version":1}    │
//!   │───────────────────────────────────────────▶
//!   │                                           │
//!   │  3. {"type":"offer", "sdp":"..."}         │
//!   │───────────────────────────────────────────▶
//!   │                                           │  Create PeerConnection
//!   │  4. {"type":"answer", "sdp":"..."}        │  Allocate mixer slot
//!   │◀───────────────────────────────────────────
//!   │                                           │
//!   │  5. Exchange ICE candidates               │
//!   │◀──────────────────────────────────────────▶
//!   │                                           │
//!   │  6. Data Channel Open                     │
//!   │═══════════════════════════════════════════│
//!   │                                           │
//!   │  7. Audio packets over Data Channel       │
//!   │◀═════════════════════════════════════════▶│
//! ```
//!
//! ## Message Types
//!
//! All messages are JSON-encoded:
//!
//! - **Protocol Detection**: `{"protocol": "webrtc", "version": 1}` (client name is in WebSocket URL)
//! - **SDP Offer**: `{"type": "offer", "sdp": "v=0\r\n..."}`
//! - **SDP Answer**: `{"type": "answer", "sdp": "v=0\r\n..."}`
//! - **ICE Candidate**: `{"type": "ice", "candidate": "...", "sdpMid": "...", "sdpMLineIndex": 0}`
//! - **Hangup**: `{"type": "hangup"}`
//! - **Error**: `{"type": "error", "message": "..."}`

use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

/// Signaling protocol version
pub const PROTOCOL_VERSION: u32 = 1;

/// Signaling message types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalingMessageType {
    /// Protocol detection/handshake
    Protocol,
    /// SDP Offer
    Offer,
    /// SDP Answer
    Answer,
    /// ICE Candidate
    Ice,
    /// Hangup/disconnect
    Hangup,
    /// Error from server
    Error,
}

/// A signaling message
#[derive(Debug, Clone)]
pub struct SignalingMessage {
    pub msg_type: SignalingMessageType,
    /// SDP content for Offer/Answer
    pub sdp: Option<String>,
    /// ICE candidate string
    pub candidate: Option<String>,
    /// SDP media ID for ICE
    pub sdp_mid: Option<String>,
    /// SDP media line index for ICE
    pub sdp_m_line_index: Option<u16>,
    /// Error message
    pub error: Option<String>,
}

impl SignalingMessage {
    /// Create a protocol detection message
    pub fn protocol() -> Self {
        Self {
            msg_type: SignalingMessageType::Protocol,
            sdp: None,
            candidate: None,
            sdp_mid: None,
            sdp_m_line_index: None,
            error: None,
        }
    }

    /// Create an offer message
    pub fn offer(sdp: &str) -> Self {
        Self {
            msg_type: SignalingMessageType::Offer,
            sdp: Some(sdp.to_string()),
            candidate: None,
            sdp_mid: None,
            sdp_m_line_index: None,
            error: None,
        }
    }

    /// Create an answer message
    pub fn answer(sdp: &str) -> Self {
        Self {
            msg_type: SignalingMessageType::Answer,
            sdp: Some(sdp.to_string()),
            candidate: None,
            sdp_mid: None,
            sdp_m_line_index: None,
            error: None,
        }
    }

    /// Create an ICE candidate message
    pub fn ice(candidate: &str, sdp_mid: &str, sdp_m_line_index: u16) -> Self {
        Self {
            msg_type: SignalingMessageType::Ice,
            sdp: None,
            candidate: Some(candidate.to_string()),
            sdp_mid: Some(sdp_mid.to_string()),
            sdp_m_line_index: Some(sdp_m_line_index),
            error: None,
        }
    }

    /// Create a hangup message
    pub fn hangup() -> Self {
        Self {
            msg_type: SignalingMessageType::Hangup,
            sdp: None,
            candidate: None,
            sdp_mid: None,
            sdp_m_line_index: None,
            error: None,
        }
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> String {
        match &self.msg_type {
            SignalingMessageType::Protocol => {
                format!(
                    r#"{{"protocol":"webrtc","version":{}}}"#,
                    PROTOCOL_VERSION
                )
            }
            SignalingMessageType::Offer => {
                let sdp = self.sdp.as_deref().unwrap_or("");
                format!(r#"{{"type":"offer","sdp":"{}"}}"#, escape_json_string(sdp))
            }
            SignalingMessageType::Answer => {
                let sdp = self.sdp.as_deref().unwrap_or("");
                format!(r#"{{"type":"answer","sdp":"{}"}}"#, escape_json_string(sdp))
            }
            SignalingMessageType::Ice => {
                let candidate = self.candidate.as_deref().unwrap_or("");
                let sdp_mid = self.sdp_mid.as_deref().unwrap_or("data");
                let index = self.sdp_m_line_index.unwrap_or(0);
                format!(
                    r#"{{"type":"ice","candidate":"{}","sdpMid":"{}","sdpMLineIndex":{}}}"#,
                    escape_json_string(candidate),
                    escape_json_string(sdp_mid),
                    index
                )
            }
            SignalingMessageType::Hangup => r#"{"type":"hangup"}"#.to_string(),
            SignalingMessageType::Error => {
                let msg = self.error.as_deref().unwrap_or("Unknown error");
                format!(r#"{{"type":"error","message":"{}"}}"#, escape_json_string(msg))
            }
        }
    }

    /// Parse from JSON
    pub fn from_json(json: &str) -> Result<Self, String> {
        // Simple JSON parsing without serde for wasm size
        // This is a minimal parser for the specific message format

        let json = json.trim();

        // Check for protocol detection response
        if json.contains(r#""protocol""#) {
            return Ok(Self {
                msg_type: SignalingMessageType::Protocol,
                sdp: None,
                candidate: None,
                sdp_mid: None,
                sdp_m_line_index: None,
                error: None,
            });
        }

        // Extract type field
        let msg_type = extract_string_field(json, "type")?;

        match msg_type.as_str() {
            "offer" => {
                let sdp = extract_string_field(json, "sdp")?;
                Ok(Self::offer(&sdp))
            }
            "answer" => {
                let sdp = extract_string_field(json, "sdp")?;
                Ok(Self::answer(&sdp))
            }
            "ice" => {
                let candidate = extract_string_field(json, "candidate")?;
                let sdp_mid = extract_string_field(json, "sdpMid").unwrap_or_else(|_| "data".to_string());
                let sdp_m_line_index = extract_number_field(json, "sdpMLineIndex").unwrap_or(0) as u16;
                Ok(Self::ice(&candidate, &sdp_mid, sdp_m_line_index))
            }
            "hangup" => Ok(Self::hangup()),
            "error" => {
                let message = extract_string_field(json, "message").unwrap_or_else(|_| "Unknown error".to_string());
                Ok(Self {
                    msg_type: SignalingMessageType::Error,
                    sdp: None,
                    candidate: None,
                    sdp_mid: None,
                    sdp_m_line_index: None,
                    error: Some(message),
                })
            }
            _ => Err(format!("Unknown message type: {}", msg_type)),
        }
    }
}

/// Escape a string for JSON encoding
fn escape_json_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => result.push_str(r#"\""#),
            '\\' => result.push_str(r#"\\"#),
            '\n' => result.push_str(r#"\n"#),
            '\r' => result.push_str(r#"\r"#),
            '\t' => result.push_str(r#"\t"#),
            c if c.is_control() => {
                result.push_str(&format!(r#"\u{:04x}"#, c as u32));
            }
            c => result.push(c),
        }
    }
    result
}

/// Extract a string field from JSON
fn extract_string_field(json: &str, field: &str) -> Result<String, String> {
    let pattern = format!(r#""{}":"#, field);
    let start = json.find(&pattern).ok_or_else(|| format!("Field '{}' not found", field))?;
    let value_start = start + pattern.len();
    let rest = &json[value_start..];

    if !rest.starts_with('"') {
        return Err(format!("Field '{}' is not a string", field));
    }

    let rest = &rest[1..]; // Skip opening quote
    let mut result = String::new();
    let mut chars = rest.chars().peekable();
    let mut escaped = false;

    while let Some(c) = chars.next() {
        if escaped {
            match c {
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                'u' => {
                    // Unicode escape
                    let hex: String = chars.by_ref().take(4).collect();
                    if let Ok(code) = u32::from_str_radix(&hex, 16) {
                        if let Some(ch) = char::from_u32(code) {
                            result.push(ch);
                        }
                    }
                }
                _ => {
                    result.push('\\');
                    result.push(c);
                }
            }
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == '"' {
            break;
        } else {
            result.push(c);
        }
    }

    Ok(result)
}

/// Extract a number field from JSON
fn extract_number_field(json: &str, field: &str) -> Result<i64, String> {
    let pattern = format!(r#""{}":"#, field);
    let start = json.find(&pattern).ok_or_else(|| format!("Field '{}' not found", field))?;
    let value_start = start + pattern.len();
    let rest = &json[value_start..].trim_start();

    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-').unwrap_or(rest.len());
    let num_str = &rest[..end];

    num_str.parse().map_err(|_| format!("Invalid number for field '{}'", field))
}

/// Connection state for hub signaling
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubConnectionState {
    /// Not connected
    Disconnected,
    /// WebSocket connecting
    Connecting,
    /// WebSocket connected, sending protocol handshake
    Handshaking,
    /// Protocol accepted, exchanging SDP
    Negotiating,
    /// WebRTC connected
    Connected,
    /// Connection failed
    Failed,
    /// Connection closed gracefully
    Closed,
}

/// Hub server signaling client
///
/// Handles WebSocket connection and signaling with a JackTrip hub server.
#[wasm_bindgen]
pub struct HubSignaling {
    /// Server URL (always `wss://` for hub WebRTC signaling)
    server_url: String,
    /// WebSocket connection
    socket: Option<WebSocket>,
    /// Connection state
    state: HubConnectionState,
    /// Whether the WebSocket is ready to send messages
    is_ready: Rc<RefCell<bool>>,
    /// Queue for outgoing messages (sent when WebSocket is ready)
    outgoing_queue: Rc<RefCell<Vec<String>>>,
    /// Callbacks stored as closures
    on_message_closure: Option<Closure<dyn FnMut(MessageEvent)>>,
    on_open_closure: Option<Closure<dyn FnMut()>>,
    on_close_closure: Option<Closure<dyn FnMut(CloseEvent)>>,
    on_error_closure: Option<Closure<dyn FnMut(Event)>>,
    /// Message queue for received messages
    message_queue: Rc<RefCell<Vec<SignalingMessage>>>,
    /// JavaScript callbacks
    js_on_answer: Option<js_sys::Function>,
    js_on_ice: Option<js_sys::Function>,
    js_on_error: Option<js_sys::Function>,
    js_on_state_change: Option<js_sys::Function>,
}

#[wasm_bindgen]
impl HubSignaling {
    /// Create a new hub signaling client
    ///
    /// WebRTC hub signaling always uses a TLS WebSocket (`wss://`) on the `/webrtc` path.
    ///
    /// # Arguments
    /// * `server_host` - The hub server hostname (from studio.server_host)
    /// * `port` - The signaling port (default 4464)
    /// * `client_name` - Client identifier (sent as URL query parameter), empty string for anonymous
    #[wasm_bindgen(constructor)]
    pub fn new(server_host: &str, port: u16, client_name: &str) -> Self {
        // Only include name parameter if client_name is not empty
        let server_url = if client_name.is_empty() {
            format!("wss://{}:{}/webrtc", server_host, port)
        } else {
            let encoded_name = js_sys::encode_uri_component(client_name);
            format!("wss://{}:{}/webrtc?name={}", server_host, port, encoded_name)
        };

        Self {
            server_url,
            socket: None,
            state: HubConnectionState::Disconnected,
            is_ready: Rc::new(RefCell::new(false)),
            outgoing_queue: Rc::new(RefCell::new(Vec::new())),
            on_message_closure: None,
            on_open_closure: None,
            on_close_closure: None,
            on_error_closure: None,
            message_queue: Rc::new(RefCell::new(Vec::new())),
            js_on_answer: None,
            js_on_ice: None,
            js_on_error: None,
            js_on_state_change: None,
        }
    }

    /// Create from a full WebSocket URL (`ws://` is upgraded to `wss://` for hub signaling)
    pub fn from_url(url: &str, _client_name: &str) -> Self {
        let server_url = if url.starts_with("ws://") {
            format!("wss://{}", &url[5..])
        } else {
            url.to_string()
        };
        Self {
            server_url,
            socket: None,
            state: HubConnectionState::Disconnected,
            is_ready: Rc::new(RefCell::new(false)),
            outgoing_queue: Rc::new(RefCell::new(Vec::new())),
            on_message_closure: None,
            on_open_closure: None,
            on_close_closure: None,
            on_error_closure: None,
            message_queue: Rc::new(RefCell::new(Vec::new())),
            js_on_answer: None,
            js_on_ice: None,
            js_on_error: None,
            js_on_state_change: None,
        }
    }

    /// Set callback for SDP answer received
    pub fn set_on_answer(&mut self, callback: js_sys::Function) {
        self.js_on_answer = Some(callback);
    }

    /// Set callback for ICE candidate received
    pub fn set_on_ice(&mut self, callback: js_sys::Function) {
        self.js_on_ice = Some(callback);
    }

    /// Set callback for errors
    pub fn set_on_error(&mut self, callback: js_sys::Function) {
        self.js_on_error = Some(callback);
    }

    /// Set callback for state changes
    pub fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.js_on_state_change = Some(callback);
    }

    /// Get current connection state
    pub fn state(&self) -> HubConnectionState {
        self.state
    }

    /// Connect to the hub server
    pub fn connect(&mut self) -> Result<(), JsValue> {
        if self.socket.is_some() {
            return Err("Already connected".into());
        }

        self.set_state(HubConnectionState::Connecting);
        let ws = WebSocket::new(&self.server_url)?;

        // Set up event handlers
        let message_queue = self.message_queue.clone();
        let js_on_answer = self.js_on_answer.clone();
        let js_on_ice = self.js_on_ice.clone();
        let js_on_error = self.js_on_error.clone();

        let on_message = Closure::wrap(Box::new(move |event: MessageEvent| {
            if let Ok(text) = event.data().dyn_into::<js_sys::JsString>() {
                let text: String = text.into();
                web_sys::console::debug_1(&format!("📨 Signaling received message: {}", text).into());
                
                if let Ok(msg) = SignalingMessage::from_json(&text) {
                    web_sys::console::debug_1(&format!("✅ Parsed message type: {:?}", msg.msg_type).into());
                    match msg.msg_type {
                        SignalingMessageType::Answer => {
                            web_sys::console::debug_1(&"📥 Signaling: Answer message detected".into());
                            if let Some(ref callback) = js_on_answer {
                                if let Some(ref sdp) = msg.sdp {
                                    web_sys::console::debug_1(&"✅ Signaling: Calling answer callback".into());
                                    let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(sdp));
                                }
                            } else {
                                web_sys::console::warn_1(&"⚠️ Answer callback not set".into());
                            }
                        }
                        SignalingMessageType::Ice => {
                            if let Some(ref callback) = js_on_ice {
                                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(&text));
                            } else {
                                web_sys::console::warn_1(&"⚠️ ICE callback not set".into());
                            }
                        }
                        SignalingMessageType::Error => {
                            let error = msg.error.as_deref().unwrap_or("Unknown error");
                            web_sys::console::error_1(&format!("❌ Signaling error: {}", error).into());
                            if let Some(ref callback) = js_on_error {
                                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(error));
                            }
                        }
                        _ => {
                            message_queue.borrow_mut().push(msg);
                        }
                    }
                } else {
                    web_sys::console::warn_1(&"⚠️ Failed to parse signaling message".into());
                }
            }
        }) as Box<dyn FnMut(_)>);

        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        self.on_message_closure = Some(on_message);

        // On open - send protocol handshake and flush queued messages
        let socket_clone = ws.clone();
        let is_ready = self.is_ready.clone();
        let outgoing_queue = self.outgoing_queue.clone();
        let on_open = Closure::wrap(Box::new(move || {
            web_sys::console::debug_1(&"✅ Signaling: WebSocket connected!".into());

            // Send protocol handshake first (client name already in URL)
            let msg = SignalingMessage::protocol();
            let json = msg.to_json();
            web_sys::console::debug_1(&format!("📤 Signaling: Sending protocol handshake: {}", json).into());
            let _ = socket_clone.send_with_str(&json);

            // Mark as ready
            *is_ready.borrow_mut() = true;

            // Flush any queued outgoing messages
            let queued: Vec<String> = outgoing_queue.borrow_mut().drain(..).collect();
            web_sys::console::debug_1(&format!("📤 Signaling: Flushing {} queued messages", queued.len()).into());
            for msg in queued {
                web_sys::console::debug_1(&format!("📤 Signaling: Sending queued: {}", msg).into());
                let _ = socket_clone.send_with_str(&msg);
            }
        }) as Box<dyn FnMut()>);

        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        self.on_open_closure = Some(on_open);

        // On close
        let js_on_state_change = self.js_on_state_change.clone();
        let on_close = Closure::wrap(Box::new(move |event: CloseEvent| {
            let code = event.code();
            let reason = event.reason();
            let was_clean = event.was_clean();
            web_sys::console::warn_1(&format!("⚠️ Signaling: WebSocket closed (code={}, reason='{}', clean={})", code, reason, was_clean).into());
            // Include the close code in the state string so callers can give specific error messages.
            // Format: "closed:CODE" (e.g. "closed:1006")
            if let Some(ref callback) = js_on_state_change {
                let state_with_code = format!("closed:{}", code);
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(&state_with_code));
            }
        }) as Box<dyn FnMut(_)>);

        ws.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        self.on_close_closure = Some(on_close);

        // On error
        let js_on_error_clone = self.js_on_error.clone();
        let on_error = Closure::wrap(Box::new(move |event: Event| {
            web_sys::console::error_1(&format!("❌ Signaling: WebSocket error: {:?}", event).into());
            if let Some(ref callback) = js_on_error_clone {
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str("WebSocket error"));
            }
        }) as Box<dyn FnMut(_)>);

        ws.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        self.on_error_closure = Some(on_error);

        self.socket = Some(ws);
        self.set_state(HubConnectionState::Handshaking);

        Ok(())
    }

    /// Send an SDP offer to the server
    /// If the WebSocket isn't ready yet, the message is queued
    pub fn send_offer(&self, sdp: &str) -> Result<(), JsValue> {
        let msg = SignalingMessage::offer(sdp);
        let json = msg.to_json();
        
        if *self.is_ready.borrow() {
            let socket = self.socket.as_ref().ok_or("Not connected")?;
            socket.send_with_str(&json)?;
        } else {
            self.outgoing_queue.borrow_mut().push(json);
        }
        Ok(())
    }

    /// Send an ICE candidate to the server
    /// If the WebSocket isn't ready yet, the message is queued
    pub fn send_ice_candidate(&self, candidate: &str, sdp_mid: &str, sdp_m_line_index: u16) -> Result<(), JsValue> {
        let msg = SignalingMessage::ice(candidate, sdp_mid, sdp_m_line_index);
        let json = msg.to_json();
        
        if *self.is_ready.borrow() {
            let socket = self.socket.as_ref().ok_or("Not connected")?;
            socket.send_with_str(&json)?;
        } else {
            self.outgoing_queue.borrow_mut().push(json);
        }
        Ok(())
    }

    /// Send a hangup message and disconnect
    pub fn disconnect(&mut self) {
        if let Some(ref socket) = self.socket {
            let msg = SignalingMessage::hangup();
            let _ = socket.send_with_str(&msg.to_json());
            let _ = socket.close();
        }
        self.cleanup();
        self.set_state(HubConnectionState::Closed);
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        matches!(
            self.state,
            HubConnectionState::Handshaking | HubConnectionState::Negotiating | HubConnectionState::Connected
        )
    }

    // Private methods

    fn set_state(&mut self, state: HubConnectionState) {
        if self.state != state {
            self.state = state;
            if let Some(ref callback) = self.js_on_state_change {
                let state_str = match state {
                    HubConnectionState::Disconnected => "disconnected",
                    HubConnectionState::Connecting => "connecting",
                    HubConnectionState::Handshaking => "handshaking",
                    HubConnectionState::Negotiating => "negotiating",
                    HubConnectionState::Connected => "connected",
                    HubConnectionState::Failed => "failed",
                    HubConnectionState::Closed => "closed",
                };
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(state_str));
            }
        }
    }

    fn cleanup(&mut self) {
        // Remove event handlers BEFORE dropping closures to prevent "closure invoked after being dropped" errors
        if let Some(ref socket) = self.socket {
            socket.set_onmessage(None);
            socket.set_onopen(None);
            socket.set_onclose(None);
            socket.set_onerror(None);
        }
        
        // Now safe to drop closures
        self.on_message_closure = None;
        self.on_open_closure = None;
        self.on_close_closure = None;
        self.on_error_closure = None;
        
        // Finally drop the socket
        self.socket = None;
        *self.is_ready.borrow_mut() = false;
        self.outgoing_queue.borrow_mut().clear();
        self.message_queue.borrow_mut().clear();
    }
}

impl Drop for HubSignaling {
    fn drop(&mut self) {
        self.disconnect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_message() {
        let msg = SignalingMessage::protocol();
        let json = msg.to_json();
        assert!(json.contains(r#""protocol":"webrtc""#));
        assert!(json.contains(r#""version":1"#));
        // Client name is now in the WebSocket URL, not in the protocol message
        assert!(!json.contains(r#""client_name""#));
    }

    #[test]
    fn test_offer_message_roundtrip() {
        let original = SignalingMessage::offer("v=0\r\no=- 123 2 IN IP4 127.0.0.1\r\n");
        let json = original.to_json();
        let parsed = SignalingMessage::from_json(&json).unwrap();
        assert_eq!(parsed.msg_type, SignalingMessageType::Offer);
        assert_eq!(parsed.sdp.as_deref(), original.sdp.as_deref());
    }

    #[test]
    fn test_ice_message_roundtrip() {
        let original = SignalingMessage::ice("candidate:1 1 UDP 2130706431 192.168.1.1 12345 typ host", "data", 0);
        let json = original.to_json();
        let parsed = SignalingMessage::from_json(&json).unwrap();
        assert_eq!(parsed.msg_type, SignalingMessageType::Ice);
        assert_eq!(parsed.candidate.as_deref(), original.candidate.as_deref());
        assert_eq!(parsed.sdp_mid.as_deref(), Some("data"));
        assert_eq!(parsed.sdp_m_line_index, Some(0));
    }

    #[test]
    fn test_json_escape() {
        let msg = SignalingMessage::offer("test\nwith\nnewlines");
        let json = msg.to_json();
        assert!(json.contains(r#"\n"#));
        assert!(!json.contains('\n'));
    }

    // --- Round-trip tests for remaining message types ---

    #[test]
    fn test_answer_message_roundtrip() {
        let sdp = "v=0\r\no=- 456 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\n";
        let original = SignalingMessage::answer(sdp);
        let json = original.to_json();
        let parsed = SignalingMessage::from_json(&json).unwrap();
        assert_eq!(parsed.msg_type, SignalingMessageType::Answer);
        assert_eq!(parsed.sdp.as_deref(), original.sdp.as_deref());
    }

    #[test]
    fn test_hangup_message_roundtrip() {
        let original = SignalingMessage::hangup();
        let json = original.to_json();
        let parsed = SignalingMessage::from_json(&json).unwrap();
        assert_eq!(parsed.msg_type, SignalingMessageType::Hangup);
        assert!(parsed.sdp.is_none());
        assert!(parsed.candidate.is_none());
        assert!(parsed.error.is_none());
    }

    #[test]
    fn test_error_message_roundtrip() {
        let error_text = "Room is full";
        let original = SignalingMessage {
            msg_type: SignalingMessageType::Error,
            sdp: None,
            candidate: None,
            sdp_mid: None,
            sdp_m_line_index: None,
            error: Some(error_text.to_string()),
        };
        let json = original.to_json();
        let parsed = SignalingMessage::from_json(&json).unwrap();
        assert_eq!(parsed.msg_type, SignalingMessageType::Error);
        assert_eq!(parsed.error.as_deref(), Some(error_text));
    }

    // --- escape_json_string edge cases ---

    #[test]
    fn test_escape_double_quotes() {
        let escaped = super::escape_json_string(r#"say "hello""#);
        assert_eq!(escaped, r#"say \"hello\""#);
    }

    #[test]
    fn test_escape_backslash() {
        // Each backslash in input should become two backslashes in output
        let escaped = super::escape_json_string("path\\to\\file");
        assert_eq!(escaped, "path\\\\to\\\\file");
    }

    #[test]
    fn test_escape_control_characters() {
        // \n, \r, \t should each become their two-character JSON sequences
        let escaped = super::escape_json_string("line1\nline2\r\ntab\there");
        assert_eq!(escaped, r"line1\nline2\r\ntab\there");
    }

    #[test]
    fn test_escape_misc_control_char() {
        // Other control characters (e.g. BEL 0x07) should become \uXXXX
        let escaped = super::escape_json_string("\x07");
        assert_eq!(escaped, r"\u0007");
    }

    #[test]
    fn test_escape_unicode_multibyte() {
        // Multi-byte UTF-8 code points are not control chars and pass through unchanged
        let input = "こんにちは";
        let escaped = super::escape_json_string(input);
        assert_eq!(escaped, input);
    }

    // --- extract_string_field with realistic SDP payloads ---

    #[test]
    fn test_extract_string_field_realistic_sdp() {
        // SDP contains colons and commas; the round-trip must preserve the full value
        let sdp = "v=0\r\no=- 123 2 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\na=group:BUNDLE data\r\nm=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n";
        let json = SignalingMessage::offer(sdp).to_json();
        let extracted = super::extract_string_field(&json, "sdp").unwrap();
        assert_eq!(extracted, sdp);
    }

    #[test]
    fn test_extract_string_field_with_escaped_chars() {
        // Embedded double-quotes and backslashes must survive the round-trip
        let sdp = r#"field with "quotes" and \backslash"#;
        let json = SignalingMessage::offer(sdp).to_json();
        let extracted = super::extract_string_field(&json, "sdp").unwrap();
        assert_eq!(extracted, sdp);
    }

    #[test]
    fn test_extract_string_field_missing() {
        let json = r#"{"type":"offer"}"#;
        let result = super::extract_string_field(json, "sdp");
        assert!(result.is_err());
    }

    // --- extract_number_field boundary values ---

    #[test]
    fn test_extract_number_field_zero() {
        let json = r#"{"type":"ice","candidate":"c","sdpMid":"data","sdpMLineIndex":0}"#;
        let val = super::extract_number_field(json, "sdpMLineIndex").unwrap();
        assert_eq!(val, 0);
    }

    #[test]
    fn test_extract_number_field_large() {
        let json = r#"{"type":"ice","candidate":"c","sdpMid":"data","sdpMLineIndex":65535}"#;
        let val = super::extract_number_field(json, "sdpMLineIndex").unwrap();
        assert_eq!(val, 65535);
    }

    #[test]
    fn test_extract_number_field_missing() {
        let json = r#"{"type":"ice","candidate":"c","sdpMid":"data"}"#;
        let result = super::extract_number_field(json, "sdpMLineIndex");
        assert!(result.is_err());
    }

    // --- from_json error handling for malformed input ---

    #[test]
    fn test_from_json_missing_type_field() {
        let result = SignalingMessage::from_json(r#"{"sdp":"v=0"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_json_unknown_type() {
        let result = SignalingMessage::from_json(r#"{"type":"unknown_type"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_json_missing_sdp_in_offer() {
        let result = SignalingMessage::from_json(r#"{"type":"offer"}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_json_non_string_type_field() {
        // type field is a number, not a string — extract_string_field should return Err
        let result = SignalingMessage::from_json(r#"{"type":42}"#);
        assert!(result.is_err());
    }
}

