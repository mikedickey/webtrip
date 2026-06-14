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

/// Server-side recording with additional fields
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct ServerRecording {
    /// Recording metadata
    #[serde(flatten)]
    pub metadata: RecordingMetadata,

    /// Studio ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studio_id: Option<String>,

    /// Duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,

    /// File size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size: Option<i64>,

    /// Whether stems are available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_stems: Option<bool>,
}

/// Recording stem (individual track) information
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct StemInfo {
    /// Stem ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Stem name (typically participant name)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Audio file URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,

    /// File size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size: Option<i64>,
}

/// User recordings storage quota
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct RecordingsQuota {
    /// Total storage used in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub used: Option<i64>,

    /// Total storage limit in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,

    /// Number of recordings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i32>,
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
    fn recording_metadata_fixture_known_good() {
        // Fixture modeled after docs/api/recordings.md.
        let json = r#"{
          "id": "rec-1",
          "name": "Last Night's Show",
          "description": "Live recording",
          "location": "https://s3.example.com/rec-1.mp4",
          "streamId": "stream-1",
          "serverName": "My Studio",
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

        let out = serde_json::to_string(&r).unwrap();
        assert!(out.contains("\"streamId\":"));
        assert!(out.contains("\"startOffset\":"));
        assert!(out.contains("\"status\":2"));
        assert!(out.contains("\"visibility\":1"));
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
            studio_id: Some("s1".into()),
            duration: Some(360.5),
            file_size: Some(1_234_567_890),
            has_stems: Some(true),
        };
        let s = roundtrip(&r);
        assert!(s.contains("\"studioId\":\"s1\""));
        assert!(s.contains("\"hasStems\":true"));
        assert!(s.contains("\"fileSize\":1234567890"));
        assert!(!s.contains("\"metadata\":"));
    }
}

