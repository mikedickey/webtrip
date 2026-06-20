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

    /// Search for broadcasts by keyword
    pub async fn search_streams(&self, query: Option<&str>) -> Result<Vec<models::StreamInfo>, ApiError> {
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
    use mockito;

    #[tokio::test]
    async fn test_list_streams_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/streams")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[{"id":"stream1","name":"Test Stream"}]"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = StreamsApi::from_client(&client);
        let result = api.list_streams().await;

        assert!(result.is_ok());
        let streams = result.unwrap();
        assert_eq!(streams.len(), 1);
        assert_eq!(streams[0].id, Some("stream1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_streams_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/streams")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = StreamsApi::from_client(&client);
        let result = api.list_streams().await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Http { status, .. } => assert_eq!(status, 500),
            _ => panic!("Expected HTTP error"),
        }
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_stream_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/streams/stream123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"stream123","name":"My Stream","followers":42}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = StreamsApi::from_client(&client);
        let result = api.get_stream("stream123").await;

        assert!(result.is_ok());
        let stream = result.unwrap();
        assert_eq!(stream.id, Some("stream123".to_string()));
        assert_eq!(stream.name, Some("My Stream".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_stream_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/streams/nonexistent")
            .with_status(404)
            .with_body("Not Found")
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = StreamsApi::from_client(&client);
        let result = api.get_stream("nonexistent").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Http { status, .. } => assert_eq!(status, 404),
            _ => panic!("Expected HTTP error"),
        }
        mock.assert_async().await;
    }
}
