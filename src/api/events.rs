//! Events API endpoints
//!
//! JackTrip Radio upcoming events and broadcasts.

use super::{to_js_value, PaginationQuery, ApiClient, ApiError, urlencode};
use crate::models;
use wasm_bindgen::prelude::*;

// =============================================================================
// Events API
// =============================================================================

api_module_struct!(EventsApi);

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
        if page.is_some() || limit.is_some() {
            self.client.get_with_query("/events-paginated", &PaginationQuery { page, limit }).await
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

    /// Get the live stream URL for an active event.
    ///
    /// Returns the active broadcast URL (`{ "redirect": "..." }`). The server
    /// returns 400 when the event is not currently active.
    pub async fn get_event_live(&self, event_id: &str) -> Result<models::Redirect, ApiError> {
        let path = format!("/events/{}/live", urlencode(event_id));
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
    #[wasm_bindgen(js_name = listEvents)]
    pub async fn list_events_js(&self) -> Result<JsValue, ApiError> {
        let events = self.list_events().await?;
        to_js_value(&events)
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
        to_js_value(&events)
    }

    #[wasm_bindgen(js_name = getEventChannel)]
    pub async fn get_event_channel_js(&self, event_id: String) -> Result<models::StreamInfo, ApiError> {
        self.get_event_channel(&event_id).await
    }

    #[wasm_bindgen(js_name = getSimilarEvents)]
    pub async fn get_similar_events_js(&self, event_id: String) -> Result<JsValue, ApiError> {
        let events = self.get_similar_events(&event_id).await?;
        to_js_value(&events)
    }

    #[wasm_bindgen(js_name = getEventLive)]
    pub async fn get_event_live_js(&self, event_id: String) -> Result<models::Redirect, ApiError> {
        self.get_event_live(&event_id).await
    }

    #[wasm_bindgen(js_name = listStudioEvents)]
    pub async fn list_studio_events_js(&self, studio_id: String) -> Result<JsValue, ApiError> {
        let events = self.list_studio_events(&studio_id).await?;
        to_js_value(&events)
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::api::test_helpers::{assert_http_status, mock_api, mock_empty, mock_json};

    fn api(client: &ApiClient) -> EventsApi {
        EventsApi::from_client(client)
    }

    #[tokio::test]
    async fn test_list_events_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/events",
            200,
            r#"[{"id":"event1","title":"Test Event"}]"#,
        )
        .await;

        let result = api(&client).list_events().await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].core.id, Some("event1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_events_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/events", 500, "boom").await;

        let err = api(&client).list_events().await.unwrap_err();
        assert_http_status(err, 500);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_events_paginated_with_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/events-paginated",
            200,
            r#"{"_meta":{"total":11,"pages":2,"current":2,"count":1,"limit":10},"results":[{"id":"e1"}]}"#,
        )
        .await;

        let result = api(&client)
            .list_events_paginated(Some(2), Some(10))
            .await
            .unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.meta.current, 2);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_events_paginated_no_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/events-paginated",
            200,
            r#"{"_meta":{"total":0,"pages":0,"current":1,"count":0,"limit":10},"results":[]}"#,
        )
        .await;

        let result = api(&client).list_events_paginated(None, None).await.unwrap();
        assert_eq!(result.results.len(), 0);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_event_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/events/evt123",
            200,
            r#"[{"id":"evt123","title":"My Event","startTime":"2024-01-01T00:00:00Z"}]"#,
        )
        .await;

        let result = api(&client).get_event("evt123").await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].core.id, Some("evt123".to_string()));
        assert_eq!(result[0].core.title, Some("My Event".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_event_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/events/nonexistent", 404, "nope").await;

        let err = api(&client).get_event("nonexistent").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_event_channel() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/events/evt1/channel",
            200,
            r#"{"id":"chan1","name":"Channel One"}"#,
        )
        .await;

        let channel = api(&client).get_event_channel("evt1").await.unwrap();
        assert_eq!(channel.id, Some("chan1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_similar_events() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/events/evt1/similar",
            200,
            r#"[{"id":"evt2"}]"#,
        )
        .await;

        let result = api(&client).get_similar_events("evt1").await.unwrap();
        assert_eq!(result[0].core.id, Some("evt2".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_event_live_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/events/evt1/live",
            200,
            r#"{"redirect":"https://live.example.com/stream"}"#,
        )
        .await;

        let live = api(&client).get_event_live("evt1").await.unwrap();
        assert_eq!(live.redirect, Some("https://live.example.com/stream".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_event_live_inactive() {
        let (mut server, client) = mock_api().await;
        // The API returns 400 when the event is not currently active.
        let mock = mock_json(&mut server, "GET", "/events/evt1/live", 400, "not active").await;

        let err = api(&client).get_event_live("evt1").await.unwrap_err();
        assert_http_status(err, 400);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_studio_events() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/events",
            200,
            r#"[{"id":"evt1","studioId":"st1"}]"#,
        )
        .await;

        let result = api(&client).list_studio_events("st1").await.unwrap();
        assert_eq!(result[0].studio_id, Some("st1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_studio_event() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/events/evt1",
            200,
            r#"{"id":"evt1","studioId":"st1"}"#,
        )
        .await;

        let event = api(&client).get_studio_event("st1", "evt1").await.unwrap();
        assert_eq!(event.core.id, Some("evt1".to_string()));
        assert_eq!(event.studio_id, Some("st1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_studio_event() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/studios/st1/events",
            200,
            r#"{"id":"evt1"}"#,
        )
        .await;

        let body = models::UpcomingEvent::default();
        let event = api(&client).create_studio_event("st1", &body).await.unwrap();
        assert_eq!(event.core.id, Some("evt1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_studio_event() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "PUT",
            "/studios/st1/events/evt1",
            200,
            r#"{"id":"evt1","title":"updated"}"#,
        )
        .await;

        let body = models::UpcomingEvent::default();
        let event = api(&client)
            .update_studio_event("st1", "evt1", &body)
            .await
            .unwrap();
        assert_eq!(event.core.title, Some("updated".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_studio_event() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/studios/st1/events/evt1", 204).await;

        api(&client).delete_studio_event("st1", "evt1").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_studio_event_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/studios/st1/events/evt1", 404).await;

        let err = api(&client)
            .delete_studio_event("st1", "evt1")
            .await
            .unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }
}
