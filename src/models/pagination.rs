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

