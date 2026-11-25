//! Hub signaling wire messages: the JSON the WebRTC signaling WebSocket
//! carries, and the connection states a signaling client reports.
//!
//! Pure data and (de)serialization, no I/O: the WebSocket client that sends
//! them lives in the browser host (`webtrip-web`).
//!
//! Message types, all JSON-encoded:
//!
//! - **Protocol Detection**: `{"protocol": "webrtc", "version": 1}` (client name is in WebSocket URL)
//! - **SDP Offer**: `{"type": "offer", "sdp": "v=0\r\n..."}`
//! - **SDP Answer**: `{"type": "answer", "sdp": "v=0\r\n..."}`
//! - **ICE Candidate**: `{"type": "ice", "candidate": "...", "sdpMid": "...", "sdpMLineIndex": 0}`
//! - **Hangup**: `{"type": "hangup"}`
//! - **Error**: `{"type": "error", "message": "..."}`

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

    // --- from_json protocol-response detection ---

    #[test]
    fn test_from_json_protocol_response() {
        // Any payload containing a "protocol" key is treated as the handshake
        // response and short-circuits to a Protocol message with all fields None.
        let msg = SignalingMessage::from_json(r#"{"protocol":"webrtc","version":1}"#).unwrap();
        assert_eq!(msg.msg_type, SignalingMessageType::Protocol);
        assert!(msg.sdp.is_none());
        assert!(msg.candidate.is_none());
        assert!(msg.sdp_mid.is_none());
        assert!(msg.sdp_m_line_index.is_none());
        assert!(msg.error.is_none());
    }

    // --- extract_string_field escape-sequence decoding ---

    #[test]
    fn test_extract_string_field_unescape_sequences() {
        // Hand-crafted JSON (not a to_json round-trip) so every backslash escape
        // branch in the unescape loop is exercised directly:
        //   \n \r \t          -> the corresponding control chars
        //   é            -> 'é' (unicode-escape branch)
        //   \x                -> literal backslash + 'x' (unknown-escape fallback)
        let json = r#"{"sdp":"a\nb\rc\tdé\xz"}"#;
        let decoded = super::extract_string_field(json, "sdp").unwrap();
        assert_eq!(decoded, "a\nb\rc\td\u{00e9}\\xz");
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
