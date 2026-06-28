//! Streams API endpoints
//!
//! JackTrip Radio live streams and channel management.

use super::{to_js_value, PaginationQuery, ApiClient, ApiError, urlencode};
use crate::models;
use serde::Serialize;
use wasm_bindgen::prelude::*;

// =============================================================================
// Streams API
// =============================================================================

api_module_struct!(StreamsApi);

// =============================================================================
// Rust API (primary interface)
// =============================================================================

fn stream_follow_path(stream_id: &str) -> String {
    format!("/streams/{}/follow", urlencode(stream_id))
}

impl StreamsApi {
    /// List all public, active broadcasts
    pub async fn list_streams(&self) -> Result<Vec<models::StreamInfo>, ApiError> {
        self.client.get("/streams").await
    }

    /// Search for broadcasts by keyword.
    ///
    /// Returns the paginated `{ _meta, results }` envelope whose items carry
    /// search-specific fields ([`models::StreamInfoSearchResult`]).
    pub async fn search_streams(&self, query: Option<&str>) -> Result<models::PaginatedStreamSearchResults, ApiError> {
        #[derive(Serialize)]
        struct Query<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            q: Option<&'a str>,
        }
        self.client.get_with_query("/streams/search", &Query { q: query }).await
    }

    /// Get a broadcast by ID
    pub async fn get_stream(&self, stream_id: &str) -> Result<models::StreamInfoWithEngagement, ApiError> {
        let path = format!("/streams/{}", urlencode(stream_id));
        self.client.get(&path).await
    }

    /// Follow a broadcast
    pub async fn follow_stream(&self, stream_id: &str) -> Result<(), ApiError> {
        self.client.post_empty_no_response(&stream_follow_path(stream_id)).await
    }

    /// Unfollow a broadcast
    pub async fn unfollow_stream(&self, stream_id: &str) -> Result<(), ApiError> {
        self.client.delete(&stream_follow_path(stream_id)).await
    }

    /// List all public channels
    pub async fn list_channels(&self) -> Result<Vec<models::StreamInfo>, ApiError> {
        self.client.get("/channels").await
    }

    /// List channels with pagination
    pub async fn list_channels_paginated(
        &self,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedChannels, ApiError> {
        if page.is_some() || limit.is_some() {
            self.client.get_with_query("/channels-paginated", &PaginationQuery { page, limit }).await
        } else {
            self.client.get("/channels-paginated").await
        }
    }

    /// Get chat session for a broadcast
    pub async fn get_stream_chat(&self, stream_id: &str, chat_id: &str) -> Result<models::ChatSession, ApiError> {
        let path = format!("/streams/{}/chat/{}", urlencode(stream_id), urlencode(chat_id));
        self.client.get(&path).await
    }

    /// Get all conversations for a stream
    pub async fn get_stream_conversations(&self, stream_id: &str) -> Result<Vec<models::Conversation>, ApiError> {
        let path = format!("/streams/{}/conversations", urlencode(stream_id));
        self.client.get(&path).await
    }

    /// Get a specific conversation
    pub async fn get_stream_conversation(
        &self,
        stream_id: &str,
        user_id: &str,
    ) -> Result<models::Conversation, ApiError> {
        let path = format!("/streams/{}/conversations/{}", urlencode(stream_id), urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get messages in a conversation
    pub async fn get_conversation_messages(
        &self,
        stream_id: &str,
        user_id: &str,
    ) -> Result<Vec<models::Message>, ApiError> {
        let path = format!("/streams/{}/conversations/{}/messages", urlencode(stream_id), urlencode(user_id));
        self.client.get(&path).await
    }

    /// Send a message in a conversation
    pub async fn send_message(
        &self,
        stream_id: &str,
        user_id: &str,
        message: &models::SendMessageRequest,
    ) -> Result<models::Message, ApiError> {
        let path = format!("/streams/{}/conversations/{}/messages", urlencode(stream_id), urlencode(user_id));
        self.client.post(&path, message).await
    }

    /// Get the stream for a studio
    pub async fn get_studio_stream(&self, studio_id: &str) -> Result<models::LiveStream, ApiError> {
        let path = format!("/studios/{}/stream", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Create or reset a stream for a studio
    pub async fn create_studio_stream(
        &self,
        studio_id: &str,
        stream: &models::LiveStream,
    ) -> Result<models::LiveStream, ApiError> {
        let path = format!("/studios/{}/stream", urlencode(studio_id));
        self.client.post(&path, stream).await
    }

    /// Update a studio's stream
    pub async fn update_studio_stream(
        &self,
        studio_id: &str,
        stream: &models::LiveStream,
    ) -> Result<models::LiveStream, ApiError> {
        let path = format!("/studios/{}/stream", urlencode(studio_id));
        self.client.put(&path, stream).await
    }

    /// Activate or deactivate a studio stream
    pub async fn activate_studio_stream(
        &self,
        studio_id: &str,
        opts: &models::ActivationRequestOpts,
    ) -> Result<models::LiveStream, ApiError> {
        let path = format!("/studios/{}/stream/activate", urlencode(studio_id));
        self.client.post(&path, opts).await
    }

    /// Get simulcast destinations for a studio
    pub async fn get_simulcast_destinations(&self, studio_id: &str) -> Result<Vec<models::SimulcastDestination>, ApiError> {
        let path = format!("/studios/{}/simulcast", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Add or update a simulcast destination
    pub async fn update_simulcast_destination(
        &self,
        studio_id: &str,
        destination: &str,
        config_data: &models::SimulcastDestination,
    ) -> Result<models::SimulcastDestination, ApiError> {
        let path = format!("/studios/{}/simulcast/{}", urlencode(studio_id), urlencode(destination));
        self.client.put(&path, config_data).await
    }

    /// Remove a simulcast destination
    pub async fn delete_simulcast_destination(&self, studio_id: &str, destination: &str) -> Result<(), ApiError> {
        let path = format!("/studios/{}/simulcast/{}", urlencode(studio_id), urlencode(destination));
        self.client.delete(&path).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[wasm_bindgen]
impl StreamsApi {
    #[wasm_bindgen(js_name = listStreams)]
    pub async fn list_streams_js(&self) -> Result<JsValue, ApiError> {
        let streams = self.list_streams().await?;
        to_js_value(&streams)
    }

    #[wasm_bindgen(js_name = searchStreams)]
    pub async fn search_streams_js(&self, query: Option<String>) -> Result<JsValue, ApiError> {
        let streams = self.search_streams(query.as_deref()).await?;
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

    #[wasm_bindgen(js_name = getStreamChat)]
    pub async fn get_stream_chat_js(&self, stream_id: String, chat_id: String) -> Result<models::ChatSession, ApiError> {
        self.get_stream_chat(&stream_id, &chat_id).await
    }

    #[wasm_bindgen(js_name = getStreamConversations)]
    pub async fn get_stream_conversations_js(&self, stream_id: String) -> Result<JsValue, ApiError> {
        let conversations = self.get_stream_conversations(&stream_id).await?;
        to_js_value(&conversations)
    }

    #[wasm_bindgen(js_name = getStreamConversation)]
    pub async fn get_stream_conversation_js(
        &self,
        stream_id: String,
        user_id: String,
    ) -> Result<models::Conversation, ApiError> {
        self.get_stream_conversation(&stream_id, &user_id).await
    }

    #[wasm_bindgen(js_name = getConversationMessages)]
    pub async fn get_conversation_messages_js(&self, stream_id: String, user_id: String) -> Result<JsValue, ApiError> {
        let messages = self.get_conversation_messages(&stream_id, &user_id).await?;
        to_js_value(&messages)
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
    ) -> Result<models::SimulcastDestination, ApiError> {
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
    async fn test_search_streams_with_query() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/streams/search",
            200,
            r#"{"_meta":{"total":1,"pages":1,"current":1,"count":1,"limit":10},"results":[{"id":"s9","name":"jazz","serverId":"studio-9","lookingFor":2}]}"#,
        )
        .await;

        let result = api(&client).search_streams(Some("jazz")).await.unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.results[0].base.name, Some("jazz".to_string()));
        assert_eq!(result.results[0].server_id, Some("studio-9".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_search_streams_without_query() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/streams/search",
            200,
            r#"{"_meta":{"total":0,"pages":0,"current":1,"count":0,"limit":10},"results":[]}"#,
        )
        .await;

        let result = api(&client).search_streams(None).await.unwrap();
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
            r#"{"id":"stream123","name":"My Stream","followers":42}"#,
        )
        .await;

        let stream = api(&client).get_stream("stream123").await.unwrap();
        assert_eq!(stream.base.id, Some("stream123".to_string()));
        assert_eq!(stream.followers, Some(42));
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
    async fn test_get_stream_chat() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/streams/s1/chat/chat1",
            200,
            r#"{"id":"chat1","roomId":"r1"}"#,
        )
        .await;

        let chat = api(&client).get_stream_chat("s1", "chat1").await.unwrap();
        assert_eq!(chat.id, Some("chat1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_stream_conversations() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/streams/s1/conversations",
            200,
            r#"[{"id":"conv1"}]"#,
        )
        .await;

        let result = api(&client).get_stream_conversations("s1").await.unwrap();
        assert_eq!(result[0].id, Some("conv1".to_string()));
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
            r#"{"id":"conv1","userId":"u1"}"#,
        )
        .await;

        let conv = api(&client).get_stream_conversation("s1", "u1").await.unwrap();
        assert_eq!(conv.user_id, Some("u1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_conversation_messages() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/streams/s1/conversations/u1/messages",
            200,
            r#"[{"id":"m1","content":"hi"}]"#,
        )
        .await;

        let result = api(&client).get_conversation_messages("s1", "u1").await.unwrap();
        assert_eq!(result[0].content, Some("hi".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_send_message() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/streams/s1/conversations/u1/messages",
            200,
            r#"{"id":"m2","content":"yo"}"#,
        )
        .await;

        let req = models::SendMessageRequest {
            content: Some("yo".to_string()),
            ..Default::default()
        };
        let msg = api(&client).send_message("s1", "u1", &req).await.unwrap();
        assert_eq!(msg.id, Some("m2".to_string()));
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
            r#"{"id":"ls1","active":true}"#,
        )
        .await;

        let stream = api(&client).get_studio_stream("st1").await.unwrap();
        assert_eq!(stream.active, Some(true));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_studio_stream() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/studios/st1/stream",
            200,
            r#"{"id":"ls1"}"#,
        )
        .await;

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
    async fn test_activate_studio_stream() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/studios/st1/stream/activate",
            200,
            r#"{"id":"ls1","active":true}"#,
        )
        .await;

        let opts = models::ActivationRequestOpts { active: Some(true) };
        let stream = api(&client).activate_studio_stream("st1", &opts).await.unwrap();
        assert_eq!(stream.active, Some(true));
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
            r#"[{"id":"d1","platform":"youtube"}]"#,
        )
        .await;

        let result = api(&client).get_simulcast_destinations("st1").await.unwrap();
        assert_eq!(result[0].platform, Some("youtube".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_simulcast_destination() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "PUT",
            "/studios/st1/simulcast/d1",
            200,
            r#"{"id":"d1","enabled":true}"#,
        )
        .await;

        let cfg = models::SimulcastDestination::default();
        let dest = api(&client)
            .update_simulcast_destination("st1", "d1", &cfg)
            .await
            .unwrap();
        assert_eq!(dest.enabled, Some(true));
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
