//! Streams API: JackTrip Radio broadcasts, channels, conversations, and studio
//! stream management.
//!
//! Reach it through [`ApiClient::streams`]. The endpoints fall into four groups:
//!
//! - **Discovery** — list, search and fetch public broadcasts and channels.
//!   No authentication needed.
//! - **Following** — follow or unfollow a broadcast.
//! - **Conversations** — the per-user message threads attached to a stream.
//! - **Studio streams** — configure, activate, and simulcast the live stream
//!   that a studio broadcasts.
//!
//! Everything except discovery requires a bearer token
//! ([`ApiClient::set_bearer_token`]). Broadcast chat is a WebSocket endpoint
//! and is not covered by this client.
//!
//! # Example
//!
//! ```rust,no_run
//! use webtrip_api::api::{ApiClient, ApiError};
//! use webtrip_api::models::{ActivationRequestOpts, BroadcastVisibility, StreamSearchQuery};
//!
//! # async fn example() -> Result<(), ApiError> {
//! let client = ApiClient::new();
//!
//! // Public search; unset filters are omitted from the query string.
//! let query = StreamSearchQuery { q: Some("jazz".into()), ..Default::default() };
//! let found = client.streams().search_streams(&query).await?;
//! println!("{} matching broadcasts", found.meta.total);
//!
//! // Going live from a studio needs a bearer token.
//! client.set_bearer_token("<token>".to_string());
//! let opts = ActivationRequestOpts {
//!     visibility: Some(BroadcastVisibility::Public),
//!     ..Default::default()
//! };
//! let stream = client.streams().activate_studio_stream("studio123", &opts).await?;
//! println!("live as stream {:?}", stream.id);
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
//! const found = await client.streams().searchStreams({ q: 'jazz', page: 1, limit: 20 });
//! found.results.forEach((s) => console.log(s.name));
//!
//! client.setBearerToken(token);
//! await client.streams().updateSimulcastDestination('studio123', 'youtube', {
//!   name: 'youtube',
//!   url: 'rtmp://a.rtmp.youtube.com/live2',
//!   key: '<key>',
//! });
//! ```

#[cfg(feature = "wasm")]
use super::to_js_value;
use super::{PaginationQuery, ApiClient, ApiError, urlencode};
use crate::models;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

api_module_struct!(
    /// JackTrip Radio broadcast, channel, conversation and studio-stream
    /// endpoints; obtain with [`ApiClient::streams`].
    StreamsApi
);

/// Query for the paginated conversation and message lists: an optional
/// filter plus `page`/`limit`, each sent only when set.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FilteredPage<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    user_id: Option<&'a str>,
    #[serde(flatten)]
    page: PaginationQuery,
}

fn stream_follow_path(stream_id: &str) -> String {
    format!("/streams/{}/follow", urlencode(stream_id))
}

fn conversation_path(stream_id: &str, user_id: &str) -> String {
    format!(
        "/streams/{}/conversations/{}",
        urlencode(stream_id),
        urlencode(user_id)
    )
}

fn conversation_messages_path(stream_id: &str, user_id: &str) -> String {
    format!("{}/messages", conversation_path(stream_id, user_id))
}

fn conversation_message_path(stream_id: &str, user_id: &str, message_id: &str) -> String {
    format!(
        "{}/{}",
        conversation_messages_path(stream_id, user_id),
        urlencode(message_id)
    )
}

impl StreamsApi {
    /// List all public, currently active broadcasts.
    ///
    /// `GET /streams`. No authentication required.
    pub async fn list_streams(&self) -> Result<Vec<models::StreamInfo>, ApiError> {
        self.client.get("/streams").await
    }

    /// Search for broadcasts.
    ///
    /// `GET /streams/search`. No authentication required. Accepts the full [`models::StreamSearchQuery`] contract (`q`, `lookingFor`,
    /// `skillLevel`, `instrument`, `genre`, `region`, `page`, `limit`) and returns
    /// the paginated `{ _meta, results }` envelope whose items carry
    /// search-specific fields ([`models::StreamInfoSearchResult`]).
    pub async fn search_streams(
        &self,
        query: &models::StreamSearchQuery,
    ) -> Result<models::PaginatedStreamSearchResults, ApiError> {
        self.client.get_with_query("/streams/search", query).await
    }

