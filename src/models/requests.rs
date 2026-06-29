//! API request body types

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Device heartbeat request
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct HeartbeatRequest {
    /// Network and RTT statistics
    #[serde(flatten)]
    pub net: super::DeviceNetworkStats,
}

/// Send a message request
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageRequest {
    /// Message content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Message type (text, image, etc.)
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub message_type: Option<String>,
}

/// Mark-messages-read request (`POST /streams/{streamId}/conversations/{userId}/last`).
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct MarkReadRequest {
    /// ID of the last message read by the user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
}

/// Update-message request (`PUT /streams/{streamId}/conversations/{userId}/messages/{messageId}`).
///
/// Only `status` may be changed; message text is immutable after creation.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMessageRequest {
    /// Message status (0=normal, 1=deleted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,
}

/// Studio session feedback request
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackRequest {
    /// Rating (1-5)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rating: Option<i32>,

    /// Feedback comment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,

    /// Session ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Audio quality rating (1-5)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_quality: Option<i32>,

    /// Latency rating (1-5)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency: Option<i32>,

    /// Whether there were connection issues
    #[serde(skip_serializing_if = "Option::is_none")]
    pub had_issues: Option<bool>,
}

/// Studio invite request
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct InviteRequest {
    /// Email address to invite
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// Invite message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Invite type (email, sms, link)
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub invite_type: Option<String>,
}

/// Track update request
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct TrackUpdateRequest {
    /// Track name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Volume level (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<u32>,

    /// Pan position (-100 to 100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pan: Option<i32>,

    /// Whether the track is muted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mute: Option<bool>,

    /// Whether the track is soloed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub solo: Option<bool>,
}

/// Analytics event
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsEvent {
    /// Event name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,

    /// Event properties
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<serde_json::Value>,

    /// User ID (if authenticated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Anonymous ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anonymous_id: Option<String>,

    /// Event timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
}

/// Checkout session request (`POST /users/{userId}/checkout`).
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutRequest {
    /// Subscription plan name to check out (required)
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
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct BillingPortalRequest {
    /// URL to redirect to after the billing portal session ends
    #[serde(rename = "callbackURL", skip_serializing_if = "Option::is_none")]
    pub callback_url: Option<String>,
}

/// Coupon code request (`PUT /redemptions`).
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct CodeRequest {
    /// Coupon code to redeem
    pub code: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_utils::roundtrip;

    #[test]
    fn heartbeat_request_renames_type_field() {
        let h = HeartbeatRequest {
            net: super::super::DeviceNetworkStats {
                api_prefix: Some("pref".into()),
                api_secret: Some("sec".into()),
                mac: Some("00:11:22:33:44:55".into()),
                version: Some("2.4.0".into()),
                device_type: Some("usb-x2".into()),
                pkts_recv: Some(100),
                pkts_sent: Some(101),
                rtt: super::super::RttStats {
                    min_rtt: Some(10),
                    max_rtt: Some(30),
                    avg_rtt: Some(15),
                    stddev_rtt: Some(2),
                    latest_rtt: Some(14),
                    stats_updated_at: Some("2026-06-14T00:00:00Z".into()),
                },
            },
        };
        let s = roundtrip(&h);
        assert!(s.contains("\"type\":\"usb-x2\""));
        assert!(s.contains("\"apiPrefix\":\"pref\""));
        assert!(s.contains("\"pktsRecv\":100"));
        assert!(s.contains("\"statsUpdatedAt\":"));
    }

    #[test]
    fn send_message_request_renames_type_field() {
        let m = SendMessageRequest {
            content: Some("hello".into()),
            message_type: Some("text".into()),
        };
        let s = roundtrip(&m);
        assert!(s.contains("\"type\":\"text\""));
        assert!(!s.contains("messageType"));
    }

    #[test]
    fn mark_read_request_wire_format() {
        let r = MarkReadRequest {
            message_id: Some("m1".into()),
        };
        let s = roundtrip(&r);
        assert!(s.contains("\"messageId\":\"m1\""));
        assert!(!s.contains("message_id"));
    }

    #[test]
    fn update_message_request_wire_format() {
        let r = UpdateMessageRequest { status: Some(1) };
        let s = roundtrip(&r);
        assert!(s.contains("\"status\":1"));
    }

    #[test]
    fn checkout_request_wire_format() {
        let r = CheckoutRequest {
            plan: "pro".into(),
            pricing_mode: Some("yearly".into()),
            callback_url: "https://app/done".into(),
            force_stripe_test_mode: Some(true),
        };
        let s = roundtrip(&r);
        assert!(s.contains("\"plan\":\"pro\""));
        assert!(s.contains("\"pricingMode\":\"yearly\""));
        // callbackURL keeps its uppercase casing on the wire.
        assert!(s.contains("\"callbackURL\":"));
        assert!(s.contains("\"forceStripeTestMode\":true"));
        assert!(!s.contains("\"callbackUrl\":"));
    }

    #[test]
    fn billing_portal_request_renames_callback_url() {
        let r = BillingPortalRequest { callback_url: Some("https://app/done".into()) };
        let s = roundtrip(&r);
        assert!(s.contains("\"callbackURL\":"));
        assert!(!s.contains("\"callbackUrl\":"));
    }

    #[test]
    fn code_request_wire_format() {
        let r = CodeRequest { code: "FREEMONTH".into() };
        let s = roundtrip(&r);
        assert!(s.contains("\"code\":\"FREEMONTH\""));
    }

    #[test]
    fn invite_request_renames_type_field() {
        let i = InviteRequest {
            email: Some("a@b".into()),
            message: Some("join us".into()),
            invite_type: Some("email".into()),
        };
        let s = roundtrip(&i);
        assert!(s.contains("\"type\":\"email\""));
    }
}

