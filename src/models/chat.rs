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

