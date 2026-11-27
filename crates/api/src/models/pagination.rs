//! Pagination models
//!
//! The API wraps every paginated list in a `{ _meta, results }` envelope, where
//! `_meta` carries [`PaginationMeta`] and `results` is a typed array. Paginated
//! endpoints take optional `page` (1-indexed) and `limit` query parameters;
//! when omitted, the server's defaults apply.

use serde::{Deserialize, Serialize};
#[cfg(feature = "wasm")]
use tsify_next::Tsify;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

use super::{
    BackingTrack, Conversation, Message, PublicUpcomingEvent, RecordingMetadata, ServerRecording, StreamInfo,
    StreamInfoSearchResult,
};

/// Pagination metadata returned in the `_meta` field of every paginated response.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct PaginationMeta {
    /// Total number of items that match the query
    pub total: i64,

    /// Total number of pages
    pub pages: i64,

    /// Current page number (1-indexed; `0` when there are no results)
    pub current: i64,

    /// Number of items on this page
    pub count: i64,

    /// Maximum number of items per page
    pub limit: i64,
}

macro_rules! paginated {
    ($(#[$meta:meta])* $name:ident, $item:ty) => {
        $(#[$meta])*
        #[cfg_attr(feature = "wasm", derive(Tsify))]
        #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
        #[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
        pub struct $name {
            /// Pagination metadata (`_meta` on the wire)
            #[serde(rename = "_meta")]
            pub meta: PaginationMeta,

            /// Items on the current page
            pub results: Vec<$item>,
        }
    };
}

paginated!(
    /// A page of channels ([`StreamInfo`]).
    PaginatedChannels, StreamInfo
);
paginated!(
    /// A page of public upcoming events.
    PaginatedEvents, PublicUpcomingEvent
);
paginated!(
    /// A page of public recordings.
    PaginatedRecordings, RecordingMetadata
);
paginated!(
    /// A page of a studio's or user's recordings, including the studio,
    /// session and owner that produced each one.
    PaginatedServerRecordings, ServerRecording
);
paginated!(
    /// A page of stream search results.
    PaginatedStreamSearchResults, StreamInfoSearchResult
);
paginated!(
    /// A page of a studio's backing tracks.
    PaginatedBackingTracks, BackingTrack
);
paginated!(
    /// A page of conversations.
    PaginatedConversations, Conversation
);
paginated!(
    /// A page of messages in a conversation.
    PaginatedMessages, Message
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginated_envelope_wire_format() {
        // Live shape from `GET /channels-paginated`, including the `_meta` key.
        let json = serde_json::json!({
            "_meta": {"total": 14, "pages": 14, "current": 1, "count": 1, "limit": 1},
            "results": [{"id": "c1", "name": "Test", "follows": 0}]
        });
        let r: PaginatedChannels = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(r.meta.total, 14);
        assert_eq!(r.results[0].id.as_deref(), Some("c1"));
        assert_eq!(serde_json::to_value(&r).unwrap(), json);
    }
}
