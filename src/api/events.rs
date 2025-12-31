//! Events API endpoints
//!
//! JackTrip Radio upcoming events and broadcasts.

use super::{ApiClient, ApiError, urlencode};
use crate::models;
use serde::Serialize;
use wasm_bindgen::prelude::*;

// =============================================================================
// Events API
// =============================================================================

/// Events API for upcoming broadcasts
#[wasm_bindgen]
pub struct EventsApi {
    client: ApiClient,
}

impl EventsApi {
    pub(crate) fn from_client(client: &ApiClient) -> Self {
        Self {
            client: client.clone(),
        }
    }
}

// =============================================================================
// Rust API (primary interface)
// =============================================================================

impl EventsApi {
    /// List all public upcoming events
    pub async fn list_events(&self) -> Result<Vec<models::PublicUpcomingEvent>, ApiError> {
        self.client.get("/events").await
    }

    /// List events with pagination
    pub async fn list_events_paginated(
        &self,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedEvents, ApiError> {
        #[derive(Serialize)]
        struct Query {
            #[serde(skip_serializing_if = "Option::is_none")]
            page: Option<i32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<i32>,
        }

        if page.is_some() || limit.is_some() {
            self.client.get_with_query("/events-paginated", &Query { page, limit }).await
        } else {
            self.client.get("/events-paginated").await
        }
    }

    /// Get a public event by ID
    pub async fn get_event(&self, event_id: &str) -> Result<Vec<models::PublicUpcomingEvent>, ApiError> {
        let path = format!("/events/{}", urlencode(event_id));
        self.client.get(&path).await
    }

    /// Get the radio channel for an event
    pub async fn get_event_channel(&self, event_id: &str) -> Result<models::StreamInfo, ApiError> {
        let path = format!("/events/{}/channel", urlencode(event_id));
        self.client.get(&path).await
    }

    /// Get similar events
    pub async fn get_similar_events(&self, event_id: &str) -> Result<Vec<models::PublicUpcomingEvent>, ApiError> {
        let path = format!("/events/{}/similar", urlencode(event_id));
        self.client.get(&path).await
    }

    /// List events for a studio
    pub async fn list_studio_events(&self, studio_id: &str) -> Result<Vec<models::UpcomingEvent>, ApiError> {
        let path = format!("/studios/{}/events", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Get a specific event for a studio
    pub async fn get_studio_event(&self, studio_id: &str, event_id: &str) -> Result<models::UpcomingEvent, ApiError> {
        let path = format!("/studios/{}/events/{}", urlencode(studio_id), urlencode(event_id));
        self.client.get(&path).await
    }

    /// Create a new event for a studio
    pub async fn create_studio_event(
        &self,
        studio_id: &str,
        event: &models::UpcomingEvent,
    ) -> Result<models::UpcomingEvent, ApiError> {
        let path = format!("/studios/{}/events", urlencode(studio_id));
        self.client.post(&path, event).await
    }

    /// Update an event for a studio
    pub async fn update_studio_event(
        &self,
        studio_id: &str,
        event_id: &str,
        event: &models::UpcomingEvent,
    ) -> Result<models::UpcomingEvent, ApiError> {
        let path = format!("/studios/{}/events/{}", urlencode(studio_id), urlencode(event_id));
        self.client.put(&path, event).await
    }

    /// Delete an event for a studio
    pub async fn delete_studio_event(&self, studio_id: &str, event_id: &str) -> Result<(), ApiError> {
        let path = format!("/studios/{}/events/{}", urlencode(studio_id), urlencode(event_id));
        self.client.delete(&path).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[wasm_bindgen]
impl EventsApi {
    #[wasm_bindgen(constructor)]
    pub fn new(client: &ApiClient) -> Self {
        Self::from_client(client)
    }

    #[wasm_bindgen(js_name = listEvents)]
    pub async fn list_events_js(&self) -> Result<JsValue, ApiError> {
        let events = self.list_events().await?;
        serde_wasm_bindgen::to_value(&events).map_err(|e| ApiError::Serialization(e.to_string()))
    }

    #[wasm_bindgen(js_name = listEventsPaginated)]
    pub async fn list_events_paginated_js(
        &self,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedEvents, ApiError> {
        self.list_events_paginated(page, limit).await
    }

    #[wasm_bindgen(js_name = getEvent)]
    pub async fn get_event_js(&self, event_id: String) -> Result<JsValue, ApiError> {
        let events = self.get_event(&event_id).await?;
        serde_wasm_bindgen::to_value(&events).map_err(|e| ApiError::Serialization(e.to_string()))
    }

    #[wasm_bindgen(js_name = getEventChannel)]
    pub async fn get_event_channel_js(&self, event_id: String) -> Result<models::StreamInfo, ApiError> {
        self.get_event_channel(&event_id).await
    }

    #[wasm_bindgen(js_name = getSimilarEvents)]
    pub async fn get_similar_events_js(&self, event_id: String) -> Result<JsValue, ApiError> {
        let events = self.get_similar_events(&event_id).await?;
        serde_wasm_bindgen::to_value(&events).map_err(|e| ApiError::Serialization(e.to_string()))
    }

    #[wasm_bindgen(js_name = listStudioEvents)]
    pub async fn list_studio_events_js(&self, studio_id: String) -> Result<JsValue, ApiError> {
        let events = self.list_studio_events(&studio_id).await?;
        serde_wasm_bindgen::to_value(&events).map_err(|e| ApiError::Serialization(e.to_string()))
    }

    #[wasm_bindgen(js_name = getStudioEvent)]
    pub async fn get_studio_event_js(&self, studio_id: String, event_id: String) -> Result<models::UpcomingEvent, ApiError> {
        self.get_studio_event(&studio_id, &event_id).await
    }

    #[wasm_bindgen(js_name = createStudioEvent)]
    pub async fn create_studio_event_js(
        &self,
        studio_id: String,
        event: models::UpcomingEvent,
    ) -> Result<models::UpcomingEvent, ApiError> {
        self.create_studio_event(&studio_id, &event).await
    }

    #[wasm_bindgen(js_name = updateStudioEvent)]
    pub async fn update_studio_event_js(
        &self,
        studio_id: String,
        event_id: String,
        event: models::UpcomingEvent,
    ) -> Result<models::UpcomingEvent, ApiError> {
        self.update_studio_event(&studio_id, &event_id, &event).await
    }

    #[wasm_bindgen(js_name = deleteStudioEvent)]
    pub async fn delete_studio_event_js(&self, studio_id: String, event_id: String) -> Result<(), ApiError> {
        self.delete_studio_event(&studio_id, &event_id).await
    }
}
