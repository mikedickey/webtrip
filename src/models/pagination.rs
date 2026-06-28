//! Pagination models
//!
//! The API wraps every paginated list in a `{ _meta, results }` envelope, where
//! `_meta` carries [`PaginationMeta`] and `results` is a typed array.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

use super::{BackingTrack, PublicUpcomingEvent, RecordingMetadata, StreamInfo, StreamInfoSearchResult};

/// Pagination metadata returned in the `_meta` field of every paginated response.
///
/// All fields are required per the spec.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PaginationMeta {
    /// Total number of items that match the query criteria
    pub total: i32,

    /// Total number of pages
    pub pages: i32,

    /// Current page number (1-indexed)
    pub current: i32,

    /// Number of items returned on this page
    pub count: i32,

    /// Maximum number of items per page
    pub limit: i32,
}

/// Generic paginated response envelope: `{ _meta, results }`.
///
/// Concrete paginated types (e.g. [`PaginatedChannels`]) mirror this shape with
/// a typed `results` array.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct PaginatedResponse<T> {
    /// Pagination metadata
    #[serde(rename = "_meta")]
    pub meta: PaginationMeta,

    /// Items on the current page
    pub results: Vec<T>,
}

/// Paginated channels/streams response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct PaginatedChannels {
    /// Pagination metadata
    #[serde(rename = "_meta")]
    pub meta: PaginationMeta,

    /// Channels on the current page
    pub results: Vec<StreamInfo>,
}

/// Paginated events response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct PaginatedEvents {
    /// Pagination metadata
    #[serde(rename = "_meta")]
    pub meta: PaginationMeta,

    /// Events on the current page
    pub results: Vec<PublicUpcomingEvent>,
}

/// Paginated recordings response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct PaginatedRecordings {
    /// Pagination metadata
    #[serde(rename = "_meta")]
    pub meta: PaginationMeta,

    /// Recordings on the current page
    pub results: Vec<RecordingMetadata>,
}

/// Paginated stream search results response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct PaginatedStreamSearchResults {
    /// Pagination metadata
    #[serde(rename = "_meta")]
    pub meta: PaginationMeta,

    /// Search results on the current page
    pub results: Vec<StreamInfoSearchResult>,
}

