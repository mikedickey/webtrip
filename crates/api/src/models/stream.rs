//! Stream and broadcast models

use super::BroadcastVisibility;
use serde::{Deserialize, Serialize};
#[cfg(feature = "wasm")]
use tsify_next::Tsify;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// Public stream/channel information
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
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

/// A [`StreamInfo`] with search-specific flags, returned by
/// [`StreamsApi::search_streams`](crate::api::streams::StreamsApi::search_streams).
/// The [`base`](Self::base) fields are flattened to the top level of the JSON
/// object.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
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

    /// Studio looking-for status (see [`Server::looking_for`](super::Server::looking_for))
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

/// A [`StreamInfo`] with studio details and engagement counts, returned by
/// [`StreamsApi::get_stream`](crate::api::streams::StreamsApi::get_stream).
/// The [`base`](Self::base) fields are flattened.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct StreamInfoWithEngagement {
    /// Base stream information
    #[serde(flatten)]
    pub base: StreamInfo,

    /// Studio ID; present only if the caller has access to the studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,

    /// Cloud region identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Studio looking-for status (see [`Server::looking_for`](super::Server::looking_for))
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looking_for: Option<i32>,

    /// Skill levels the studio is targeted at
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_levels: Option<Vec<String>>,

    /// Instruments associated with the studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruments: Option<Vec<String>>,

    /// Genres associated with the studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genres: Option<Vec<String>>,

    /// Whether the studio is publicly accessible
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public: Option<bool>,

    /// Number of broadcast views
    #[serde(skip_serializing_if = "Option::is_none")]
    pub views: Option<i64>,

    /// Number of broadcast likes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub likes: Option<i64>,

    /// Whether the caller has liked this broadcast
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liked: Option<bool>,

    /// Whether the caller follows this stream
    #[serde(skip_serializing_if = "Option::is_none")]
    pub followed: Option<bool>,
}

/// A [`StreamInfo`] with the details needed to reach the studio serving it,
/// returned by [`EventsApi::get_event_channel`](crate::api::events::EventsApi::get_event_channel).
/// The [`base`](Self::base) fields are flattened.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct StreamCacheEntry {
    /// Base stream information
    #[serde(flatten)]
    pub base: StreamInfo,

    /// Studio ID backing this stream
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,

    /// Cloud region identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Hostname of the studio server serving the stream
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,

    /// Whether JackTrip manages the studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed: Option<bool>,

    /// The studio's current session ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Token for accessing the studio server in this session
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,

    /// Studio looking-for status (see [`Server::looking_for`](super::Server::looking_for))
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looking_for: Option<i32>,

    /// Skill levels the studio is targeted at
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_levels: Option<Vec<String>>,

    /// Instruments associated with the studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instruments: Option<Vec<String>>,

    /// Genres associated with the studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genres: Option<Vec<String>>,
}

// Hand-written so the auth token never reaches logs.
impl std::fmt::Debug for StreamCacheEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamCacheEntry")
            .field("base", &self.base)
            .field("server_id", &self.server_id)
            .field("region", &self.region)
            .field("host", &self.host)
            .field("managed", &self.managed)
            .field("session_id", &self.session_id)
            .field("auth_token", &self.auth_token.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

/// Query options for `GET /streams/search`; every field is optional and
/// omitted from the request when `None`.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct StreamSearchQuery {
    /// Free-text search term
    #[serde(skip_serializing_if = "Option::is_none")]
    pub q: Option<String>,

    /// Looking-for status filter, using the codes of
    /// [`Server::looking_for`](super::Server::looking_for)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looking_for: Option<i32>,

    /// Skill level filter, e.g. `"beginner"`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_level: Option<String>,

    /// Instrument filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrument: Option<String>,

    /// Genre filter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,

    /// Cloud region filter (a region ID such as `ec2-us-oregon`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Page number (1-indexed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<i32>,

    /// Maximum number of items per page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
}

