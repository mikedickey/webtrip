//! Studio (server) models

use super::{BroadcastVisibility, BufferStrategy, Period, QueueBuffer, ResourceStatus};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Shared audio network properties used by devices and studios.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct AudioProperties {
    /// Audio frame period
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<Period>,

    /// Jitter buffer size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_buffer: Option<QueueBuffer>,

    /// Buffer strategy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_strategy: Option<BufferStrategy>,
}

/// SuperCollider mix configuration.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ServerMix {
    /// SuperCollider mixer branch (from jacktrip-sc)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mix_branch: Option<String>,

    /// SuperCollider mixer raw code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mix_code: Option<String>,
}

/// Studio configuration state when connected to by devices.
///
/// Spec composes this as `AudioProperties` + `ServerMix` + a few studio-level
/// fields; we flatten the shared structs.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ServerAgentConfig {
    /// Shared audio network properties
    #[serde(flatten)]
    pub audio: AudioProperties,

    /// SuperCollider mix configuration
    #[serde(flatten)]
    pub mix: ServerMix,

    /// Broadcast visibility setting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcast: Option<BroadcastVisibility>,

    /// Expiration timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,

    /// Maximum number of musicians allowed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_musicians: Option<i32>,
}

/// A JackTrip Virtual Studio instance (spec name: `Server`).
///
/// Composed from [`ServerAgentConfig`] plus studio-level identifiers and
/// metadata.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    /// Agent configuration (audio properties, mix, broadcast, expiry, capacity)
    #[serde(flatten)]
    pub config: ServerAgentConfig,

    /// Studio ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Owner's user ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,

    /// Cloud instance identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_id: Option<String>,

    /// Cloud identifier recorded when the studio was first provisioned
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_cloud_id: Option<String>,

    /// Active session identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Associated stream identifier (for broadcasting)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,

    /// Unlisted stream identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unlisted_stream_id: Option<String>,

    /// Chat room identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,

    /// Invite key used to generate shareable join links
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invite_key: Option<String>,

    /// Cloud region identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Instance size/type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,

    /// Banner image URL
    #[serde(rename = "bannerURL", skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,

    /// Current status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ResourceStatus>,

    /// Non-zero if this studio is provisioned and managed by JackTrip
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed: Option<i32>,

    /// Collaboration intent advertised by the studio owner
    /// (0 = unset, 1 = not looking, 2 = looking for band members, 3 = looking for students)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looking_for: Option<i32>,

    /// Skill levels the studio is targeted at
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_levels: Option<Vec<String>>,

    /// Instruments associated with the studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruments: Option<Vec<String>>,

    /// Musical genres associated with the studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genres: Option<Vec<String>>,

    /// Creation timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// Last update timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Access rights of the authenticated user for a studio (spec name: `ServerAccess`),
/// returned by `GET /studios/{studioId}/access`.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ServerAccess {
    /// Studio ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,

    /// Authenticated user ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Whether the user is a studio admin
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin: Option<bool>,

    /// Whether the user is the studio owner
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<bool>,

    /// List of named permissions with current values
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<ServerAccessPermission>>,
}

/// A single named permission entry within [`ServerAccess`].
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ServerAccessPermission {
    /// Permission name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Whether permission is granted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<bool>,

    /// Human-readable explanation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explanation: Option<String>,
}

/// Mixer definition (returned in the `GET /mixers` map).
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Mixer {
    /// Mixer type
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub mixer_type: Option<String>,

    /// Mixer source URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// List of mixer configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configs: Option<Vec<MixerConfig>>,

    /// List of link configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<MixerConfig>>,

    /// List of preset configurations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presets: Option<Vec<MixerConfig>>,
}

/// A single encoded mixer configuration entry.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct MixerConfig {
    /// Encoded mixer configuration content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Configuration encoding format (e.g. "base64")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

