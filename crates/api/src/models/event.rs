//! Upcoming-event models

use serde::{Deserialize, Serialize};
#[cfg(feature = "wasm")]
use tsify_next::Tsify;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// The fields every event carries. [`UpcomingEvent`] and
/// [`PublicUpcomingEvent`] flatten it, so its keys sit at the top level of
/// their JSON objects.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct EventInfo {
    /// Event ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Event name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Event description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Event image URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// When the event starts (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,

    /// When the event ends (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,

    /// When the event was created (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// An event as listed publicly on JackTrip Radio, with its studio's name and
/// banner.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct PublicUpcomingEvent {
    /// Common event fields (flattened)
    #[serde(flatten)]
    pub event: EventInfo,

    /// ID of the studio hosting the event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,

    /// Display name of the studio hosting the event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,

    /// Studio banner image URL
    #[serde(rename = "bannerURL", skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,
}

/// An event as managed by its studio (`/studios/{studioId}/events`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct UpcomingEvent {
    /// Common event fields (flattened)
    #[serde(flatten)]
    pub event: EventInfo,

    /// ID of the studio hosting the event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_wire_format() {
        // Regression: events used `title` / `startTime` / `endTime`; the API
        // sends `name` / `start` / `end`, so every event decoded empty.
        let json = serde_json::json!({
            "id": "evt-1", "name": "Friday Night Jam", "description": "Weekly jam",
            "image": "img.png", "start": "2026-06-20T01:00:00Z", "end": "2026-06-20T03:00:00Z",
            "createdAt": "2026-06-01T00:00:00Z",
            "serverId": "studio-1", "serverName": "Studio", "bannerURL": "b.png"
        });
        let e: PublicUpcomingEvent = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(e.event.name.as_deref(), Some("Friday Night Jam"));
        assert_eq!(e.event.start.as_deref(), Some("2026-06-20T01:00:00Z"));
        assert_eq!(e.banner_url.as_deref(), Some("b.png"));
        assert_eq!(serde_json::to_value(&e).unwrap(), json);
    }
}