/// A studio's JackTrip Radio live stream (`/studios/{studioId}/stream`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct LiveStream {
    /// Stream ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Studio ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,

    /// Session ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// ID of the recording made from this stream
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_id: Option<String>,

    /// Studio owner's user ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,

    /// Auth token
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,

    /// Whether the stream is ephemeral
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ephemeral: Option<bool>,

    /// Stream name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Stream description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Cumulative follower count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub follows: Option<i64>,

    /// When the stream started (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,

    /// When the stream was last updated (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

// Hand-written so the auth token never reaches logs.
impl std::fmt::Debug for LiveStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveStream")
            .field("id", &self.id)
            .field("server_id", &self.server_id)
            .field("session_id", &self.session_id)
            .field("name", &self.name)
            .field("auth_token", &self.auth_token.as_ref().map(|_| "<redacted>"))
            .finish_non_exhaustive()
    }
}

/// A simulcast destination: an RTMP endpoint the studio's broadcast is
/// re-published to. One per platform, identified by [`name`](Self::name).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct SimulcastDestination {
    /// Platform: `"youtube"`, `"facebook"` or `"twitch"`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// RTMP ingest URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Stream key used when publishing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

// Hand-written so the stream key never reaches logs.
impl std::fmt::Debug for SimulcastDestination {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimulcastDestination")
            .field("name", &self.name)
            .field("url", &self.url)
            .field("key", &self.key.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

/// Activation body (`POST /studios/{studioId}/stream/activate`): what the
/// studio should broadcast or record.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct ActivationRequestOpts {
    /// Broadcast or recording mode; [`Offline`](BroadcastVisibility::Offline)
    /// stops it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<BroadcastVisibility>,

    /// Session ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

/// Backing track file stored for a studio
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
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
        // Fixture modeled after the `api::streams` module docs. `bannerURL` keeps its
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
    fn live_stream_simulcast_and_activation_wire_format() {
        // Regression: these used invented fields (`rtmpUrl`, `streamKey`,
        // `hlsUrl`, `active`) that the API neither sends nor reads.
        let ls = serde_json::json!({
            "id": "st-1", "serverId": "studio-1", "sessionId": "sess", "recordingId": "rec",
            "ownerId": "u1", "ephemeral": false, "name": "Live", "follows": 3, "startTime": "t"
        });
        let parsed: LiveStream = serde_json::from_value(ls.clone()).unwrap();
        assert_eq!(parsed.server_id.as_deref(), Some("studio-1"));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), ls);

        let d = SimulcastDestination { name: Some("youtube".into()), url: Some("rtmp://x".into()), key: Some("k".into()) };
        assert_eq!(
            serde_json::to_value(&d).unwrap(),
            serde_json::json!({"name": "youtube", "url": "rtmp://x", "key": "k"})
        );
        assert!(!format!("{d:?}").contains("\"k\""));

        let a = ActivationRequestOpts { visibility: Some(BroadcastVisibility::Unlisted), session_id: Some("sess".into()) };
        assert_eq!(serde_json::to_value(&a).unwrap(), serde_json::json!({"visibility": 2, "sessionId": "sess"}));
    }

    #[test]
    fn engagement_and_cache_entry_wire_format() {
        let e = serde_json::json!({
            "id": "s1", "serverId": "st1", "lookingFor": 1, "views": 9, "likes": 2,
            "liked": true, "followed": false
        });
        let parsed: StreamInfoWithEngagement = serde_json::from_value(e.clone()).unwrap();
        assert_eq!(parsed.base.id.as_deref(), Some("s1"));
        assert_eq!(parsed.views, Some(9));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), e);

        let c = serde_json::json!({"id": "s1", "host": "h.example.com", "managed": true, "authToken": "s3cr3t"});
        let parsed: StreamCacheEntry = serde_json::from_value(c.clone()).unwrap();
        assert_eq!(parsed.host.as_deref(), Some("h.example.com"));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), c);
        assert!(!format!("{parsed:?}").contains("s3cr3t"));
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
