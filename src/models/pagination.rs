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

/// Pagination metadata shared across paginated response types.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PageMeta {
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

/// Paginated channels/streams response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedChannels {
    /// Channels in this page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<StreamInfo>>,

    /// Pagination metadata
    #[serde(flatten)]
    pub meta: PageMeta,
}

/// Paginated events response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedEvents {
    /// Events in this page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<PublicUpcomingEvent>>,

    /// Pagination metadata
    #[serde(flatten)]
    pub meta: PageMeta,
}

/// Paginated recordings response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PaginatedRecordings {
    /// Recordings in this page
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<RecordingMetadata>>,

    /// Pagination metadata
    #[serde(flatten)]
    pub meta: PageMeta,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_utils::roundtrip;

    // ---- PaginatedResponse<T> ----

    #[test]
    fn paginated_response_fixture_known_good() {
        // Realistic fixture for the generic type using strings as the item type.
        let json = r#"{
          "items": ["alpha", "beta", "gamma"],
          "page": 2,
          "limit": 3,
          "total": 9,
          "totalPages": 3,
          "hasNext": true,
          "hasPrev": true
        }"#;
        let r: PaginatedResponse<String> = serde_json::from_str(json).unwrap();
        assert_eq!(r.items.len(), 3);
        assert_eq!(r.items[0], "alpha");
        assert_eq!(r.page, 2);
        assert_eq!(r.total_pages, 3);
        assert_eq!(r.has_next, true);
        assert_eq!(r.has_prev, true);

        let s = serde_json::to_string(&r).unwrap();
        // camelCase wire keys must be present
        assert!(s.contains("\"totalPages\":"));
        assert!(s.contains("\"hasNext\":"));
        assert!(s.contains("\"hasPrev\":"));
        // snake_case must NOT appear on the wire
        assert!(!s.contains("\"total_pages\":"));
        assert!(!s.contains("\"has_next\":"));
        assert!(!s.contains("\"has_prev\":"));
    }

    #[test]
    fn paginated_response_empty_items() {
        // Edge case: empty page (total=0).
        let r: PaginatedResponse<String> = PaginatedResponse {
            items: vec![],
            page: 1,
            limit: 10,
            total: 0,
            total_pages: 0,
            has_next: false,
            has_prev: false,
        };
        let s = roundtrip(&r);
        assert!(s.contains("\"items\":[]"));
        assert!(s.contains("\"total\":0"));
        assert!(s.contains("\"hasNext\":false"));
        assert!(s.contains("\"hasPrev\":false"));
    }

    #[test]
    fn paginated_response_single_page() {
        // Single page: first and only page — no next, no prev.
        let r: PaginatedResponse<i32> = PaginatedResponse {
            items: vec![1, 2, 3],
            page: 1,
            limit: 10,
            total: 3,
            total_pages: 1,
            has_next: false,
            has_prev: false,
        };
        let s = roundtrip(&r);
        assert!(s.contains("\"page\":1"));
        assert!(s.contains("\"totalPages\":1"));
        assert!(s.contains("\"hasNext\":false"));
        assert!(s.contains("\"hasPrev\":false"));
    }

    #[test]
    fn paginated_response_last_page() {
        // Last page of a multi-page result: hasPrev=true, hasNext=false.
        let r: PaginatedResponse<String> = PaginatedResponse {
            items: vec!["x".into()],
            page: 3,
            limit: 10,
            total: 21,
            total_pages: 3,
            has_next: false,
            has_prev: true,
        };
        let s = roundtrip(&r);
        assert!(s.contains("\"page\":3"));
        assert!(s.contains("\"totalPages\":3"));
        assert!(s.contains("\"hasNext\":false"));
        assert!(s.contains("\"hasPrev\":true"));
    }

    // ---- PageMeta ----

    #[test]
    fn page_meta_skips_none_fields() {
        // All fields None → serializes to an empty object.
        let m = PageMeta::default();
        let s = roundtrip(&m);
        assert_eq!(s, "{}");
    }

    #[test]
    fn page_meta_partial_fields_camelcase() {
        // Only some fields populated; None fields must be absent from the wire.
        let m = PageMeta {
            page: Some(2),
            limit: Some(20),
            total: None,
            total_pages: Some(5),
            has_next: Some(true),
            has_prev: Some(true),
        };
        let s = roundtrip(&m);
        assert!(s.contains("\"totalPages\":5"));
        assert!(s.contains("\"hasNext\":true"));
        assert!(s.contains("\"hasPrev\":true"));
        // total was None → must be absent
        assert!(!s.contains("\"total\":"));
    }

    // ---- PaginatedChannels ----

    #[test]
    fn paginated_channels_fixture_known_good() {
        // Realistic fixture mirroring a paginated streams/channels response.
        // bannerURL preserves its uppercase casing (see StreamInfo).
        let json = r#"{
          "items": [
            {
              "id": "stream-1",
              "name": "Live Jam",
              "description": "Weekly jam session",
              "serverName": "Studio A",
              "metaUrl": "https://hls.example.com/x.m3u8",
              "chatId": "chat-1",
              "bannerURL": "https://cdn.example.com/banner.png"
            }
          ],
          "page": 1,
          "limit": 10,
          "total": 1,
          "totalPages": 1,
          "hasNext": false,
          "hasPrev": false
        }"#;
        let p: PaginatedChannels = serde_json::from_str(json).unwrap();
        let items = p.items.as_ref().expect("items should be Some");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id.as_deref(), Some("stream-1"));
        assert_eq!(items[0].server_name.as_deref(), Some("Studio A"));
        assert_eq!(p.meta.page, Some(1));
        assert_eq!(p.meta.has_next, Some(false));
        assert_eq!(p.meta.has_prev, Some(false));

        let s = roundtrip(&p);
        assert!(s.contains("\"hasNext\":"));
        assert!(s.contains("\"hasPrev\":"));
        assert!(s.contains("\"totalPages\":"));
        assert!(s.contains("\"bannerURL\":"));
    }

    #[test]
    fn paginated_channels_none_items() {
        // items absent from the server response → None, and must be omitted on re-serialization.
        let p = PaginatedChannels {
            items: None,
            meta: PageMeta {
                page: Some(1),
                limit: Some(10),
                total: Some(0),
                total_pages: Some(0),
                has_next: Some(false),
                has_prev: Some(false),
            },
        };
        let s = roundtrip(&p);
        assert!(!s.contains("\"items\":"));
        assert!(s.contains("\"hasNext\":false"));
        assert!(s.contains("\"hasPrev\":false"));
    }

    // ---- PaginatedEvents ----

    #[test]
    fn paginated_events_fixture_known_good() {
        // Realistic fixture mirroring a paginated upcoming events response.
        let json = r#"{
          "items": [
            {
              "id": "evt-1",
              "title": "Friday Night Jam",
              "streamId": "stream-1",
              "streamName": "Studio Live",
              "startTime": "2026-07-04T01:00:00Z",
              "endTime": "2026-07-04T03:00:00Z",
              "timezone": "America/New_York",
              "recurring": false
            }
          ],
          "page": 1,
          "limit": 20,
          "total": 1,
          "totalPages": 1,
          "hasNext": false,
          "hasPrev": false
        }"#;
        let p: PaginatedEvents = serde_json::from_str(json).unwrap();
        let items = p.items.as_ref().expect("items should be Some");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].core.id.as_deref(), Some("evt-1"));
        assert_eq!(items[0].stream_name.as_deref(), Some("Studio Live"));
        assert_eq!(p.meta.total, Some(1));
        assert_eq!(p.meta.has_next, Some(false));

        let s = roundtrip(&p);
        assert!(s.contains("\"hasNext\":"));
        assert!(s.contains("\"totalPages\":"));
        assert!(s.contains("\"streamId\":"));
    }

    #[test]
    fn paginated_events_empty_items() {
        // Edge case: Some(vec![]) — list present but no events on this page.
        let p = PaginatedEvents {
            items: Some(vec![]),
            meta: PageMeta {
                page: Some(1),
                limit: Some(20),
                total: Some(0),
                total_pages: Some(0),
                has_next: Some(false),
                has_prev: Some(false),
            },
        };
        let s = roundtrip(&p);
        assert!(s.contains("\"items\":[]"));
        assert!(s.contains("\"hasNext\":false"));
        assert!(s.contains("\"hasPrev\":false"));
    }

    // ---- PaginatedRecordings ----

    #[test]
    fn paginated_recordings_fixture_known_good() {
        // Realistic fixture mirroring a paginated recordings response.
        let json = r#"{
          "items": [
            {
              "id": "rec-1",
              "name": "Last Night's Show",
              "streamId": "stream-1",
              "serverName": "My Studio",
              "views": 10,
              "status": 2,
              "visibility": 1,
              "createdAt": "2026-06-14T01:00:00Z"
            }
          ],
          "page": 1,
          "limit": 10,
          "total": 5,
          "totalPages": 1,
          "hasNext": false,
          "hasPrev": false
        }"#;
        let p: PaginatedRecordings = serde_json::from_str(json).unwrap();
        let items = p.items.as_ref().expect("items should be Some");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id.as_deref(), Some("rec-1"));
        assert_eq!(items[0].views, Some(10));
        assert_eq!(p.meta.total, Some(5));
        assert_eq!(p.meta.total_pages, Some(1));

        let s = roundtrip(&p);
        assert!(s.contains("\"hasNext\":"));
        assert!(s.contains("\"hasPrev\":"));
        assert!(s.contains("\"totalPages\":"));
        assert!(s.contains("\"streamId\":"));
    }

    #[test]
    fn paginated_recordings_last_page() {
        // Multi-page response on the last page: hasPrev=true, hasNext=false.
        let p = PaginatedRecordings {
            items: Some(vec![RecordingMetadata {
                id: Some("rec-42".into()),
                name: Some("Final Recording".into()),
                ..Default::default()
            }]),
            meta: PageMeta {
                page: Some(5),
                limit: Some(10),
                total: Some(42),
                total_pages: Some(5),
                has_next: Some(false),
                has_prev: Some(true),
            },
        };
        let s = roundtrip(&p);
        assert!(s.contains("\"hasNext\":false"));
        assert!(s.contains("\"hasPrev\":true"));
        assert!(s.contains("\"page\":5"));
        assert!(s.contains("\"totalPages\":5"));
    }
}
