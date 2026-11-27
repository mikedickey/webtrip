//! Studio (server) models

use super::{BroadcastVisibility, BufferStrategy, Period, QueueBuffer, ResourceStatus, ServerConfig};
use serde::{Deserialize, Serialize};
#[cfg(feature = "wasm")]
use tsify_next::Tsify;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// Shared audio network properties used by devices and studios.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
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
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct ServerMix {
    /// SuperCollider mixer branch (from jacktrip-sc)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mix_branch: Option<String>,

    /// SuperCollider mixer raw code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mix_code: Option<String>,
}

/// The studio settings devices apply when they connect: audio properties, the
/// mix, broadcast visibility, expiry and capacity. The [`audio`](Self::audio)
/// and [`mix`](Self::mix) fields are flattened, so their keys sit at the top
/// level of the JSON object.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct ServerAgentConfig {
    /// Shared audio network properties
    #[serde(flatten)]
    pub audio: AudioProperties,

    /// SuperCollider mix configuration
    #[serde(flatten)]
    pub mix: ServerMix,

    /// Broadcast visibility: [`Offline`](BroadcastVisibility::Offline),
    /// [`Public`](BroadcastVisibility::Public) or
    /// [`Unlisted`](BroadcastVisibility::Unlisted)
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
/// metadata. The studio configuration properties (`type`, `name`, `serverHost`,
/// `serverPort`, `sampleRate`, `public`, `stereo`, `loopback`, `enabled`) are
/// flattened in from [`ServerConfig`] so create/update payloads and `/studios`
/// responses preserve studio metadata.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct Server {
    /// Studio configuration (type, name, host/port, sample rate, visibility flags)
    #[serde(flatten)]
    pub config: ServerConfig,

    /// Agent configuration (audio properties, mix, broadcast, expiry, capacity)
    #[serde(flatten)]
    pub agent: ServerAgentConfig,

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

    /// Cloud region identifier, e.g. `ec2-us-oregon` or `azure-us-wa-seattle`
    /// (the keys of [`SystemApi::list_regions`](crate::api::system::SystemApi::list_regions))
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Instance type, e.g. `c5.large`, `Standard_F2s_v2` or `e2-highcpu-2`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,

    /// Banner image URL
    #[serde(rename = "bannerURL", skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,

    /// Current status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ResourceStatus>,

    /// `1` if JackTrip provisions and manages this studio, `0` if it is
    /// self-hosted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed: Option<i32>,

    /// Collaboration intent advertised by the studio owner: `0` = unset,
    /// `1` = not looking, `2` = looking for band members, `3` = looking for
    /// students. Also the value to pass as
    /// [`StreamSearchQuery::looking_for`](super::StreamSearchQuery::looking_for).
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
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
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
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
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
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
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
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct MixerConfig {
    /// Encoded mixer configuration content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Configuration encoding format (e.g. "base64")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<String>,
}

/// A participant in a studio's active session
/// (`GET /studios/{studioId}/participants`). Keys are snake_case on the wire.
///
/// For the participant's full profile, pass `user_id` to
/// [`StudiosApi::get_participant`](crate::api::studios::StudiosApi::get_participant).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct Participant {
    /// Participant's user ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Participant's profile picture URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
}

/// A studio's current session (`GET /studios/{studioId}/session`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct Session {
    /// Session ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Studio ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,

    /// When the session started (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,

    /// Whether this is the first session on its cloud instance
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_initial: Option<bool>,

    /// Session API key; only for self-hosted studios, and only when the caller
    /// is a studio admin
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Session API secret; present under the same conditions as `api_key`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_secret: Option<String>,
}

// Hand-written so the session secret never reaches logs.
impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("id", &self.id)
            .field("server_id", &self.server_id)
            .field("started_at", &self.started_at)
            .field("is_initial", &self.is_initial)
            .field("api_key", &self.api_key)
            .field("api_secret", &self.api_secret.as_ref().map(|_| "<redacted>"))
            .finish()
    }
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
          "type": "JackTrip+Jamulus",
          "name": "My Studio",
          "serverHost": "1.2.3.4",
          "serverPort": 4464,
          "sampleRate": 48000,
          "public": true,
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
        // ServerConfig fields (flattened).
        assert_eq!(s.config.name.as_deref(), Some("My Studio"));
        assert_eq!(s.config.studio_type, Some(super::super::StudioType::JackTripJamulus));
        assert_eq!(s.config.server_port, Some(4464));
        assert_eq!(s.config.public, Some(true));
        // ServerAgentConfig fields (flattened).
        assert_eq!(s.agent.audio.period, Some(Period::P128));
        assert_eq!(s.agent.audio.queue_buffer, Some(QueueBuffer::Q4));
        assert_eq!(s.agent.audio.buffer_strategy, Some(BufferStrategy::Standard));
        assert_eq!(s.agent.broadcast, Some(BroadcastVisibility::Unlisted));
        assert_eq!(s.agent.max_musicians, Some(5));
        assert_eq!(s.agent.mix.mix_branch.as_deref(), Some("main"));
        assert_eq!(s.agent.expires_at.as_deref(), Some("2026-06-14T00:00:00Z"));

        // Wire-format check: flattened fields are at the top level.
        let out = serde_json::to_string(&s).unwrap();
        assert!(out.contains("\"bannerURL\":"));
        assert!(out.contains("\"ownerId\":"));
        assert!(out.contains("\"type\":\"JackTrip+Jamulus\""));
        assert!(out.contains("\"name\":\"My Studio\""));
        assert!(out.contains("\"serverPort\":4464"));
        assert!(out.contains("\"queueBuffer\":4"));
        assert!(out.contains("\"mixBranch\":\"main\""));
        assert!(out.contains("\"maxMusicians\":5"));
        assert!(!out.contains("\"config\":"));
        assert!(!out.contains("\"agent\":"));
        assert!(!out.contains("\"audio\":"));
    }

    #[test]
    fn participant_and_session_wire_format() {
        // Regression: Participant was camelCase with invented fields
        // (`muted`, `volume`, `joinedAt`); the API sends snake_case `user_id`.
        let p = serde_json::json!({"user_id": "u1", "picture": "p.png"});
        let parsed: Participant = serde_json::from_value(p.clone()).unwrap();
        assert_eq!(parsed.user_id.as_deref(), Some("u1"));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), p);

        let s = serde_json::json!({
            "id": "sess1", "serverId": "st1", "startedAt": "t", "isInitial": true,
            "apiKey": "k", "apiSecret": "shh"
        });
        let parsed: Session = serde_json::from_value(s.clone()).unwrap();
        assert_eq!(parsed.server_id.as_deref(), Some("st1"));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), s);
        assert!(!format!("{parsed:?}").contains("shh"));
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
