//! API request body types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(feature = "wasm")]
use tsify_next::Tsify;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// Device heartbeat body (`POST /devices/{deviceId}/heartbeat`): the device's
/// API key plus what it reports. Both parts are flattened into one JSON object.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct HeartbeatRequest {
    /// The device's API key (`apiPrefix` / `apiSecret`)
    #[serde(flatten)]
    pub credentials: super::AgentCredentials,

    /// MAC address, version, type and ping statistics
    #[serde(flatten)]
    pub heartbeat: super::DeviceHeartbeat,
}

/// Send-message body (`POST /streams/{streamId}/conversations/{userId}/messages`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct SendMessageRequest {
    /// Message text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// `0` = normal (the default), `1` = deleted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,
}

/// Mark-messages-read request (`POST /streams/{streamId}/conversations/{userId}/last`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct MarkReadRequest {
    /// ID of the last message read by the user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// Update-message request (`PUT /streams/{streamId}/conversations/{userId}/messages/{messageId}`).
///
/// Only `status` may be changed; message text is immutable after creation.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct UpdateMessageRequest {
    /// Message status (0=normal, 1=deleted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,
}

/// Studio feedback body (`POST /studios/{studioId}/feedback`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct FeedbackRequest {
    /// Rating from 1 to 5
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<i32>,

    /// Feedback text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Client platform, e.g. `"web"` or `"desktop"`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,

    /// Operating system version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_version: Option<String>,

    /// Application version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,

    /// Audio input device name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_device: Option<String>,

    /// Audio output device name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_device: Option<String>,

    /// Description of any device issues
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_issues: Option<String>,
}

/// Studio invitation body (`POST /studios/{studioId}/invite`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct InviteRequest {
    /// Email addresses to invite
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emails: Vec<String>,

    /// Optional message included in the invitation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Backing-track update body (`PUT /studios/{studioId}/tracks/{trackId}`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct TrackUpdateRequest {
    /// New track display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// An analytics event (`POST /collect`). Keys are snake_case on the wire
/// except `EnableCustomValue` / `CustomValue`.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct AnalyticsEvent {
    /// Event name, e.g. `vs_studio`, `vs_device` or `vs_session`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// ID of the user the event concerns
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// Whether the event came from an anonymous user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_anonymous: Option<bool>,

    /// URL the event originated from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Arbitrary key/value labels
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<HashMap<String, String>>,

    /// Optional deduplication key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_id: Option<String>,

    /// Event time (RFC3339); set by the server, so leave it `None` when sending
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<String>,

    /// Optional user details for enriched tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_metadata: Option<AnalyticsUserMetadata>,

    /// When `true`, [`custom_value`](Self::custom_value) replaces the event's
    /// default metric value
    #[serde(rename = "EnableCustomValue", skip_serializing_if = "Option::is_none")]
    pub enable_custom_value: Option<bool>,

    /// Custom metric value, used when `enable_custom_value` is `true`
    #[serde(rename = "CustomValue", skip_serializing_if = "Option::is_none")]
    pub custom_value: Option<f64>,
}

/// User details attached to an [`AnalyticsEvent`]. Keys are snake_case.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct AnalyticsUserMetadata {
    /// Email address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// First name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_name: Option<String>,

    /// Last name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_name: Option<String>,

    /// Browser user agent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,

    /// Facebook click ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fbclid: Option<String>,

    /// When `fbclid` was first seen
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fbclid_timestamp: Option<String>,
}

/// Checkout session request (`POST /users/{userId}/checkout`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct CheckoutRequest {
    /// Subscription plan name to check out, e.g. `"solo"` or `"ensemble"` (required)
    pub plan: String,

    /// URL to redirect to after checkout completes (required)
    #[serde(rename = "callbackURL")]
    pub callback_url: String,

    /// Pricing mode (e.g. "yearly")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pricing_mode: Option<String>,

    /// Force Stripe test mode regardless of environment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_stripe_test_mode: Option<bool>,
}

