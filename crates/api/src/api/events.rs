//! Events API: JackTrip Radio upcoming events and broadcasts.
//!
//! Reach it through [`ApiClient::events`]. Two groups of endpoints live here:
//!
//! - **Public events** (`/events…`) — browse upcoming events, their radio
//!   channel, similar events, and the live stream URL. No authentication needed.
//! - **Studio events** (`/studios/{studioId}/events…`) — create, read, update
//!   and delete the events a studio schedules. These require a bearer token
//!   ([`ApiClient::set_bearer_token`]).
//!
//! # Example
//!
//! ```rust,no_run
//! use webtrip_api::api::{ApiClient, ApiError};
//!
//! # async fn example() -> Result<(), ApiError> {
//! let client = ApiClient::new();
//!
//! // Public: no token needed.
//! let page = client.events().list_events_paginated(Some(1), Some(20)).await?;
//! for event in &page.results {
//!     println!("{:?} starts {:?}", event.event.name, event.event.start);
//! }
//!
//! // Studio events need a bearer token.
//! client.set_bearer_token("<token>".to_string());
//! let scheduled = client.events().list_studio_events("studio123").await?;
//! # let _ = scheduled;
//! # Ok(())
//! # }
//! ```
//!
//! From JavaScript (package `webtrip`):
//!
//! ```js
//! import init, { ApiClient } from 'webtrip';
//! await init();
//!
//! const client = new ApiClient();
//! const page = await client.events().listEventsPaginated(1, 20);
//!
//! client.setBearerToken(token);
//! const event = await client.events().createStudioEvent('studio123', {
//!   name: 'Live Session',
//!   start: '2026-06-20T01:00:00Z',
//!   end: '2026-06-20T03:00:00Z',
//! });
//! ```

#[cfg(feature = "wasm")]
use super::to_js_value;
use super::{PaginationQuery, ApiClient, ApiError, urlencode};
use crate::models;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

api_module_struct!(
    /// Public and studio event endpoints; obtain with [`ApiClient::events`].
    EventsApi
);

impl EventsApi {
    /// List all public upcoming events.
    ///
    /// `GET /events`. No authentication required. See also
    /// [`list_events_paginated`](Self::list_events_paginated).
    pub async fn list_events(&self) -> Result<Vec<models::PublicUpcomingEvent>, ApiError> {
        self.client.get("/events").await
    }

