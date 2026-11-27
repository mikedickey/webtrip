//! API response types

use serde::{Deserialize, Serialize};
#[cfg(feature = "wasm")]
use tsify_next::Tsify;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// Health-check response (`GET /ping`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct Ping {
    /// Service status; `"OK"` when healthy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// LiveKit access token for a studio (`POST /studios/{studioId}/lktoken`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct LiveKitTokenResponse {
    /// LiveKit access token (a JWT)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

// Hand-written so the token never reaches logs.
impl std::fmt::Debug for LiveKitTokenResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveKitTokenResponse")
            .field("token", &self.token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// The JSON body of most API error responses.
///
/// Parse it from the `body` of an [`ApiError::Http`](crate::api::ApiError::Http):
///
/// ```
/// use webtrip_api::models::ErrorResponse;
///
/// let body = r#"{"error":"Studio is in motion"}"#;
/// let parsed: ErrorResponse = serde_json::from_str(body).unwrap();
/// assert_eq!(parsed.error.as_deref(), Some("Studio is in motion"));
/// ```
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct ErrorResponse {
    /// Human-readable error message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Shared creation and update timestamp model
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct ModifiedAtTime {
    /// RFC3339-formatted creation timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// RFC3339-formatted timestamp of most recent update
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// A URL to send the user to: a Stripe billing-portal or checkout session, or
/// an event's live stream.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct Redirect {
    /// Redirect URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect: Option<String>,
}

/// HubSpot visitor token response (`GET /users/{userId}/hubspot-token`)
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct HubSpotToken {
    /// HubSpot visitor identification token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// Server configuration (returned with studios)
///
/// Configuration properties for JackTrip Virtual Studios including
/// audio settings, network configuration, and visibility options.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
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

/// A studio together with the authenticated user's relationship to it
/// (`admin`, `owner`, `subStatus`). The [`server`](Self::server) fields are
/// flattened to the top level of the JSON object.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
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