/// Billing portal session request (`POST /users/{userId}/billing`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct BillingPortalRequest {
    /// URL to redirect to after the billing portal session ends
    #[serde(rename = "callbackURL", skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,
}

/// Coupon code request (`PUT /redemptions`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct CodeRequest {
    /// Coupon code to redeem
    pub code: String,
}

/// Create studio subscription request (`POST /studios/{studioId}/subscriptions`).
///
/// All fields are optional; `inviteKey` is only needed for private studios.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct CreateSubscriptionRequest {
    /// Studio ID to subscribe to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,

    /// User ID of the subscriber
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Optional invite key for private studios
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_key: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Each request body below once used field names the API does not read,
    // so the server silently dropped them. These pin the exact JSON sent.

    #[test]
    fn heartbeat_request_wire_format() {
        let h = HeartbeatRequest {
            credentials: super::super::AgentCredentials { api_prefix: Some("pre".into()), api_secret: Some("sec".into()) },
            heartbeat: super::super::DeviceHeartbeat {
                mac: Some("00:11".into()),
                ping: super::super::PingStats { avg_rtt: Some(15), ..Default::default() },
                ..Default::default()
            },
        };
        assert_eq!(
            serde_json::to_value(&h).unwrap(),
            json!({"apiPrefix": "pre", "apiSecret": "sec", "mac": "00:11", "avg_rtt": 15})
        );
    }

    #[test]
    fn message_feedback_invite_and_track_wire_format() {
        let m = SendMessageRequest { text: Some("hello".into()), status: Some(0) };
        assert_eq!(serde_json::to_value(&m).unwrap(), json!({"text": "hello", "status": 0}));

        let f = FeedbackRequest {
            rating: Some(5),
            message: Some("great".into()),
            os_version: Some("14".into()),
            app_version: Some("1.0".into()),
            input_device: Some("mic".into()),
            device_issues: Some("none".into()),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&f).unwrap(),
            json!({"rating": 5, "message": "great", "osVersion": "14", "appVersion": "1.0",
                   "inputDevice": "mic", "deviceIssues": "none"})
        );

        let i = InviteRequest { emails: vec!["a@example.com".into()], message: Some("join".into()) };
        assert_eq!(serde_json::to_value(&i).unwrap(), json!({"emails": ["a@example.com"], "message": "join"}));

        let t = TrackUpdateRequest { name: Some("Loop".into()) };
        assert_eq!(serde_json::to_value(&t).unwrap(), json!({"name": "Loop"}));
    }

    #[test]
    fn analytics_event_wire_format() {
        let e = AnalyticsEvent {
            name: Some("vs_session".into()),
            user: Some("u1".into()),
            is_anonymous: Some(false),
            labels: Some(HashMap::from([("k".to_string(), "v".to_string())])),
            insert_id: Some("dedupe".into()),
            user_metadata: Some(AnalyticsUserMetadata { first_name: Some("Ada".into()), ..Default::default() }),
            enable_custom_value: Some(true),
            custom_value: Some(1.5),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"name": "vs_session", "user": "u1", "is_anonymous": false, "labels": {"k": "v"},
                   "insert_id": "dedupe", "user_metadata": {"first_name": "Ada"},
                   "EnableCustomValue": true, "CustomValue": 1.5})
        );
    }

    #[test]
    fn billing_request_wire_format() {
        let r = CheckoutRequest {
            plan: "solo".into(),
            pricing_mode: Some("yearly".into()),
            callback_url: "https://app/done".into(),
            force_stripe_test_mode: Some(true),
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            json!({"plan": "solo", "pricingMode": "yearly", "callbackURL": "https://app/done",
                   "forceStripeTestMode": true})
        );
        let p = BillingPortalRequest { callback_url: Some("https://app/done".into()) };
        assert_eq!(serde_json::to_value(&p).unwrap(), json!({"callbackURL": "https://app/done"}));
        assert_eq!(serde_json::to_value(CreateSubscriptionRequest::default()).unwrap(), json!({}));
    }
}
