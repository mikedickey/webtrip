//! Pagination models

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use super::{PublicUpcomingEvent, RecordingMetadata, StreamInfo};

/// Generic paginated response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedResponse<T> {
    /// Items in this page
    pub items: Vec<T>,

    /// Current page number (1-indexed)
    pub page: i32,

    /// Items per page
    pub limit: i32,

    /// Total number of items
    pub total: i32,

    /// Total number of pages
    pub total_pages: i32,

    /// Whether there is a next page
    pub has_next: bool,

    /// Whether there is a previous page
    pub has_prev: bool,
}

/// Paginated channels/streams response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedChannels {
    /// Channels in this page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<StreamInfo>>,

    /// Current page number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<i32>,

    /// Items per page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,

    /// Total number of items
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<i32>,

    /// Total number of pages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<i32>,

    /// Whether there is a next page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_next: Option<bool>,

    /// Whether there is a previous page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_prev: Option<bool>,
}

/// Paginated events response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedEvents {
    /// Events in this page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<PublicUpcomingEvent>>,

    /// Current page number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<i32>,

    /// Items per page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,

    /// Total number of items
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<i32>,

    /// Total number of pages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<i32>,

    /// Whether there is a next page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_next: Option<bool>,

    /// Whether there is a previous page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_prev: Option<bool>,
}

/// Paginated recordings response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedRecordings {
    /// Recordings in this page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<RecordingMetadata>>,

    /// Current page number
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<i32>,

    /// Items per page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,

    /// Total number of items
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<i32>,

    /// Total number of pages
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<i32>,

    /// Whether there is a next page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_next: Option<bool>,

    /// Whether there is a previous page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_prev: Option<bool>,
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
    fn paginated_response_generic_with_stream_info() {
        let p = PaginatedResponse {
            items: vec![StreamInfo {
                id: Some("s1".into()),
                name: Some("name".into()),
                ..Default::default()
            }],
            page: 1,
            limit: 20,
            total: 1,
            total_pages: 1,
            has_next: false,
            has_prev: false,
        };
        let s = roundtrip(&p);
        assert!(s.contains("\"items\":"));
        assert!(s.contains("\"totalPages\":1"));
        assert!(s.contains("\"hasNext\":false"));
        assert!(s.contains("\"hasPrev\":false"));
    }

    #[test]
    fn paginated_channels_roundtrip() {
        let p = PaginatedChannels {
            items: Some(vec![StreamInfo {
                id: Some("c1".into()),
                ..Default::default()
            }]),
            page: Some(2),
            limit: Some(50),
            total: Some(120),
            total_pages: Some(3),
            has_next: Some(true),
            has_prev: Some(true),
        };
        let s = roundtrip(&p);
        assert!(s.contains("\"hasNext\":true"));
        assert!(s.contains("\"totalPages\":3"));
    }

    #[test]
    fn paginated_events_roundtrip() {
        let e = PaginatedEvents {
            items: Some(vec![PublicUpcomingEvent {
                id: Some("e1".into()),
                title: Some("Show".into()),
                ..Default::default()
            }]),
            page: Some(1),
            limit: Some(10),
            total: Some(1),
            total_pages: Some(1),
            has_next: Some(false),
            has_prev: Some(false),
        };
        roundtrip(&e);
    }

    #[test]
    fn paginated_recordings_roundtrip() {
        let r = PaginatedRecordings {
            items: Some(vec![RecordingMetadata {
                id: Some("r1".into()),
                ..Default::default()
            }]),
            page: Some(1),
            limit: Some(10),
            total: Some(1),
            total_pages: Some(1),
            has_next: Some(false),
            has_prev: Some(false),
        };
        roundtrip(&r);
    }
}

