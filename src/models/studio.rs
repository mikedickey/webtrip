//! Studio (virtual server) models

use super::{BroadcastVisibility, BufferStrategy, Period, QueueBuffer, ResourceStatus, SampleRate, StudioType};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// A JackTrip Virtual Studio instance
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Studio {
    /// Studio ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Owner's user ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,

    /// Cloud instance identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_id: Option<String>,

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

    /// Audio frame period
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<Period>,

    /// Jitter buffer size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_buffer: Option<QueueBuffer>,

    /// Buffer strategy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_strategy: Option<BufferStrategy>,

    /// SuperCollider mixer branch name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mix_branch: Option<String>,

    /// Custom SuperCollider mixer code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mix_code: Option<String>,

    /// Broadcast visibility setting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcast: Option<BroadcastVisibility>,

    /// Maximum number of musicians allowed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_musicians: Option<i32>,

    /// Expiration timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,

    /// Creation timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// Last update timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,

    // =========================================================================
    // ServerConfig fields
    // =========================================================================

    /// Studio type (JackTrip or JackTrip+Jamulus)
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub studio_type: Option<StudioType>,

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
    pub sample_rate: Option<SampleRate>,

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

    // =========================================================================
    // ServerWithSubscription fields (returned when listing studios)
    // =========================================================================

    /// Whether the current user is an admin of this studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin: Option<bool>,

    /// Whether the current user is the owner of this studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<bool>,

    /// Subscription status (Active, Deleted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_status: Option<String>,
}

/// Studio access control settings
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct AccessSettings {
    /// Whether the studio requires a password
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_protected: Option<bool>,

    /// Studio access password (write-only, not returned in responses)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,

    /// Whether to allow anonymous/guest access
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_guests: Option<bool>,

    /// Maximum number of guests allowed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_guests: Option<i32>,

    /// Allowed user IDs (if restricted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_users: Option<Vec<String>>,
}

/// Studio mixer configuration
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Mixer {
    /// Mixer ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Mixer name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Mixer description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// SuperCollider code branch
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,

    /// Custom SuperCollider code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// Whether this is a system preset
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<bool>,
}

/// Mixer configuration settings
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct MixerConfig {
    /// Master volume (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_volume: Option<u32>,

    /// Reverb level (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverb: Option<u32>,

    /// Whether limiter is enabled
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limiter: Option<bool>,

    /// Whether compressor is enabled
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compressor: Option<bool>,
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

/// Server mix track information
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ServerMix {
    /// Track ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

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

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T>(v: &T) -> String
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let s = serde_json::to_string(v).expect("serialize");
        let back: T = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(v, &back);
        s
    }

    #[test]
    fn studio_fixture_known_good() {
        // Fixture modeled after docs/api/studios.md — a typical studio create response.
        // Note the `bannerURL` (preserved casing) and `type` rename.
        let json = r#"{
          "id": "studio123",
          "ownerId": "user-1",
          "region": "us-west-2",
          "type": "JackTrip+Jamulus",
          "name": "My Studio",
          "bannerURL": "https://cdn.example.com/banner.png",
          "status": "Ready",
          "period": 128,
          "queueBuffer": 4,
          "bufferStrategy": 1,
          "sampleRate": 48000,
          "broadcast": 2,
          "stereo": true,
          "public": false,
          "createdAt": "2026-06-14T00:00:00Z"
        }"#;
        let s: Studio = serde_json::from_str(json).unwrap();
        assert_eq!(s.id.as_deref(), Some("studio123"));
        assert_eq!(s.studio_type, Some(StudioType::JackTripJamulus));
        assert_eq!(s.status, Some(ResourceStatus::Ready));
        assert_eq!(s.period, Some(Period::P128));
        assert_eq!(s.queue_buffer, Some(QueueBuffer::Q4));
        assert_eq!(s.buffer_strategy, Some(BufferStrategy::Standard));
        assert_eq!(s.sample_rate, Some(SampleRate::Rate48000));
        assert_eq!(s.broadcast, Some(BroadcastVisibility::Public));
        assert_eq!(s.banner_url.as_deref(), Some("https://cdn.example.com/banner.png"));

        // Wire-format check: `type` and `bannerURL` are preserved verbatim.
        let out = serde_json::to_string(&s).unwrap();
        assert!(out.contains("\"type\":\"JackTrip+Jamulus\""));
        assert!(out.contains("\"bannerURL\":"));
        assert!(out.contains("\"ownerId\":"));
        assert!(out.contains("\"queueBuffer\":4"));
    }

    #[test]
    fn studio_roundtrip_with_enum_fields() {
        let s = Studio {
            id: Some("s1".into()),
            studio_type: Some(StudioType::JackTrip),
            status: Some(ResourceStatus::Starting),
            period: Some(Period::P256),
            queue_buffer: Some(QueueBuffer::Q8),
            buffer_strategy: Some(BufferStrategy::AutoAdjust),
            sample_rate: Some(SampleRate::Rate96000),
            broadcast: Some(BroadcastVisibility::Private),
            ..Default::default()
        };
        roundtrip(&s);
    }

    #[test]
    fn studio_empty_default_serializes_to_empty_object() {
        let s = Studio::default();
        assert_eq!(serde_json::to_string(&s).unwrap(), "{}");
    }

    #[test]
    fn access_settings_roundtrip_camel_case() {
        let a = AccessSettings {
            password_protected: Some(true),
            password: Some("hunter2".into()),
            allow_guests: Some(false),
            max_guests: Some(0),
            allowed_users: Some(vec!["u1".into(), "u2".into()]),
        };
        let s = roundtrip(&a);
        assert!(s.contains("\"passwordProtected\":true"));
        assert!(s.contains("\"allowGuests\":false"));
        assert!(s.contains("\"maxGuests\":0"));
        assert!(s.contains("\"allowedUsers\":[\"u1\",\"u2\"]"));
    }

    #[test]
    fn mixer_and_mixer_config_roundtrip() {
        let m = Mixer {
            id: Some("m1".into()),
            name: Some("Default".into()),
            description: Some("System preset".into()),
            branch: Some("main".into()),
            code: Some("// sc code".into()),
            preset: Some(true),
        };
        roundtrip(&m);

        let cfg = MixerConfig {
            master_volume: Some(80),
            reverb: Some(25),
            limiter: Some(true),
            compressor: Some(false),
        };
        let s = roundtrip(&cfg);
        assert!(s.contains("\"masterVolume\":80"));
    }

    #[test]
    fn participant_and_server_mix_roundtrip() {
        let p = Participant {
            user_id: Some("u1".into()),
            name: Some("Alice".into()),
            device_id: Some("d1".into()),
            muted: Some(false),
            volume: Some(75),
            joined_at: Some("2026-06-14T01:23:45Z".into()),
        };
        let s = roundtrip(&p);
        assert!(s.contains("\"userId\":\"u1\""));
        assert!(s.contains("\"deviceId\":\"d1\""));

        let mix = ServerMix {
            id: Some("t1".into()),
            name: Some("Drums".into()),
            volume: Some(60),
            pan: Some(-25),
            mute: Some(false),
            solo: Some(true),
        };
        roundtrip(&mix);
    }
}
