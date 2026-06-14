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

    /// Studio display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,

    /// HLS metadata URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta_url: Option<String>,

    /// Chat room ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,

    /// Banner image URL
    #[serde(rename = "bannerURL", skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,
}

/// Stream info with engagement metrics
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfoWithEngagement {
    /// Stream ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Stream name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Stream description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Studio display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,

    /// HLS metadata URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta_url: Option<String>,

    /// Chat room ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_id: Option<String>,

    /// Banner image URL
    #[serde(rename = "bannerURL", skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,

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

/// Backing track for studio playback
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct BackingTrack {
    /// Track ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Track name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Audio file URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    /// Duration in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,

    /// Whether the track is currently playing
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playing: Option<bool>,

    /// Current playback position in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<f64>,

    /// Whether the track loops
    #[serde(skip_serializing_if = "Option::is_none")]
    pub looping: Option<bool>,

    /// Volume level (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume: Option<u32>,
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
            id: Some("s1".into()),
            name: Some("Name".into()),
            description: None,
            server_name: Some("studio".into()),
            meta_url: None,
            chat_id: None,
            banner_url: Some("https://b".into()),
            viewers: Some(42),
            followers: Some(1000),
            following: Some(true),
        };
        let out = roundtrip(&s);
        assert!(out.contains("\"viewers\":42"));
        assert!(out.contains("\"followers\":1000"));
        assert!(out.contains("\"bannerURL\":"));
    }
}
