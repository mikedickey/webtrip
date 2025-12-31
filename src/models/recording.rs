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

