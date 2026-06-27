//! Stream and broadcast models

use super::Visibility;
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Public stream/channel information
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfo {
    /// Stream ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Stream name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Stream description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Cumulative follower count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follows: Option<i64>,

    /// Studio display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,

    /// HLS metadata URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta_url: Option<String>,

    /// Chat room ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,

    /// Recording identifier associated with this stream
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_id: Option<String>,

    /// Banner image URL
    #[serde(rename = "bannerURL", skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,
}

/// Stream info enriched with search-specific flags, returned by `GET /streams/search`.
///
/// Spec models this as an `allOf` over [`StreamInfo`]; we mirror that with a
/// flattened `base`.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfoSearchResult {
    /// Base stream information
    #[serde(flatten)]
    pub base: StreamInfo,

    /// Studio (server) identifier (only present for public studios)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,

    /// Cloud region identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Studio looking-for status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looking_for: Option<i32>,

    /// Skill levels
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_levels: Option<Vec<String>>,

    /// Instruments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruments: Option<Vec<String>>,

    /// Genres
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genres: Option<Vec<String>>,

    /// Whether the studio is publicly accessible
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public: Option<bool>,

    /// Whether the studio is public and has an active session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_public_with_active_session: Option<bool>,

    /// Whether the studio is actively recruiting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_recruiting: Option<bool>,

    /// Whether the studio is publicly broadcasting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_publicly_broadcasting: Option<bool>,

    /// Number of public recordings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_public_recordings: Option<i32>,
}

/// Stream info with engagement metrics
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfoWithEngagement {
    /// Base stream information
    #[serde(flatten)]
    pub base: StreamInfo,

    /// Number of current viewers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub viewers: Option<i32>,

    /// Number of followers
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followers: Option<i32>,

    /// Whether the current user follows this stream
    #[serde(skip_serializing_if = "Option::is_none")]
    pub following: Option<bool>,
}

/// Live stream configuration
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct LiveStream {
    /// Stream ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Stream name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Stream description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Visibility setting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,

    /// Whether the stream is currently active
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,

    /// RTMP stream key (write-only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_key: Option<String>,

    /// HLS playback URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hls_url: Option<String>,

    /// RTMP ingest URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtmp_url: Option<String>,
}

/// Simulcast destination configuration
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct SimulcastDestination {
    /// Destination ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Destination platform (youtube, twitch, facebook, custom)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<String>,

    /// Destination name/label
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// RTMP URL for the destination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rtmp_url: Option<String>,

    /// Stream key for the destination
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_key: Option<String>,

    /// Whether this destination is enabled
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// Stream activation options
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ActivationRequestOpts {
    /// Whether to activate (true) or deactivate (false)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,
}

/// Backing track file stored for a studio
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct BackingTrack {
    /// Track ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Studio ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,

    /// User ID of the track owner
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,

    /// GCS location of the backing track file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,

    /// Track display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Track duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<i32>,

    /// Track status (0=ready, 1=deleting)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,

    /// Upload timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// Last modification timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_utils::roundtrip;

    #[test]
    fn stream_info_fixture_with_preserved_banner_url() {
        // Fixture modeled after docs/api/streams.md. `bannerURL` keeps its
        // uppercase casing on the wire.
        let json = r#"{
          "id": "stream-1",
          "name": "My Stream",
          "description": "Live jam",
          "serverName": "My Studio",
          "metaUrl": "https://hls.example.com/x.m3u8",
          "chatId": "chat-1",
          "bannerURL": "https://cdn.example.com/banner.png"
        }"#;
        let s: StreamInfo = serde_json::from_str(json).unwrap();
        assert_eq!(s.banner_url.as_deref(), Some("https://cdn.example.com/banner.png"));
        assert_eq!(s.server_name.as_deref(), Some("My Studio"));
        assert_eq!(s.meta_url.as_deref(), Some("https://hls.example.com/x.m3u8"));

        let out = serde_json::to_string(&s).unwrap();
        assert!(out.contains("\"bannerURL\":"));
        assert!(out.contains("\"serverName\":"));
        assert!(!out.contains("\"bannerUrl\":"));
    }

    #[test]
    fn stream_info_with_engagement_roundtrip() {
        let s = StreamInfoWithEngagement {
            base: StreamInfo {
                id: Some("s1".into()),
                name: Some("Name".into()),
                server_name: Some("studio".into()),
                banner_url: Some("https://b".into()),
                ..Default::default()
            },
            viewers: Some(42),
            followers: Some(1000),
            following: Some(true),
        };
        let out = roundtrip(&s);
        assert!(out.contains("\"viewers\":42"));
        assert!(out.contains("\"followers\":1000"));
        assert!(out.contains("\"bannerURL\":"));
    }

    #[test]
    fn stream_info_search_result_flattens_base() {
        let json = r#"{
          "id": "stream-1",
          "name": "Jazz Quartet",
          "bannerURL": "https://cdn.example.com/banner.png",
          "serverId": "studio-1",
          "region": "ec2-us-north-ca",
          "lookingFor": 2,
          "skillLevels": ["intermediate"],
          "instruments": ["sax"],
          "genres": ["jazz"],
          "public": true,
          "isRecruiting": true,
          "numPublicRecordings": 3
        }"#;
        let r: StreamInfoSearchResult = serde_json::from_str(json).unwrap();
        // Flatten means StreamInfo fields are at the top level.
        assert_eq!(r.base.id.as_deref(), Some("stream-1"));
        assert_eq!(r.server_id.as_deref(), Some("studio-1"));
        assert_eq!(r.looking_for, Some(2));
        assert_eq!(r.num_public_recordings, Some(3));

        let out = roundtrip(&r);
        assert!(out.contains("\"serverId\":"));
        assert!(out.contains("\"lookingFor\":"));
        assert!(out.contains("\"bannerURL\":"));
        assert!(!out.contains("\"base\":"));
    }

    #[test]
    fn backing_track_fixture_known_good() {
        let json = r#"{
          "id": "trk-1",
          "serverId": "studio-1",
          "ownerId": "user-1",
          "location": "gs://bucket/trk-1.wav",
          "name": "Drum Loop",
          "duration": 120,
          "status": 0,
          "createdAt": "2026-06-14T00:00:00Z",
          "updatedAt": "2026-06-14T01:00:00Z"
        }"#;
        let t: BackingTrack = serde_json::from_str(json).unwrap();
        assert_eq!(t.id.as_deref(), Some("trk-1"));
        assert_eq!(t.server_id.as_deref(), Some("studio-1"));
        assert_eq!(t.duration, Some(120));
        assert_eq!(t.status, Some(0));

        let out = roundtrip(&t);
        assert!(out.contains("\"serverId\":"));
        assert!(out.contains("\"ownerId\":"));
        assert!(out.contains("\"createdAt\":"));
    }
}
