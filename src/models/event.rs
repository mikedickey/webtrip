//! Event models

use super::Visibility;
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Public upcoming event information
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PublicUpcomingEvent {
    /// Event ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Event title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Event description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Stream/channel ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,

    /// Stream/channel name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_name: Option<String>,

    /// Event image URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Banner image URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banner_url: Option<String>,

    /// Start time (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,

    /// End time (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,

    /// Timezone (e.g., "America/Los_Angeles")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,

    /// Whether this is a recurring event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurring: Option<bool>,

    /// Recurrence rule (iCal RRULE format)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rrule: Option<String>,
}

/// Studio event (editable by owner)
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct UpcomingEvent {
    /// Event ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Event title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Event description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Studio ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studio_id: Option<String>,

    /// Stream ID (for broadcast events)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,

    /// Event image URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Visibility setting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Visibility>,

    /// Start time (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,

    /// End time (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,

    /// Timezone
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,

    /// Whether this is a recurring event
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurring: Option<bool>,

    /// Recurrence rule
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rrule: Option<String>,

    /// Creation timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// Last update timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Event information (simplified)
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct EventInfo {
    /// Event ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Event title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Start time (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<String>,

    /// End time (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<String>,
}