    /// List public upcoming events, one page at a time.
    ///
    /// `GET /events-paginated`. No authentication required. `page` and `limit`
    /// are sent as query parameters only when given; omitted ones fall back to
    /// the server defaults. The [`models::PaginatedEvents`] envelope carries the
    /// page metadata in `meta` (`_meta` on the wire).
    pub async fn list_events_paginated(
        &self,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedEvents, ApiError> {
        self.client.get_with_query("/events-paginated", &PaginationQuery { page, limit }).await
    }

    /// Get a public event by ID.
    ///
    /// `GET /events/{eventId}`. No authentication required.
    pub async fn get_event(&self, event_id: &str) -> Result<models::PublicUpcomingEvent, ApiError> {
        let path = format!("/events/{}", urlencode(event_id));
        self.client.get(&path).await
    }

    /// Get the radio channel an event broadcasts on, with the details needed
    /// to reach the studio serving it.
    ///
    /// `GET /events/{eventId}/channel`. No authentication required.
    pub async fn get_event_channel(&self, event_id: &str) -> Result<models::StreamCacheEntry, ApiError> {
        let path = format!("/events/{}/channel", urlencode(event_id));
        self.client.get(&path).await
    }

    /// Get events similar to the given one.
    ///
    /// `GET /events/{eventId}/similar`. No authentication required.
    pub async fn get_similar_events(&self, event_id: &str) -> Result<Vec<models::PublicUpcomingEvent>, ApiError> {
        let path = format!("/events/{}/similar", urlencode(event_id));
        self.client.get(&path).await
    }

    /// Get the live stream URL for an active event.
    ///
    /// `GET /events/{eventId}/live`. No authentication required. Returns the
    /// active broadcast URL in [`models::Redirect::redirect`]. The server
    /// responds `400` (surfaced as [`ApiError::Http`]) when the event is not
    /// currently active, i.e. before its start time or after its end time.
    pub async fn get_event_live(&self, event_id: &str) -> Result<models::Redirect, ApiError> {
        let path = format!("/events/{}/live", urlencode(event_id));
        self.client.get(&path).await
    }

    /// List the events scheduled by a studio.
    ///
    /// `GET /studios/{studioId}/events`. Requires a bearer token.
    pub async fn list_studio_events(&self, studio_id: &str) -> Result<Vec<models::UpcomingEvent>, ApiError> {
        let path = format!("/studios/{}/events", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Get one of a studio's events.
    ///
    /// `GET /studios/{studioId}/events/{eventId}`. Requires a bearer token.
    pub async fn get_studio_event(&self, studio_id: &str, event_id: &str) -> Result<models::UpcomingEvent, ApiError> {
        let path = format!("/studios/{}/events/{}", urlencode(studio_id), urlencode(event_id));
        self.client.get(&path).await
    }

    /// Create a new event for a studio.
    ///
    /// `POST /studios/{studioId}/events`. Requires a bearer token. Returns the
    /// event as created by the server.
    pub async fn create_studio_event(
        &self,
        studio_id: &str,
        event: &models::UpcomingEvent,
    ) -> Result<models::UpcomingEvent, ApiError> {
        let path = format!("/studios/{}/events", urlencode(studio_id));
        self.client.post(&path, event).await
    }

    /// Update one of a studio's events.
    ///
    /// `PUT /studios/{studioId}/events/{eventId}`. Requires a bearer token.
    pub async fn update_studio_event(
        &self,
        studio_id: &str,
        event_id: &str,
        event: &models::UpcomingEvent,
    ) -> Result<models::UpcomingEvent, ApiError> {
        let path = format!("/studios/{}/events/{}", urlencode(studio_id), urlencode(event_id));
        self.client.put(&path, event).await
    }

    /// Delete one of a studio's events.
    ///
    /// `DELETE /studios/{studioId}/events/{eventId}`. Requires a bearer token.
    pub async fn delete_studio_event(&self, studio_id: &str, event_id: &str) -> Result<(), ApiError> {
        let path = format!("/studios/{}/events/{}", urlencode(studio_id), urlencode(event_id));
        self.client.delete(&path).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[cfg(feature = "wasm")]
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
    pub async fn get_event_js(&self, event_id: String) -> Result<models::PublicUpcomingEvent, ApiError> {
        self.get_event(&event_id).await
    }

    #[wasm_bindgen(js_name = getEventChannel)]
    pub async fn get_event_channel_js(&self, event_id: String) -> Result<models::StreamCacheEntry, ApiError> {
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
            r#"[{"id":"event1","name":"Test Event"}]"#,
        )
        .await;

        let result = api(&client).list_events().await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].event.id, Some("event1".to_string()));
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
            r#"{"id":"evt123","name":"My Event","start":"2024-01-01T00:00:00Z"}"#,
        )
        .await;

        let result = api(&client).get_event("evt123").await.unwrap();
        assert_eq!(result.event.id, Some("evt123".to_string()));
        assert_eq!(result.event.name, Some("My Event".to_string()));
        assert_eq!(result.event.start.as_deref(), Some("2024-01-01T00:00:00Z"));
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
        assert_eq!(channel.base.id, Some("chan1".to_string()));
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
        assert_eq!(result[0].event.id, Some("evt2".to_string()));
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
            r#"[{"id":"evt1","serverId":"st1"}]"#,
        )
        .await;

        let result = api(&client).list_studio_events("st1").await.unwrap();
        assert_eq!(result[0].server_id, Some("st1".to_string()));
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
            r#"{"id":"evt1","serverId":"st1"}"#,
        )
        .await;

        let event = api(&client).get_studio_event("st1", "evt1").await.unwrap();
        assert_eq!(event.event.id, Some("evt1".to_string()));
        assert_eq!(event.server_id, Some("st1".to_string()));
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
        assert_eq!(event.event.id, Some("evt1".to_string()));
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
            r#"{"id":"evt1","name":"updated"}"#,
        )
        .await;

        let body = models::UpcomingEvent::default();
        let event = api(&client)
            .update_studio_event("st1", "evt1", &body)
            .await
            .unwrap();
        assert_eq!(event.event.name, Some("updated".to_string()));
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