/// Paginated backing tracks response
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct PaginatedBackingTracks {
    /// Pagination metadata
    #[serde(rename = "_meta")]
    pub meta: PaginationMeta,

    /// Backing tracks on the current page
    pub results: Vec<BackingTrack>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_utils::roundtrip;

    fn meta_fixture() -> PaginationMeta {
        PaginationMeta { total: 9, pages: 3, current: 2, count: 3, limit: 3 }
    }

    // ---- PaginationMeta ----

    #[test]
    fn pagination_meta_fixture_known_good() {
        let json = r#"{
          "total": 42,
          "pages": 5,
          "current": 2,
          "count": 10,
          "limit": 10
        }"#;
        let m: PaginationMeta = serde_json::from_str(json).unwrap();
        assert_eq!(m.total, 42);
        assert_eq!(m.pages, 5);
        assert_eq!(m.current, 2);
        assert_eq!(m.count, 10);
        assert_eq!(m.limit, 10);

        let s = roundtrip(&m);
        assert!(s.contains("\"total\":42"));
        assert!(s.contains("\"pages\":5"));
        assert!(s.contains("\"current\":2"));
        assert!(s.contains("\"count\":10"));
        assert!(s.contains("\"limit\":10"));
    }

    // ---- PaginatedResponse<T> ----

    #[test]
    fn paginated_response_fixture_known_good() {
        let json = r#"{
          "_meta": {"total": 9, "pages": 3, "current": 2, "count": 3, "limit": 3},
          "results": ["alpha", "beta", "gamma"]
        }"#;
        let r: PaginatedResponse<String> = serde_json::from_str(json).unwrap();
        assert_eq!(r.results.len(), 3);
        assert_eq!(r.results[0], "alpha");
        assert_eq!(r.meta.current, 2);
        assert_eq!(r.meta.pages, 3);

        let s = serde_json::to_string(&r).unwrap();
        // Envelope keys must be present verbatim.
        assert!(s.contains("\"_meta\":"));
        assert!(s.contains("\"results\":"));
    }

    #[test]
    fn paginated_response_empty_results() {
        let r: PaginatedResponse<String> = PaginatedResponse {
            meta: PaginationMeta { total: 0, pages: 0, current: 1, count: 0, limit: 10 },
            results: vec![],
        };
        let s = roundtrip(&r);
        assert!(s.contains("\"results\":[]"));
        assert!(s.contains("\"total\":0"));
    }

    // ---- PaginatedChannels ----

    #[test]
    fn paginated_channels_fixture_known_good() {
        // bannerURL preserves its uppercase casing (see StreamInfo).
        let json = r#"{
          "_meta": {"total": 1, "pages": 1, "current": 1, "count": 1, "limit": 10},
          "results": [
            {
              "id": "stream-1",
              "name": "Live Jam",
              "description": "Weekly jam session",
              "serverName": "Studio A",
              "metaUrl": "https://hls.example.com/x.m3u8",
              "chatId": "chat-1",
              "bannerURL": "https://cdn.example.com/banner.png"
            }
          ]
        }"#;
        let p: PaginatedChannels = serde_json::from_str(json).unwrap();
        assert_eq!(p.results.len(), 1);
        assert_eq!(p.results[0].id.as_deref(), Some("stream-1"));
        assert_eq!(p.results[0].server_name.as_deref(), Some("Studio A"));
        assert_eq!(p.meta.current, 1);

        let s = roundtrip(&p);
        assert!(s.contains("\"_meta\":"));
        assert!(s.contains("\"results\":"));
        assert!(s.contains("\"bannerURL\":"));
    }

    // ---- PaginatedEvents ----

    #[test]
    fn paginated_events_fixture_known_good() {
        let json = r#"{
          "_meta": {"total": 1, "pages": 1, "current": 1, "count": 1, "limit": 20},
          "results": [
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
          ]
        }"#;
        let p: PaginatedEvents = serde_json::from_str(json).unwrap();
        assert_eq!(p.results.len(), 1);
        assert_eq!(p.results[0].core.id.as_deref(), Some("evt-1"));
        assert_eq!(p.results[0].stream_name.as_deref(), Some("Studio Live"));
        assert_eq!(p.meta.total, 1);

        let s = roundtrip(&p);
        assert!(s.contains("\"_meta\":"));
        assert!(s.contains("\"streamId\":"));
    }

    // ---- PaginatedRecordings ----

    #[test]
    fn paginated_recordings_fixture_known_good() {
        let json = r#"{
          "_meta": {"total": 5, "pages": 1, "current": 1, "count": 1, "limit": 10},
          "results": [
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
          ]
        }"#;
        let p: PaginatedRecordings = serde_json::from_str(json).unwrap();
        assert_eq!(p.results.len(), 1);
        assert_eq!(p.results[0].id.as_deref(), Some("rec-1"));
        assert_eq!(p.results[0].views, Some(10));
        assert_eq!(p.meta.total, 5);

        let s = roundtrip(&p);
        assert!(s.contains("\"_meta\":"));
        assert!(s.contains("\"streamId\":"));
    }

    #[test]
    fn paginated_recordings_last_page() {
        let p = PaginatedRecordings {
            meta: meta_fixture(),
            results: vec![RecordingMetadata {
                id: Some("rec-42".into()),
                name: Some("Final Recording".into()),
                ..Default::default()
            }],
        };
        let s = roundtrip(&p);
        assert!(s.contains("\"current\":2"));
        assert!(s.contains("\"pages\":3"));
    }

    // ---- PaginatedStreamSearchResults ----

    #[test]
    fn paginated_stream_search_results_fixture_known_good() {
        let json = r#"{
          "_meta": {"total": 1, "pages": 1, "current": 1, "count": 1, "limit": 10},
          "results": [
            {
              "id": "stream-1",
              "name": "Jazz Quartet",
              "bannerURL": "https://cdn.example.com/banner.png",
              "serverId": "studio-1",
              "region": "ec2-us-north-ca",
              "lookingFor": 2,
              "skillLevels": ["intermediate"],
              "instruments": ["sax"],
              "genres": ["jazz"]
            }
          ]
        }"#;
        let p: PaginatedStreamSearchResults = serde_json::from_str(json).unwrap();
        assert_eq!(p.results.len(), 1);
        assert_eq!(p.results[0].base.id.as_deref(), Some("stream-1"));
        assert_eq!(p.results[0].server_id.as_deref(), Some("studio-1"));
        assert_eq!(p.results[0].looking_for, Some(2));

        let s = roundtrip(&p);
        assert!(s.contains("\"serverId\":"));
        assert!(s.contains("\"lookingFor\":"));
        assert!(s.contains("\"bannerURL\":"));
    }

    // ---- PaginatedBackingTracks ----

    #[test]
    fn paginated_backing_tracks_fixture_known_good() {
        let json = r#"{
          "_meta": {"total": 1, "pages": 1, "current": 1, "count": 1, "limit": 10},
          "results": [
            {
              "id": "trk-1",
              "serverId": "studio-1",
              "name": "Drum Loop",
              "duration": 120,
              "status": 0
            }
          ]
        }"#;
        let p: PaginatedBackingTracks = serde_json::from_str(json).unwrap();
        assert_eq!(p.results.len(), 1);
        assert_eq!(p.results[0].id.as_deref(), Some("trk-1"));
        assert_eq!(p.results[0].server_id.as_deref(), Some("studio-1"));

        let s = roundtrip(&p);
        assert!(s.contains("\"_meta\":"));
        assert!(s.contains("\"serverId\":"));
    }
}
