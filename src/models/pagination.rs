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
}
