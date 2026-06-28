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

/// Unread messages count response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct UnreadMessagesResponse {
    /// Number of unread messages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
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

/// Shared creation and update timestamp model
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ModifiedAtTime {
    /// RFC3339-formatted creation timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// RFC3339-formatted timestamp of most recent update
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Redirect response (e.g. billing portal / checkout URLs, `/redirect/{ext}`)
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Redirect {
    /// Redirect URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect: Option<String>,
}

/// Signed download URL response (e.g. `GET /studios/{studioId}/tracks/{trackId}/download`).
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct DownloadUrl {
    /// Signed URL to download the file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
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

/// Server with the authenticated user's relationship to it.
///
/// Spec composes this as `Server` + `{admin, owner, subStatus}`.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ServerWithSubscription {
    /// Server/studio information
    #[serde(flatten)]
    pub server: super::Server,

    /// Whether the user is an admin of this studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin: Option<bool>,

    /// Whether the user is the creator/owner of this studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<bool>,

    /// Studio subscription status ("Active" | "Deleted")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_status: Option<String>,
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
    fn download_url_roundtrip() {
        let json = r#"{"url":"https://storage.googleapis.com/bucket/trk-1.wav?signature=abc"}"#;
        let d: DownloadUrl = serde_json::from_str(json).unwrap();
        assert_eq!(
            d.url.as_deref(),
            Some("https://storage.googleapis.com/bucket/trk-1.wav?signature=abc")
        );

        let out = roundtrip(&d);
        assert!(out.contains("\"url\":"));

        // Empty payload omits the optional field entirely.
        let empty = roundtrip(&DownloadUrl::default());
        assert_eq!(empty, "{}");
    }

    #[test]
    fn server_with_subscription_flattens_server() {
        let s = ServerWithSubscription {
            server: super::super::Server {
                id: Some("st1".into()),
                ..Default::default()
            },
            admin: Some(true),
            owner: Some(false),
            sub_status: Some("Active".into()),
        };
        let out = roundtrip(&s);
        // Flatten means Server fields are at top level, not nested under "server".
        assert!(out.contains("\"id\":\"st1\""));
        assert!(out.contains("\"subStatus\":\"Active\""));
        assert!(!out.contains("\"server\":"));
    }
}
