//! User-related models

use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
#[cfg(feature = "wasm")]
use tsify_next::Tsify;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// A JackTrip user account. Keys are snake_case on the wire.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct User {
    /// Unique user identifier (an Auth0 ID such as `auth0|…`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// User name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// User nickname
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,

    /// Profile picture URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,

    /// Email address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// When the profile was last updated (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,

    /// When the account was created (RFC3339); absent when unknown
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// User-editable metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_metadata: Option<UserMetadata>,

    /// Application-managed metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_metadata: Option<AppMetadata>,
}

/// User-editable profile metadata. Keys are snake_case on the wire.
///
/// [`UsersApi::update_user`](crate::api::users::UsersApi::update_user) sends
/// only the fields that are `Some`.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct UserMetadata {
    /// Display name shown on the user's profile
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,

    /// Instruments the user plays
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruments: Option<Vec<String>>,

    /// Tags the user selected for themselves
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,

    /// Device used by default when connecting to a studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_device: Option<String>,

    /// Maximum value, in minutes, of the studio timer slider
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_server_timer: Option<i32>,

    /// When the user last accepted the terms (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_terms_approved: Option<String>,

    /// When the user last dismissed product updates (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_product_updates: Option<String>,

    /// When the user last viewed notifications (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_notifications_seen: Option<String>,

    /// When the user last viewed the studio tour (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_tour_seen: Option<String>,

    /// Facebook click ID captured from an ad
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fbclid: Option<String>,

    /// When `fbclid` was first seen
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fbclid_timestamp: Option<String>,

    /// Whether the account is pending deletion
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_delete_pending: Option<bool>,
}

/// Application-managed user metadata; read-only for users. Keys are snake_case
/// on the wire except `hubSpotContactId`.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct AppMetadata {
    /// Subscription plan name (e.g. `free`, `lite`, `essential`, `advanced`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,

    /// Plan minutes left in the current billing cycle (may be negative)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_minutes_left: Option<f64>,

    /// Coupon minutes left (never negative; used before plan minutes)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coupon_minutes_left: Option<f64>,

    /// Start of the current billing cycle (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_billing_start: Option<String>,

    /// End of the current billing cycle (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_billing_end: Option<String>,

    /// Largest studio instance the user may run, in vCPUs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_vcpu: Option<i32>,

    /// Whether the user has admin privileges
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_admin: Option<bool>,

    /// Stripe customer ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stripe_customer_id: Option<String>,

    /// Stripe subscription ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stripe_subscription_id: Option<String>,

    /// HubSpot contact ID
    #[serde(rename = "hubSpotContactId", skip_serializing_if = "Option::is_none")]
    pub hubspot_contact_id: Option<i64>,
}

/// Number of referrals a user has made (`GET /users/{userId}/referrals`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct ReferralCount {
    /// Number of referrals
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
}

/// What a [`Notification`] is about.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[repr(u8)]
pub enum NotificationClass {
    /// `0`: a product update
    ProductUpdate = 0,
    /// `1`: someone liked one of the user's recordings
    RecordingLike = 1,
    /// `2`: someone followed one of the user's streams
    StreamFollow = 2,
    /// `3`: a new upcoming event
    NewUpcomingEvent = 3,
    /// `4`: a new recording
    NewRecording = 4,
    /// `5`: a studio sent the user a message
    NewStudioMessageToUser = 5,
    /// `6`: a message was sent to one of the user's studios
    NewStudioMessageToServer = 6,
}

/// A user notification (`GET /users/{userId}/notifications`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    /// What the notification is about (`class` on the wire)
    #[serde(rename = "class", skip_serializing_if = "Option::is_none")]
    pub class: Option<NotificationClass>,

    /// Who caused the notification
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,

    /// Avatar image of the actor
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,

    /// Notification text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Link to the entity the notification is about
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,

    /// Image of the entity the notification is about
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Studio banner image URL, when the notification concerns a studio
    #[serde(rename = "serverBannerURL", skip_serializing_if = "Option::is_none")]
    pub server_banner_url: Option<String>,

    /// When the notified activity happened (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// One unread studio message (`GET /users/{userId}/unread-messages`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct UnreadMessage {
    /// Studio name of the conversation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_server_name: Option<String>,

    /// Studio image URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_server_image: Option<String>,

    /// Stream ID of the conversation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_stream_id: Option<String>,

    /// User ID of the conversation participant
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_user_id: Option<String>,

    /// Display name of the conversation participant
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_user_name: Option<String>,

    /// Avatar URL of the conversation participant
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_user_image: Option<String>,

    /// Message text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// When the message was sent (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// Whether the message is addressed to the user as a studio admin
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_wire_format() {
        // Regression: UserMetadata/AppMetadata were camelCase with fields the
        // API never sends (`bio`, `website`, `admin`), so they decoded empty.
        let json = serde_json::json!({
            "user_id": "auth0|abc123",
            "name": "Ada Lovelace",
            "email": "ada@example.com",
            "created_at": "2026-01-01T00:00:00Z",
            "user_metadata": {"display_name": "Ada", "instruments": ["piano"], "max_server_timer": 120},
            "app_metadata": {"plan": "essential", "is_admin": true, "plan_minutes_left": -5.5, "hubSpotContactId": 42}
        });
        let u: User = serde_json::from_value(json.clone()).unwrap();
        let meta = u.user_metadata.as_ref().unwrap();
        assert_eq!(meta.display_name.as_deref(), Some("Ada"));
        assert_eq!(meta.max_server_timer, Some(120));
        let app = u.app_metadata.as_ref().unwrap();
        assert_eq!(app.is_admin, Some(true));
        assert_eq!(app.plan_minutes_left, Some(-5.5));
        assert_eq!(app.hubspot_contact_id, Some(42));
        assert_eq!(serde_json::to_value(&u).unwrap(), json);
    }

    #[test]
    fn notification_wire_format() {
        let json = serde_json::json!({
            "class": 1, "actor": "Bob", "message": "liked your recording",
            "serverBannerURL": "b.png", "createdAt": "2026-06-14T00:00:00Z"
        });
        let n: Notification = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(n.class, Some(NotificationClass::RecordingLike));
        assert_eq!(n.server_banner_url.as_deref(), Some("b.png"));
        assert_eq!(serde_json::to_value(&n).unwrap(), json);
    }

    #[test]
    fn unread_message_wire_format() {
        let json = serde_json::json!({
            "conversationServerName": "Studio", "conversationStreamId": "s1",
            "conversationUserId": "u1", "message": "hi", "createdAt": "t", "admin": false
        });
        let m: UnreadMessage = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(m.conversation_stream_id.as_deref(), Some("s1"));
        assert_eq!(serde_json::to_value(&m).unwrap(), json);
    }
}
