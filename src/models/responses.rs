//! API response types

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Ping/health check response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Ping {
    /// API version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Server timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,

    /// Service status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Ping statistics
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PingStats {
    /// RTT statistics
    #[serde(flatten)]
    pub rtt: super::RttStats,

    /// Packets sent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packets_sent: Option<i32>,

    /// Packets received
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packets_received: Option<i32>,

    /// Packet loss percentage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packet_loss: Option<f64>,
}

/// LiveKit token response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct LiveKitTokenResponse {
    /// LiveKit access token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,

    /// LiveKit server URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Checkout session response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutResponse {
    /// Stripe checkout session URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Stripe session ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Unread messages count response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct UnreadMessagesResponse {
    /// Number of unread messages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
}

/// Generic URL response (used for redirects and billing portal URLs)
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct UrlResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}


/// API error response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Error {
    /// Error code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Error message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// HTTP status code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,

    /// Additional error details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Timestamp response for modified resources
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ModifiedAtTime {
    /// Modification timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
}

/// Audio properties for a track/stream
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct AudioProperties {
    /// Sample rate (Hz)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<i32>,

    /// Bit depth
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bit_depth: Option<i32>,

    /// Number of channels
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<i32>,

    /// Codec name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,

    /// Bitrate in kbps
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate: Option<i32>,
}

/// Server configuration (returned with studios)
///
/// Configuration properties for JackTrip Virtual Studios including
/// audio settings, network configuration, and visibility options.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    /// Studio type (audio engine configuration)
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub studio_type: Option<super::StudioType>,

    /// Studio display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Studio hostname/IP address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_host: Option<String>,

    /// Studio port number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_port: Option<i32>,

    /// Audio sample rate in Hz (44100, 48000, 88200, 96000)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<super::SampleRate>,

    /// Whether the studio is publicly visible
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public: Option<bool>,

    /// Whether stereo audio is enabled
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stereo: Option<bool>,

    /// Whether loopback audio is enabled
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loopback: Option<bool>,

    /// Whether the studio is currently active/enabled
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Server agent configuration
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ServerAgentConfig {
    /// Agent version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Update URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_url: Option<String>,

    /// Heartbeat interval in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_interval: Option<i32>,
}

/// Server with subscription information
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ServerWithSubscription {
    /// Server/Studio information
    #[serde(flatten)]
    pub server: super::Studio,

    /// Subscription information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subscription: Option<super::Subscription>,
}

/// Device configuration (internal)
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct DeviceConfig {
    /// Device settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<super::Device>,

    /// Studio settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studio: Option<super::Studio>,
}

/// Coupon response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct CouponResponse {
    /// Whether the coupon was valid
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid: Option<bool>,

    /// Discount amount (percentage or fixed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discount: Option<f64>,

    /// Discount type (percent_off, amount_off)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discount_type: Option<String>,

    /// Error message if invalid
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Promo code response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PromoResponse {
    /// Whether the promo was applied
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applied: Option<bool>,

    /// Description of the promo
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Error message if not applied
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Entitlement/feature flag
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Entitlement {
    /// Entitlement ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Whether it's enabled
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Invoice list response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceListResponse {
    /// List of invoices
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invoices: Option<Vec<Invoice>>,

    /// Cursor for pagination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,

    /// Whether there are more results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_more: Option<bool>,
}

/// Invoice
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Invoice {
    /// Invoice ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Invoice number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,

    /// Amount in cents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,

    /// Currency code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,

    /// Invoice status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    /// Invoice date (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,

    /// PDF download URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf_url: Option<String>,

    /// Hosted invoice URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hosted_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_utils::roundtrip;

    #[test]
    fn server_config_renames_type_to_studio_type() {
        let c = ServerConfig {
            studio_type: Some(super::super::StudioType::JackTripJamulus),
            name: Some("Studio".into()),
            server_host: Some("studio.example.com".into()),
            server_port: Some(4464),
            sample_rate: Some(super::super::SampleRate::Rate48000),
            public: Some(true),
            stereo: Some(true),
            loopback: Some(false),
            enabled: Some(true),
        };
        let s = roundtrip(&c);
        assert!(s.contains("\"type\":\"JackTrip+Jamulus\""));
        assert!(s.contains("\"sampleRate\":48000"));
        assert!(s.contains("\"serverHost\":"));
        assert!(s.contains("\"serverPort\":4464"));
        assert!(!s.contains("studioType"));
    }

    #[test]
    fn error_response_known_good_fixture() {
        // Fixture modeled after docs/api/error-handling.md.
        let json = r#"{
          "code": "not_found",
          "message": "Studio not found",
          "status": 404,
          "details": {"resource": "studio", "id": "missing"}
        }"#;
        let e: Error = serde_json::from_str(json).unwrap();
        assert_eq!(e.code.as_deref(), Some("not_found"));
        assert_eq!(e.status, Some(404));
        assert!(e.details.is_some());
        let s = roundtrip(&e);
        assert!(s.contains("\"code\":\"not_found\""));
        assert!(s.contains("\"status\":404"));
    }

    #[test]
    fn server_with_subscription_flattens_studio() {
        let s = ServerWithSubscription {
            server: super::super::Studio {
                id: Some("st1".into()),
                config: super::super::ServerConfig {
                    name: Some("Studio".into()),
                    ..Default::default()
                },
                ..Default::default()
            },
            subscription: Some(super::super::Subscription {
                id: Some("sub_1".into()),
                status: Some("active".into()),
                ..Default::default()
            }),
        };
        let out = roundtrip(&s);
        // Flatten means Studio fields are at top level, not nested under "server".
        assert!(out.contains("\"id\":\"st1\""));
        assert!(out.contains("\"subscription\":"));
        assert!(!out.contains("\"server\":"));
    }
}