    /// Get a broadcast by ID, with engagement data.
    ///
    /// `GET /streams/{streamId}`. No authentication required.
    pub async fn get_stream(&self, stream_id: &str) -> Result<models::StreamInfoWithEngagement, ApiError> {
        let path = format!("/streams/{}", urlencode(stream_id));
        self.client.get(&path).await
    }

    /// Follow a broadcast as the authenticated user.
    ///
    /// `POST /streams/{streamId}/follow` (empty body). Requires a bearer token.
    pub async fn follow_stream(&self, stream_id: &str) -> Result<(), ApiError> {
        self.client.post_empty_no_response(&stream_follow_path(stream_id)).await
    }

    /// Unfollow a broadcast as the authenticated user.
    ///
    /// `DELETE /streams/{streamId}/follow`. Requires a bearer token.
    pub async fn unfollow_stream(&self, stream_id: &str) -> Result<(), ApiError> {
        self.client.delete(&stream_follow_path(stream_id)).await
    }

    /// List all public channels.
    ///
    /// `GET /channels`. No authentication required. See also
    /// [`list_channels_paginated`](Self::list_channels_paginated).
    pub async fn list_channels(&self) -> Result<Vec<models::StreamInfo>, ApiError> {
        self.client.get("/channels").await
    }

    /// List public channels, one page at a time.
    ///
    /// `GET /channels-paginated`. No authentication required. `page` and `limit` are sent as
    /// query parameters only when given; omitted ones fall back to the server
    /// defaults.
    pub async fn list_channels_paginated(
        &self,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedChannels, ApiError> {
        self.client.get_with_query("/channels-paginated", &PaginationQuery { page, limit }).await
    }

    /// Get a page of the conversations attached to a stream.
    ///
    /// `GET /streams/{streamId}/conversations`. Requires a bearer token.
    /// `user_id` narrows the results to one user's conversation; `page`
    /// (1-indexed) and `limit` fall back to the server defaults when `None`.
    pub async fn get_stream_conversations(
        &self,
        stream_id: &str,
        user_id: Option<&str>,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedConversations, ApiError> {
        let path = format!("/streams/{}/conversations", urlencode(stream_id));
        let query = FilteredPage { user_id, page: PaginationQuery { page, limit } };
        self.client.get_with_query(&path, &query).await
    }

    /// Get the conversation with one user on a stream.
    ///
    /// `GET /streams/{streamId}/conversations/{userId}`. Requires a bearer token.
    pub async fn get_stream_conversation(
        &self,
        stream_id: &str,
        user_id: &str,
    ) -> Result<models::Conversation, ApiError> {
        self.client.get(&conversation_path(stream_id, user_id)).await
    }

    /// Mark messages as read in a conversation.
    ///
    /// `POST /streams/{streamId}/conversations/{userId}/last`. Requires a bearer token.
    /// Sets the authenticated user's last-read position to
    /// [`models::MarkReadRequest::message_id`].
    pub async fn mark_conversation_read(
        &self,
        stream_id: &str,
        user_id: &str,
        req: &models::MarkReadRequest,
    ) -> Result<(), ApiError> {
        let path = format!("{}/last", conversation_path(stream_id, user_id));
        self.client.post_no_response(&path, req).await
    }

    /// Get a page of the messages in a conversation.
    ///
    /// `GET /streams/{streamId}/conversations/{userId}/messages`. Requires a
    /// bearer token. Pagination behaves as in
    /// [`get_stream_conversations`](Self::get_stream_conversations).
    pub async fn get_conversation_messages(
        &self,
        stream_id: &str,
        user_id: &str,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedMessages, ApiError> {
        let query = FilteredPage { user_id: None, page: PaginationQuery { page, limit } };
        self.client.get_with_query(&conversation_messages_path(stream_id, user_id), &query).await
    }

    /// Send a message in a conversation.
    ///
    /// `POST /streams/{streamId}/conversations/{userId}/messages`. Requires a bearer token.
    pub async fn send_message(
        &self,
        stream_id: &str,
        user_id: &str,
        message: &models::SendMessageRequest,
    ) -> Result<models::Message, ApiError> {
        self.client.post(&conversation_messages_path(stream_id, user_id), message).await
    }

    /// Get one message in a conversation.
    ///
    /// `GET /streams/{streamId}/conversations/{userId}/messages/{messageId}`.
    /// Requires a bearer token.
    pub async fn get_conversation_message(
        &self,
        stream_id: &str,
        user_id: &str,
        message_id: &str,
    ) -> Result<models::Message, ApiError> {
        self.client
            .get(&conversation_message_path(stream_id, user_id, message_id))
            .await
    }

    /// Update an existing message in a conversation.
    ///
    /// `PUT /streams/{streamId}/conversations/{userId}/messages/{messageId}`.
    /// Requires a bearer token. Only the message status may be changed (see
    /// [`models::UpdateMessageRequest`]); the text is immutable.
    pub async fn update_conversation_message(
        &self,
        stream_id: &str,
        user_id: &str,
        message_id: &str,
        req: &models::UpdateMessageRequest,
    ) -> Result<models::Message, ApiError> {
        self.client
            .put(&conversation_message_path(stream_id, user_id, message_id), req)
            .await
    }

    /// Get a studio's live stream configuration.
    ///
    /// `GET /studios/{studioId}/stream`. Requires a bearer token.
    pub async fn get_studio_stream(&self, studio_id: &str) -> Result<models::LiveStream, ApiError> {
        let path = format!("/studios/{}/stream", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Create a studio's live stream, or reset an existing one.
    ///
    /// `POST /studios/{studioId}/stream`. Requires a bearer token.
    pub async fn create_studio_stream(
        &self,
        studio_id: &str,
        stream: &models::LiveStream,
    ) -> Result<models::LiveStream, ApiError> {
        let path = format!("/studios/{}/stream", urlencode(studio_id));
        self.client.post(&path, stream).await
    }

    /// Update a studio's live stream configuration.
    ///
    /// `PUT /studios/{studioId}/stream`. Requires a bearer token.
    pub async fn update_studio_stream(
        &self,
        studio_id: &str,
        stream: &models::LiveStream,
    ) -> Result<models::LiveStream, ApiError> {
        let path = format!("/studios/{}/stream", urlencode(studio_id));
        self.client.put(&path, stream).await
    }

    /// Start, change or stop a studio's broadcast or recording.
    ///
    /// `POST /studios/{studioId}/stream/activate`. Requires a bearer token.
    /// [`models::ActivationRequestOpts::visibility`] picks the mode (public or
    /// unlisted broadcast, private or stem recording);
    /// [`Offline`](models::BroadcastVisibility::Offline) stops it. Returns the
    /// updated [`models::LiveStream`].
    pub async fn activate_studio_stream(
        &self,
        studio_id: &str,
        opts: &models::ActivationRequestOpts,
    ) -> Result<models::LiveStream, ApiError> {
        let path = format!("/studios/{}/stream/activate", urlencode(studio_id));
        self.client.post(&path, opts).await
    }

    /// List a studio's simulcast destinations (external RTMP targets that
    /// receive a copy of the stream).
    ///
    /// `GET /studios/{studioId}/simulcast`. Requires a bearer token.
    pub async fn get_simulcast_destinations(&self, studio_id: &str) -> Result<Vec<models::SimulcastDestination>, ApiError> {
        let path = format!("/studios/{}/simulcast", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Add or update a simulcast destination.
    ///
    /// `PUT /studios/{studioId}/simulcast/{destination}`. Requires a bearer
    /// token. `destination` is the platform name (`"youtube"`, `"facebook"` or
    /// `"twitch"`); the destination is added if it does not exist yet,
    /// otherwise updated. The server returns no body; re-read with
    /// [`get_simulcast_destinations`](Self::get_simulcast_destinations).
    pub async fn update_simulcast_destination(
        &self,
        studio_id: &str,
        destination: &str,
        config_data: &models::SimulcastDestination,
    ) -> Result<(), ApiError> {
        let path = format!("/studios/{}/simulcast/{}", urlencode(studio_id), urlencode(destination));
        self.client.put(&path, config_data).await
    }

    /// Remove a simulcast destination.
    ///
    /// `DELETE /studios/{studioId}/simulcast/{destination}`. Requires a bearer token.
    pub async fn delete_simulcast_destination(&self, studio_id: &str, destination: &str) -> Result<(), ApiError> {
        let path = format!("/studios/{}/simulcast/{}", urlencode(studio_id), urlencode(destination));
        self.client.delete(&path).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[cfg(feature = "wasm")]
#[wasm_bindgen]
impl StreamsApi {
    #[wasm_bindgen(js_name = listStreams)]
    pub async fn list_streams_js(&self) -> Result<JsValue, ApiError> {
        let streams = self.list_streams().await?;
        to_js_value(&streams)
    }

    #[wasm_bindgen(js_name = searchStreams)]
    pub async fn search_streams_js(&self, query: models::StreamSearchQuery) -> Result<JsValue, ApiError> {
        let streams = self.search_streams(&query).await?;
        to_js_value(&streams)
    }

    #[wasm_bindgen(js_name = getStream)]
    pub async fn get_stream_js(&self, stream_id: String) -> Result<models::StreamInfoWithEngagement, ApiError> {
        self.get_stream(&stream_id).await
    }

    #[wasm_bindgen(js_name = followStream)]
    pub async fn follow_stream_js(&self, stream_id: String) -> Result<(), ApiError> {
        self.follow_stream(&stream_id).await
    }

    #[wasm_bindgen(js_name = unfollowStream)]
    pub async fn unfollow_stream_js(&self, stream_id: String) -> Result<(), ApiError> {
        self.unfollow_stream(&stream_id).await
    }

    #[wasm_bindgen(js_name = listChannels)]
    pub async fn list_channels_js(&self) -> Result<JsValue, ApiError> {
        let channels = self.list_channels().await?;
        to_js_value(&channels)
    }

    #[wasm_bindgen(js_name = listChannelsPaginated)]
    pub async fn list_channels_paginated_js(
        &self,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedChannels, ApiError> {
        self.list_channels_paginated(page, limit).await
    }

    #[wasm_bindgen(js_name = getStreamConversations)]
    pub async fn get_stream_conversations_js(
        &self,
        stream_id: String,
        user_id: Option<String>,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedConversations, ApiError> {
        self.get_stream_conversations(&stream_id, user_id.as_deref(), page, limit).await
    }

    #[wasm_bindgen(js_name = getStreamConversation)]
    pub async fn get_stream_conversation_js(
        &self,
        stream_id: String,
        user_id: String,
    ) -> Result<models::Conversation, ApiError> {
        self.get_stream_conversation(&stream_id, &user_id).await
    }

    #[wasm_bindgen(js_name = markConversationRead)]
    pub async fn mark_conversation_read_js(
        &self,
        stream_id: String,
        user_id: String,
        req: models::MarkReadRequest,
    ) -> Result<(), ApiError> {
        self.mark_conversation_read(&stream_id, &user_id, &req).await
    }

    #[wasm_bindgen(js_name = getConversationMessages)]
    pub async fn get_conversation_messages_js(
        &self,
        stream_id: String,
        user_id: String,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedMessages, ApiError> {
        self.get_conversation_messages(&stream_id, &user_id, page, limit).await
    }

    #[wasm_bindgen(js_name = sendMessage)]
    pub async fn send_message_js(
        &self,
        stream_id: String,
        user_id: String,
        message: models::SendMessageRequest,
    ) -> Result<models::Message, ApiError> {
        self.send_message(&stream_id, &user_id, &message).await
    }

    #[wasm_bindgen(js_name = getConversationMessage)]
    pub async fn get_conversation_message_js(
        &self,
        stream_id: String,
        user_id: String,
        message_id: String,
    ) -> Result<models::Message, ApiError> {
        self.get_conversation_message(&stream_id, &user_id, &message_id).await
    }

    #[wasm_bindgen(js_name = updateConversationMessage)]
    pub async fn update_conversation_message_js(
        &self,
        stream_id: String,
        user_id: String,
        message_id: String,
        req: models::UpdateMessageRequest,
    ) -> Result<models::Message, ApiError> {
        self.update_conversation_message(&stream_id, &user_id, &message_id, &req).await
    }

    #[wasm_bindgen(js_name = getStudioStream)]
    pub async fn get_studio_stream_js(&self, studio_id: String) -> Result<models::LiveStream, ApiError> {
        self.get_studio_stream(&studio_id).await
    }

    #[wasm_bindgen(js_name = createStudioStream)]
    pub async fn create_studio_stream_js(
        &self,
        studio_id: String,
        stream: models::LiveStream,
    ) -> Result<models::LiveStream, ApiError> {
        self.create_studio_stream(&studio_id, &stream).await
    }

    #[wasm_bindgen(js_name = updateStudioStream)]
    pub async fn update_studio_stream_js(
        &self,
        studio_id: String,
        stream: models::LiveStream,
    ) -> Result<models::LiveStream, ApiError> {
        self.update_studio_stream(&studio_id, &stream).await
    }

    #[wasm_bindgen(js_name = activateStudioStream)]
    pub async fn activate_studio_stream_js(
        &self,
        studio_id: String,
        opts: models::ActivationRequestOpts,
    ) -> Result<models::LiveStream, ApiError> {
        self.activate_studio_stream(&studio_id, &opts).await
    }

    #[wasm_bindgen(js_name = getSimulcastDestinations)]
    pub async fn get_simulcast_destinations_js(&self, studio_id: String) -> Result<JsValue, ApiError> {
        let destinations = self.get_simulcast_destinations(&studio_id).await?;
        to_js_value(&destinations)
    }

    #[wasm_bindgen(js_name = updateSimulcastDestination)]
    pub async fn update_simulcast_destination_js(
        &self,
        studio_id: String,
        destination: String,
        config_data: models::SimulcastDestination,
    ) -> Result<(), ApiError> {
        self.update_simulcast_destination(&studio_id, &destination, &config_data).await
    }

    #[wasm_bindgen(js_name = deleteSimulcastDestination)]
    pub async fn delete_simulcast_destination_js(&self, studio_id: String, destination: String) -> Result<(), ApiError> {
        self.delete_simulcast_destination(&studio_id, &destination).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::api::test_helpers::{assert_http_status, mock_api, mock_empty, mock_json};

    fn api(client: &ApiClient) -> StreamsApi {
        StreamsApi::from_client(client)
    }

    #[tokio::test]
    async fn test_list_streams_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/streams",
            200,
            r#"[{"id":"stream1","name":"Test Stream"}]"#,
        )
        .await;

        let result = api(&client).list_streams().await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, Some("stream1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_streams_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/streams", 500, "boom").await;

        let err = api(&client).list_streams().await.unwrap_err();
        assert_http_status(err, 500);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_search_streams_with_filters_and_pagination() {
        let (mut server, client) = mock_api().await;
        // Assert every supported filter is serialized with the spec's query-key
        // casing (e.g. `lookingFor`, `skillLevel`).
        let mock = server
            .mock("GET", "/streams/search")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("q".into(), "jazz".into()),
                mockito::Matcher::UrlEncoded("lookingFor".into(), "2".into()),
                mockito::Matcher::UrlEncoded("skillLevel".into(), "intermediate".into()),
                mockito::Matcher::UrlEncoded("instrument".into(), "sax".into()),
                mockito::Matcher::UrlEncoded("genre".into(), "jazz".into()),
                mockito::Matcher::UrlEncoded("region".into(), "ec2-us-north-ca".into()),
                mockito::Matcher::UrlEncoded("page".into(), "2".into()),
                mockito::Matcher::UrlEncoded("limit".into(), "5".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"_meta":{"total":6,"pages":2,"current":2,"count":1,"limit":5},"results":[{"id":"s9","name":"jazz","serverId":"studio-9","lookingFor":2}]}"#)
            .create_async()
            .await;

        let query = models::StreamSearchQuery {
            q: Some("jazz".to_string()),
            looking_for: Some(2),
            skill_level: Some("intermediate".to_string()),
            instrument: Some("sax".to_string()),
            genre: Some("jazz".to_string()),
            region: Some("ec2-us-north-ca".to_string()),
            page: Some(2),
            limit: Some(5),
        };
        let result = api(&client).search_streams(&query).await.unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.meta.current, 2);
        assert_eq!(result.results[0].base.name, Some("jazz".to_string()));
        assert_eq!(result.results[0].server_id, Some("studio-9".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_search_streams_empty_query() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/streams/search",
            200,
            r#"{"_meta":{"total":0,"pages":0,"current":1,"count":0,"limit":10},"results":[]}"#,
        )
        .await;

        let result = api(&client).search_streams(&models::StreamSearchQuery::default()).await.unwrap();
        assert!(result.results.is_empty());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_stream_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/streams/stream123",
            200,
            r#"{"id":"stream123","name":"My Stream","views":42}"#,
        )
        .await;

        let stream = api(&client).get_stream("stream123").await.unwrap();
        assert_eq!(stream.base.id, Some("stream123".to_string()));
        assert_eq!(stream.views, Some(42));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_stream_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/streams/nonexistent", 404, "nope").await;

        let err = api(&client).get_stream("nonexistent").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_follow_stream() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "POST", "/streams/s1/follow", 204).await;

        api(&client).follow_stream("s1").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_unfollow_stream() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/streams/s1/follow", 204).await;

        api(&client).unfollow_stream("s1").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_channels() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/channels", 200, r#"[{"id":"c1"}]"#).await;

        let result = api(&client).list_channels().await.unwrap();
        assert_eq!(result[0].id, Some("c1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_channels_paginated_with_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/channels-paginated",
            200,
            r#"{"_meta":{"total":11,"pages":2,"current":2,"count":1,"limit":10},"results":[{"id":"c1"}]}"#,
        )
        .await;

        let result = api(&client)
            .list_channels_paginated(Some(2), Some(10))
            .await
            .unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.meta.current, 2);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_channels_paginated_no_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/channels-paginated",
            200,
            r#"{"_meta":{"total":0,"pages":0,"current":1,"count":0,"limit":10},"results":[]}"#,
        )
        .await;

        let result = api(&client).list_channels_paginated(None, None).await.unwrap();
        assert_eq!(result.results.len(), 0);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_stream_conversations_is_paginated_and_filtered() {
        // Regression: decoded as a bare array, but the API returns a
        // paginated envelope.
        let (mut server, client) = mock_api().await;
        let mock = server
            .mock("GET", "/streams/s1/conversations")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("userId".into(), "u1".into()),
                mockito::Matcher::UrlEncoded("limit".into(), "5".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"_meta":{"total":1,"pages":1,"current":1,"count":1,"limit":5},"results":[{"userId":"u1"}]}"#)
            .create_async()
            .await;

        let page = api(&client).get_stream_conversations("s1", Some("u1"), None, Some(5)).await.unwrap();
        assert_eq!(page.results[0].user_id.as_deref(), Some("u1"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_stream_conversation() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/streams/s1/conversations/u1",
            200,
            r#"{"streamId":"s1","userId":"u1"}"#,
        )
        .await;

        let conv = api(&client).get_stream_conversation("s1", "u1").await.unwrap();
        assert_eq!(conv.user_id, Some("u1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_conversation_messages_is_paginated() {
        // Regression: decoded as a bare array of messages with a `content`
        // field; the API returns a paginated envelope of `text` messages.
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/streams/s1/conversations/u1/messages",
            200,
            r#"{"_meta":{"total":1,"pages":1,"current":1,"count":1,"limit":20},"results":[{"id":"m1","text":"hi"}]}"#,
        )
        .await;

        let page = api(&client).get_conversation_messages("s1", "u1", None, None).await.unwrap();
        assert_eq!(page.results[0].text.as_deref(), Some("hi"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_send_message() {
        let (mut server, client) = mock_api().await;
        let mock = server
            .mock("POST", "/streams/s1/conversations/u1/messages")
            .match_body(mockito::Matcher::Json(serde_json::json!({"text": "yo"})))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"m2","text":"yo"}"#)
            .create_async()
            .await;

        let req = models::SendMessageRequest { text: Some("yo".to_string()), ..Default::default() };
        let msg = api(&client).send_message("s1", "u1", &req).await.unwrap();
        assert_eq!(msg.id, Some("m2".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_mark_conversation_read() {
        let (mut server, client) = mock_api().await;
        // Assert the last-read payload serializes `message_id` as `messageId`.
        let mock = server
            .mock("POST", "/streams/s1/conversations/u1/last")
            .match_body(mockito::Matcher::Json(serde_json::json!({"messageId": "m1"})))
            .with_status(204)
            .create_async()
            .await;

        let req = models::MarkReadRequest {
            message_id: Some("m1".to_string()),
        };
        api(&client).mark_conversation_read("s1", "u1", &req).await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_conversation_message() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/streams/s1/conversations/u1/messages/m1",
            200,
            r#"{"id":"m1","text":"hi"}"#,
        )
        .await;

        let msg = api(&client).get_conversation_message("s1", "u1", "m1").await.unwrap();
        assert_eq!(msg.text, Some("hi".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_conversation_message() {
        let (mut server, client) = mock_api().await;
        // Assert the update payload serializes `status` and the decoded result.
        let mock = server
            .mock("PUT", "/streams/s1/conversations/u1/messages/m1")
            .match_body(mockito::Matcher::Json(serde_json::json!({"status": 1})))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"m1","status":1}"#)
            .create_async()
            .await;

        let req = models::UpdateMessageRequest { status: Some(1) };
        let msg = api(&client)
            .update_conversation_message("s1", "u1", "m1", &req)
            .await
            .unwrap();
        assert_eq!(msg.status, Some(1));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_studio_stream() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/stream",
            200,
            r#"{"id":"ls1","serverId":"st1"}"#,
        )
        .await;

        let stream = api(&client).get_studio_stream("st1").await.unwrap();
        assert_eq!(stream.server_id.as_deref(), Some("st1"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_studio_stream() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "POST", "/studios/st1/stream", 201, r#"{"id":"ls1"}"#).await;

        let body = models::LiveStream::default();
        let stream = api(&client).create_studio_stream("st1", &body).await.unwrap();
        assert_eq!(stream.id, Some("ls1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_studio_stream() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "PUT",
            "/studios/st1/stream",
            200,
            r#"{"id":"ls1","name":"updated"}"#,
        )
        .await;

        let body = models::LiveStream::default();
        let stream = api(&client).update_studio_stream("st1", &body).await.unwrap();
        assert_eq!(stream.name, Some("updated".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_activate_studio_stream_sends_visibility() {
        // Regression: the body was `{"active": true}`, which the API ignores.
        let (mut server, client) = mock_api().await;
        let mock = server
            .mock("POST", "/studios/st1/stream/activate")
            .match_body(mockito::Matcher::Json(serde_json::json!({"visibility": 1})))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"ls1"}"#)
            .create_async()
            .await;

        let opts = models::ActivationRequestOpts {
            visibility: Some(models::BroadcastVisibility::Public),
            ..Default::default()
        };
        let stream = api(&client).activate_studio_stream("st1", &opts).await.unwrap();
        assert_eq!(stream.id.as_deref(), Some("ls1"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_simulcast_destinations() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/simulcast",
            200,
            r#"[{"name":"youtube","url":"rtmp://a.rtmp.youtube.com/live2","key":"k"}]"#,
        )
        .await;

        let result = api(&client).get_simulcast_destinations("st1").await.unwrap();
        assert_eq!(result[0].name.as_deref(), Some("youtube"));
        assert_eq!(result[0].url.as_deref(), Some("rtmp://a.rtmp.youtube.com/live2"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_simulcast_destination_accepts_empty_200() {
        // Regression: the endpoint answers 200 with no body, which failed to
        // decode as a `SimulcastDestination`.
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "PUT", "/studios/st1/simulcast/youtube", 200).await;

        let cfg = models::SimulcastDestination { name: Some("youtube".into()), ..Default::default() };
        api(&client).update_simulcast_destination("st1", "youtube", &cfg).await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_simulcast_destination() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/studios/st1/simulcast/d1", 204).await;

        api(&client)
            .delete_simulcast_destination("st1", "d1")
            .await
            .unwrap();
        mock.assert_async().await;
    }
}
