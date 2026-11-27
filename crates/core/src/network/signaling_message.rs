//! Hub signaling wire messages: the JSON the WebRTC signaling WebSocket
//! carries, and the connection states a signaling client reports.
//!
//! Pure data and (de)serialization, no I/O: the WebSocket client that sends
//! them lives in the browser host (`webtrip-web`).
//!
//! Message types, all JSON objects (shown here as [`SignalingMessage::to_json`]
//! writes them; [`SignalingMessage::from_json`] accepts any whitespace):
//!
//! - **Protocol Detection**: `{"protocol":"webrtc","version":1}` (the client name goes in the WebSocket URL)
//! - **SDP Offer**: `{"type":"offer","sdp":"v=0\r\n..."}`
//! - **SDP Answer**: `{"type":"answer","sdp":"v=0\r\n..."}`
//! - **ICE Candidate**: `{"type":"ice","candidate":"...","sdpMid":"...","sdpMLineIndex":0}`
//! - **Hangup**: `{"type":"hangup"}`
//! - **Error**: `{"type":"error","message":"..."}`

#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

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
    /// What kind of message this is; decides which other fields are set.
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

    /// Serialize to compact JSON (no whitespace), in the shapes listed in the
    /// [module docs](crate::network::signaling_message).
    ///
    /// Fields the message type needs but that are `None` are filled with
    /// defaults rather than omitted: an empty `sdp` or `candidate`, `sdpMid`
    /// `"data"`, `sdpMLineIndex` `0`, and error message `"Unknown error"`.
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

    /// Parse one signaling message.
    ///
    /// Any JSON object with a top-level `"protocol"` key is the hub's answer
    /// to the protocol handshake and parses as
    /// [`Protocol`](SignalingMessageType::Protocol), with every other field
    /// `None`. Otherwise the `"type"` string selects the message. For `"ice"`,
    /// a missing `sdpMid` defaults to `"data"` and a missing `sdpMLineIndex`
    /// to `0`; for `"error"`, a missing `message` defaults to
    /// `"Unknown error"`. Unknown keys are ignored.
    ///
    /// # Errors
    ///
    /// A human-readable reason if `json` is not a JSON object, has no string
    /// `"type"` (and no `"protocol"`), names an unknown type, or lacks a
    /// required field: `sdp` for offers and answers, `candidate` for ICE.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let value: serde_json::Value =
            serde_json::from_str(json).map_err(|e| format!("Invalid JSON: {e}"))?;
        let object = value.as_object().ok_or("Message is not a JSON object")?;

        if object.contains_key("protocol") {
            return Ok(Self::protocol());
        }

        let string_field = |field: &str| -> Result<&str, String> {
            match object.get(field) {
                None => Err(format!("Field '{field}' not found")),
                Some(v) => v.as_str().ok_or_else(|| format!("Field '{field}' is not a string")),
            }
        };

        let msg_type = string_field("type")?;
        match msg_type {
            "offer" => Ok(Self::offer(string_field("sdp")?)),
            "answer" => Ok(Self::answer(string_field("sdp")?)),
            "ice" => {
                let candidate = string_field("candidate")?;
                let sdp_mid = string_field("sdpMid").unwrap_or("data");
                let sdp_m_line_index = object
                    .get("sdpMLineIndex")
                    .and_then(|n| n.as_u64())
                    .map_or(0, |n| n as u16);
                Ok(Self::ice(candidate, sdp_mid, sdp_m_line_index))
            }
            "hangup" => Ok(Self::hangup()),
            "error" => {
                let message = string_field("message").unwrap_or("Unknown error").to_string();
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

/// Connection state for hub signaling
#[cfg_attr(feature = "wasm", wasm_bindgen)]
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

/// Fields extracted from an ICE-candidate JSON string.
///
/// The JSON format used by the JackTrip hub server is:
/// `{"candidate":"...","sdpMid":"audio","sdpMLineIndex":0}`
#[derive(Debug, PartialEq, Eq)]
pub struct ParsedIceCandidate {
    /// The `candidate` string (SDP `a=candidate` line).
    pub candidate: String,
    /// The `sdpMid` media ID, if present.
    pub sdp_mid: Option<String>,
    /// The `sdpMLineIndex` media line index, if present.
    pub sdp_m_line_index: Option<u16>,
}

/// Parse an ICE-candidate JSON string into its component fields.
///
/// Returns `None` for empty input, invalid JSON, or when the required
/// `"candidate"` key is absent or not a string.  The `sdpMid` and
/// `sdpMLineIndex` keys are optional and map to `None` when missing.
///
/// This is the single source of truth for ICE-candidate JSON parsing.
/// The browser callback wraps this and forwards the result to `web_sys`.
pub fn parse_ice_candidate_json(json: &str) -> Option<ParsedIceCandidate> {
    if json.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let candidate = v.get("candidate")?.as_str()?.to_string();
    let sdp_mid = v.get("sdpMid").and_then(|s| s.as_str()).map(String::from);
    let sdp_m_line_index = v
        .get("sdpMLineIndex")
        .and_then(|n| n.as_u64())
        .map(|n| n as u16);
    Some(ParsedIceCandidate { candidate, sdp_mid, sdp_m_line_index })
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
        let result = SignalingMessage::from_json(r#"{"type":42}"#);
        assert!(result.is_err());
        assert!(SignalingMessage::from_json("not json").is_err());
        assert!(SignalingMessage::from_json(r#"["type","offer"]"#).is_err());
    }

    /// Regression: the old hand-rolled parser matched `"type":"` byte-for-byte,
    /// so the module docs' own examples (a space after each colon) failed
    /// with "Field 'type' is not a string".
    #[test]
    fn test_from_json_accepts_whitespace() {
        let ice = SignalingMessage::from_json(
            r#"{ "type" : "ice", "candidate": "c", "sdpMid": "audio", "sdpMLineIndex": 2 }"#,
        )
        .unwrap();
        assert_eq!(ice.msg_type, SignalingMessageType::Ice);
        assert_eq!(ice.candidate.as_deref(), Some("c"));
        assert_eq!(ice.sdp_mid.as_deref(), Some("audio"));
        assert_eq!(ice.sdp_m_line_index, Some(2));

        let answer = SignalingMessage::from_json(r#"{"type": "answer", "sdp": "a\nb \"q\" é"}"#).unwrap();
        assert_eq!(answer.sdp.as_deref(), Some("a\nb \"q\" \u{00e9}"));
    }

    /// ICE and error fields fall back to their documented defaults.
    #[test]
    fn test_from_json_optional_field_defaults() {
        let ice = SignalingMessage::from_json(r#"{"type":"ice","candidate":"c"}"#).unwrap();
        assert_eq!((ice.sdp_mid.as_deref(), ice.sdp_m_line_index), (Some("data"), Some(0)));
        let err = SignalingMessage::from_json(r#"{"type":"error"}"#).unwrap();
        assert_eq!(err.error.as_deref(), Some("Unknown error"));
    }

    // --- from_json protocol-response detection ---

    #[test]
    fn test_from_json_protocol_response() {
        // A top-level "protocol" key is the handshake response and
        // short-circuits to a Protocol message with all fields None.
        let msg = SignalingMessage::from_json(r#"{"protocol": "webrtc", "version": 1}"#).unwrap();
        assert_eq!(msg.msg_type, SignalingMessageType::Protocol);
        assert!(msg.sdp.is_none());
        assert!(msg.candidate.is_none());
        assert!(msg.sdp_mid.is_none());
        assert!(msg.sdp_m_line_index.is_none());
        assert!(msg.error.is_none());

        // Regression: the substring `"protocol"` inside a value (here an
        // error message quoting it) used to be classified as the handshake.
        let err = SignalingMessage::from_json(r#"{"type":"error","message":"bad \"protocol\" field"}"#).unwrap();
        assert_eq!(err.msg_type, SignalingMessageType::Error);
    }

    // ── ICE candidate JSON parsing ────────────────────────────────────────────

    #[test]
    fn parse_ice_candidate_json_full_object() {
        let json = r#"{"candidate":"candidate:1 1 UDP 2122252543 192.168.1.1 56789 typ host","sdpMid":"audio","sdpMLineIndex":0}"#;
        let parsed = parse_ice_candidate_json(json).unwrap();
        assert!(parsed.candidate.starts_with("candidate:"));
        assert_eq!(parsed.sdp_mid.as_deref(), Some("audio"));
        assert_eq!(parsed.sdp_m_line_index, Some(0));
    }

    #[test]
    fn parse_ice_candidate_json_optional_fields_absent() {
        let json = r#"{"candidate":"candidate:1 1 UDP 2122252543 10.0.0.1 12345 typ host"}"#;
        let parsed = parse_ice_candidate_json(json).unwrap();
        assert!(parsed.candidate.contains("typ host"));
        assert_eq!(parsed.sdp_mid, None);
        assert_eq!(parsed.sdp_m_line_index, None);
    }

    #[test]
    fn parse_ice_candidate_json_empty_string_returns_none() {
        assert_eq!(parse_ice_candidate_json(""), None);
    }

    #[test]
    fn parse_ice_candidate_json_malformed_returns_none() {
        assert_eq!(parse_ice_candidate_json("{not valid json"), None);
        assert_eq!(parse_ice_candidate_json("null"), None);
        assert_eq!(parse_ice_candidate_json("[]"), None);
    }

    #[test]
    fn parse_ice_candidate_json_missing_candidate_key_returns_none() {
        // JSON object but no "candidate" field → must return None.
        let json = r#"{"sdpMid":"audio","sdpMLineIndex":0}"#;
        assert_eq!(parse_ice_candidate_json(json), None);
    }

    #[test]
    fn parse_ice_candidate_json_non_string_candidate_returns_none() {
        // "candidate" present but not a string → must return None.
        let json = r#"{"candidate":42}"#;
        assert_eq!(parse_ice_candidate_json(json), None);
    }
}
