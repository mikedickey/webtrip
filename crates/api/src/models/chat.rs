//! Studio messaging models: conversations between a user and a studio's
//! stream, and the messages in them.

use serde::{Deserialize, Serialize};
#[cfg(feature = "wasm")]
use tsify_next::Tsify;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// A conversation between a user and a studio, with its last message.
///
/// A conversation is identified by the studio's `streamId` plus the user's
/// `userId`.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    /// Stream/channel ID of the studio side
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,

    /// User ID of the participant
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Conversation status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,

    /// Studio display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,

    /// Studio image URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_img: Option<String>,

    /// ID of the last message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,

    /// Sender ID of the last message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<String>,

    /// Text of the last message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Status of the last message (see [`Message::status`])
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_status: Option<i32>,

    /// Whether the authenticated user has unread messages here
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unread: Option<bool>,

    /// When the conversation was created (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// When the conversation was last updated (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A message in a [`Conversation`], with sender details.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct Message {
    /// Message ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Stream ID of the conversation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_stream_id: Option<String>,

    /// User ID of the conversation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_user_id: Option<String>,

    /// Sender's user ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_id: Option<String>,

    /// Sender's display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_name: Option<String>,

    /// Sender's avatar URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sender_img: Option<String>,

    /// Message text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// `0` = normal, `1` = deleted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,

    /// When the message was sent (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// When the message was last updated (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_and_message_wire_format() {
        // Regression: these used invented fields (`content`, `type`,
        // `lastMessage`, `unreadCount`) and decoded every response as empty.
        let conv = serde_json::json!({
            "streamId": "s1", "userId": "u1", "serverName": "Studio", "serverImg": "i.png",
            "last": "m9", "senderId": "u1", "text": "hi", "messageStatus": 0, "unread": true
        });
        let c: Conversation = serde_json::from_value(conv.clone()).unwrap();
        assert_eq!(c.text.as_deref(), Some("hi"));
        assert_eq!(c.unread, Some(true));
        assert_eq!(serde_json::to_value(&c).unwrap(), conv);

        let msg = serde_json::json!({
            "id": "m9", "conversationStreamId": "s1", "conversationUserId": "u1",
            "senderId": "u1", "senderName": "Ada", "senderImg": "a.png", "text": "hi", "status": 1
        });
        let m: Message = serde_json::from_value(msg.clone()).unwrap();
        assert_eq!(m.status, Some(1));
        assert_eq!(serde_json::to_value(&m).unwrap(), msg);
    }
}