/// A participant in a studio session
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    /// Participant's user ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Participant's display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Device ID (for JackTrip devices)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,

    /// Whether the participant is muted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub muted: Option<bool>,

    /// Participant's volume level (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<u32>,

    /// Join timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub joined_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_utils::roundtrip;

    #[test]
    fn server_fixture_known_good() {
        // Fixture modeled after the spec `Server` schema. Note the `bannerURL`
        // (preserved casing) and the flattened agent-config fields.
        let json = r#"{
          "id": "studio123",
          "ownerId": "user-1",
          "initialCloudId": "i-0123",
          "region": "ec2-us-north-ca",
          "size": "c5.large",
          "bannerURL": "https://cdn.example.com/banner.png",
          "status": "Ready",
          "managed": 1,
          "lookingFor": 2,
          "skillLevels": ["beginner", "intermediate"],
          "instruments": ["guitar"],
          "genres": ["rock"],
          "period": 128,
          "queueBuffer": 4,
          "bufferStrategy": 1,
          "broadcast": 2,
          "maxMusicians": 5,
          "mixBranch": "main",
          "expiresAt": "2026-06-14T00:00:00Z",
          "createdAt": "2026-06-14T00:00:00Z"
        }"#;
        let s: Server = serde_json::from_str(json).unwrap();
        assert_eq!(s.id.as_deref(), Some("studio123"));
        assert_eq!(s.initial_cloud_id.as_deref(), Some("i-0123"));
        assert_eq!(s.status, Some(ResourceStatus::Ready));
        assert_eq!(s.managed, Some(1));
        assert_eq!(s.looking_for, Some(2));
        assert_eq!(s.config.audio.period, Some(Period::P128));
        assert_eq!(s.config.audio.queue_buffer, Some(QueueBuffer::Q4));
        assert_eq!(s.config.audio.buffer_strategy, Some(BufferStrategy::Standard));
        assert_eq!(s.config.broadcast, Some(BroadcastVisibility::Public));
        assert_eq!(s.config.max_musicians, Some(5));
        assert_eq!(s.config.mix.mix_branch.as_deref(), Some("main"));
        assert_eq!(s.config.expires_at.as_deref(), Some("2026-06-14T00:00:00Z"));

        // Wire-format check: flattened fields are at the top level.
        let out = serde_json::to_string(&s).unwrap();
        assert!(out.contains("\"bannerURL\":"));
        assert!(out.contains("\"ownerId\":"));
        assert!(out.contains("\"queueBuffer\":4"));
        assert!(out.contains("\"mixBranch\":\"main\""));
        assert!(out.contains("\"maxMusicians\":5"));
        assert!(!out.contains("\"config\":"));
        assert!(!out.contains("\"audio\":"));
    }

    #[test]
    fn server_status_removed_variant_roundtrips() {
        let s: Server = serde_json::from_str(r#"{"status":"Removed"}"#).unwrap();
        assert_eq!(s.status, Some(ResourceStatus::Removed));
    }

    #[test]
    fn server_access_fixture_known_good() {
        let json = r#"{
          "serverId": "studio-1",
          "userId": "user-1",
          "admin": true,
          "owner": false,
          "permissions": [
            {"name": "edit", "value": true, "explanation": "Can edit studio"}
          ]
        }"#;
        let a: ServerAccess = serde_json::from_str(json).unwrap();
        assert_eq!(a.server_id.as_deref(), Some("studio-1"));
        assert_eq!(a.user_id.as_deref(), Some("user-1"));
        assert_eq!(a.admin, Some(true));
        assert_eq!(a.owner, Some(false));
        assert_eq!(a.permissions.as_ref().map(|p| p.len()), Some(1));

        let out = roundtrip(&a);
        assert!(out.contains("\"serverId\":"));
        assert!(out.contains("\"userId\":"));
    }

    #[test]
    fn mixer_fixture_known_good() {
        let json = r#"{
          "type": "sclang",
          "url": "https://api.github.com/repos/jacktrip/jacktrip-sc/commits/abc",
          "configs": [{"content": "Zm9v", "encoding": "base64"}],
          "links": [],
          "presets": [{"content": "YmFy", "encoding": "base64"}]
        }"#;
        let m: Mixer = serde_json::from_str(json).unwrap();
        assert_eq!(m.mixer_type.as_deref(), Some("sclang"));
        assert_eq!(m.configs.as_ref().map(|c| c.len()), Some(1));
        assert_eq!(
            m.configs.as_ref().and_then(|c| c.first()).and_then(|c| c.encoding.as_deref()),
            Some("base64")
        );

        let out = roundtrip(&m);
        assert!(out.contains("\"type\":\"sclang\""));
        assert!(out.contains("\"configs\":"));
        assert!(out.contains("\"presets\":"));
        assert!(!out.contains("mixerType"));
    }
}
