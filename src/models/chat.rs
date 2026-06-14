//! Chat and messaging models

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Chat session
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ChatSession {
    /// Session ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Room/channel ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,

    /// Chat token for authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// WebSocket URL for real-time chat
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ws_url: Option<String>,
}

/// A chat/DM conversation
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    /// Conversation ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Other participant's user ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Other participant's name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_name: Option<String>,

    /// Other participant's profile picture
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_picture: Option<String>,

    /// Most recent message preview
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message: Option<String>,

    /// Timestamp of last message (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_message_at: Option<String>,

    /// Number of unread messages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unread_count: Option<i32>,
}

/// A chat message
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// Message ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Sender's user ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<String>,

    /// Sender's display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_name: Option<String>,

    /// Message content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Message type (text, image, etc.)
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub message_type: Option<String>,

    /// Creation timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// Whether the message has been read
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<bool>,
}

/// Session information
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// Session ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// User ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Studio ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studio_id: Option<String>,

    /// Device ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,

    /// Session start timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,

    /// Session end timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,

    /// Session duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T>(v: &T) -> String
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let s = serde_json::to_string(v).expect("serialize");
        let back: T = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(v, &back);
        s
    }

    #[test]
    fn chat_session_roundtrip_camel_case() {
        let c = ChatSession {
            id: Some("c1".into()),
            room_id: Some("room-1".into()),
            token: Some("tok".into()),
            ws_url: Some("wss://chat".into()),
        };
        let s = roundtrip(&c);
        assert!(s.contains("\"roomId\":"));
        assert!(s.contains("\"wsUrl\":"));
    }

    #[test]
    fn conversation_roundtrip() {
        let c = Conversation {
            id: Some("conv-1".into()),
            user_id: Some("u1".into()),
            user_name: Some("Alice".into()),
            user_picture: Some("https://p".into()),
            last_message: Some("hello".into()),
            last_message_at: Some("2026-06-14T00:00:00Z".into()),
            unread_count: Some(3),
        };
        let s = roundtrip(&c);
        assert!(s.contains("\"userId\":"));
        assert!(s.contains("\"unreadCount\":3"));
        assert!(s.contains("\"lastMessageAt\":"));
    }

    #[test]
    fn message_renames_type_field() {
        let m = Message {
            id: Some("m1".into()),
            sender_id: Some("u1".into()),
            sender_name: Some("Alice".into()),
            content: Some("hi".into()),
            message_type: Some("text".into()),
            created_at: Some("2026-06-14T00:00:00Z".into()),
            read: Some(false),
        };
        let s = roundtrip(&m);
        assert!(s.contains("\"type\":\"text\""));
        assert!(!s.contains("messageType"));
    }

    #[test]
    fn session_roundtrip_camel_case() {
        let s = Session {
            id: Some("sess-1".into()),
            user_id: Some("u1".into()),
            studio_id: Some("st1".into()),
            device_id: Some("d1".into()),
            started_at: Some("2026-06-14T00:00:00Z".into()),
            ended_at: None,
            duration: Some(600),
        };
        let out = roundtrip(&s);
        assert!(out.contains("\"userId\":"));
        assert!(out.contains("\"studioId\":"));
        assert!(out.contains("\"startedAt\":"));
        assert!(!out.contains("\"endedAt\":"));
    }
}
