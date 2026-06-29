//! Recording models

use super::{RecordingStatus, Visibility};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Recording metadata
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct RecordingMetadata {
    /// Recording ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Recording name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Recording description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Media file location/URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,

    /// Stream/channel ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,

    /// Studio/channel name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,

    /// Studio banner image URL of the studio that produced the recording
    #[serde(rename = "serverBannerURL", skip_serializing_if = "Option::is_none")]
    pub server_banner_url: Option<String>,

    /// Thumbnail image URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// View count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub views: Option<i32>,

    /// Like count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub likes: Option<i32>,

    /// Start offset in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_offset: Option<i32>,

    /// Processing status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<RecordingStatus>,

    /// Visibility setting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,

    /// Creation timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// Last update timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Recording with personalized user data
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PersonalizedRecording {
    /// Recording metadata
    #[serde(flatten)]
    pub metadata: RecordingMetadata,

    /// Whether the current user has liked this recording
    #[serde(skip_serializing_if = "Option::is_none")]
    pub liked: Option<bool>,

    /// Whether the current user follows the channel
    #[serde(skip_serializing_if = "Option::is_none")]
    pub following: Option<bool>,
}

/// Server-side recording with additional studio/session identifiers.
///
/// Spec composes this as `RecordingMetadata` + `{serverId, sessionId, ownerId}`.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ServerRecording {
    /// Recording metadata
    #[serde(flatten)]
    pub metadata: RecordingMetadata,

    /// Studio identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,

    /// Session identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,

    /// Studio owner user identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
}

/// Signed download URL returned by
/// `GET /studios/{studioId}/recordings/{recordingId}/download`.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct RecordingDownload {
    /// Signed GCS URL to download the recording file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// Stem track summary returned by
/// `GET /studios/{studioId}/recordings/{recordingId}/stems`.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct StemSummary {
    /// List of stem client tracks
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clients: Option<Vec<StemClient>>,
}

/// A single stem client track within a [`StemSummary`].
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct StemClient {
    /// Client track ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i32>,

    /// Client track display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Stem filename in storage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
}

/// User recordings quota, returned by `GET /users/{userId}/recordings/quota`.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct RecordingsQuota {
    /// Private recording quota usage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_recordings: Option<PrivateRecordingsQuota>,
}

/// Private recording counts within a [`RecordingsQuota`].
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PrivateRecordingsQuota {
    /// Current number of private recordings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,

    /// Maximum allowed private recordings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_utils::roundtrip;

    #[test]
    fn recording_metadata_fixture_known_good() {
        // Fixture modeled after docs/api/recordings.md.
        let json = r#"{
          "id": "rec-1",
          "name": "Last Night's Show",
          "description": "Live recording",
          "location": "https://s3.example.com/rec-1.mp4",
          "streamId": "stream-1",
          "serverName": "My Studio",
          "serverBannerURL": "https://cdn.example.com/banner.png",
          "image": "https://cdn.example.com/thumb.png",
          "views": 42,
          "likes": 7,
          "startOffset": 0,
          "status": 2,
          "visibility": 1,
          "createdAt": "2026-06-14T01:00:00Z",
          "updatedAt": "2026-06-14T02:00:00Z"
        }"#;
        let r: RecordingMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(r.status, Some(RecordingStatus::Ready));
        assert_eq!(r.visibility, Some(Visibility::Public));
        assert_eq!(r.views, Some(42));

        assert_eq!(r.server_banner_url.as_deref(), Some("https://cdn.example.com/banner.png"));

        let out = serde_json::to_string(&r).unwrap();
        assert!(out.contains("\"streamId\":"));
        assert!(out.contains("\"startOffset\":"));
        assert!(out.contains("\"status\":2"));
        assert!(out.contains("\"visibility\":1"));
        // serverBannerURL uses the spec's exact (non-camelCase) casing.
        assert!(out.contains("\"serverBannerURL\":"));
    }

    #[test]
    fn personalized_recording_flattens_metadata() {
        let p = PersonalizedRecording {
            metadata: RecordingMetadata {
                id: Some("r1".into()),
                name: Some("Show".into()),
                status: Some(RecordingStatus::Processing),
                ..Default::default()
            },
            liked: Some(true),
            following: Some(false),
        };
        let s = roundtrip(&p);
        // Flatten means the metadata fields appear at the top level alongside
        // `liked`/`following`, NOT nested under `metadata`.
        assert!(s.contains("\"id\":\"r1\""));
        assert!(s.contains("\"status\":1"));
        assert!(s.contains("\"liked\":true"));
        assert!(s.contains("\"following\":false"));
        assert!(!s.contains("\"metadata\":"));
    }

    #[test]
    fn server_recording_flattens_metadata() {
        let r = ServerRecording {
            metadata: RecordingMetadata { id: Some("r1".into()), ..Default::default() },
            server_id: Some("s1".into()),
            session_id: Some("sess1".into()),
            owner_id: Some("u1".into()),
        };
        let s = roundtrip(&r);
        // Flatten means metadata fields are at top level, not nested under "metadata".
        assert!(s.contains("\"id\":\"r1\""));
        assert!(s.contains("\"serverId\":\"s1\""));
        assert!(s.contains("\"sessionId\":\"sess1\""));
        assert!(s.contains("\"ownerId\":\"u1\""));
        assert!(!s.contains("\"metadata\":"));
    }

    #[test]
    fn recording_download_roundtrip() {
        let json = r#"{"url":"https://storage.example.com/signed?token=abc"}"#;
        let d: RecordingDownload = serde_json::from_str(json).unwrap();
        assert_eq!(d.url.as_deref(), Some("https://storage.example.com/signed?token=abc"));
        let out = roundtrip(&d);
        assert!(out.contains("\"url\":\"https://storage.example.com/signed?token=abc\""));
    }

    #[test]
    fn recordings_quota_nests_private_recordings() {
        let json = r#"{"privateRecordings":{"count":3,"limit":10}}"#;
        let q: RecordingsQuota = serde_json::from_str(json).unwrap();
        let private = q.private_recordings.as_ref().expect("privateRecordings present");
        assert_eq!(private.count, Some(3));
        assert_eq!(private.limit, Some(10));
        let out = roundtrip(&q);
        assert!(out.contains("\"privateRecordings\":"));
        assert!(out.contains("\"count\":3"));
        assert!(out.contains("\"limit\":10"));
    }

    #[test]
    fn stem_summary_fixture_known_good() {
        let json = r#"{
          "clients": [
            {"id": 1, "name": "vocals", "filename": "stem-1.wav"},
            {"id": 2, "name": "guitar", "filename": "stem-2.wav"}
          ]
        }"#;
        let s: StemSummary = serde_json::from_str(json).unwrap();
        let clients = s.clients.as_ref().expect("clients present");
        assert_eq!(clients.len(), 2);
        assert_eq!(clients[0].id, Some(1));
        assert_eq!(clients[0].name.as_deref(), Some("vocals"));
        assert_eq!(clients[1].filename.as_deref(), Some("stem-2.wav"));

        let out = roundtrip(&s);
        assert!(out.contains("\"clients\":"));
        assert!(out.contains("\"filename\":"));
    }
}

