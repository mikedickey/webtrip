//! Studios API endpoints
//!
//! Virtual studio management, configuration, and related operations.

use super::{to_js_value, ApiClient, ApiError, urlencode};
use crate::models;
use wasm_bindgen::prelude::*;

// =============================================================================
// Studios API
// =============================================================================

api_module_struct!(StudiosApi);

// =============================================================================
// Rust API (primary interface)
// =============================================================================

impl StudiosApi {
    /// List all studios for the authenticated user.
    ///
    /// Responses carry the caller's relationship to each studio (`admin`,
    /// `owner`, `subStatus`), so they deserialize into [`models::ServerWithSubscription`].
    pub async fn list_studios(&self) -> Result<Vec<models::ServerWithSubscription>, ApiError> {
        self.client.get("/studios").await
    }

    /// Create a new studio. The request payload is a [`models::Server`]; the
    /// response includes the caller's relationship fields.
    pub async fn create_studio(&self, studio: &models::Server) -> Result<models::ServerWithSubscription, ApiError> {
        self.client.post("/studios", studio).await
    }

    /// Get a studio by ID
    pub async fn get_studio(&self, studio_id: &str) -> Result<models::ServerWithSubscription, ApiError> {
        let path = format!("/studios/{}", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Update a studio's configuration
    pub async fn update_studio(&self, studio_id: &str, studio: &models::Server) -> Result<models::ServerWithSubscription, ApiError> {
        let path = format!("/studios/{}", urlencode(studio_id));
        self.client.put(&path, studio).await
    }

    /// Delete a studio
    pub async fn delete_studio(&self, studio_id: &str) -> Result<(), ApiError> {
        let path = format!("/studios/{}", urlencode(studio_id));
        self.client.delete(&path).await
    }

    /// Extend a studio's expiration time.
    ///
    /// The endpoint responds with `202 Accepted` (extended) or `204 No Content`
    /// (no extension needed) and never returns a body.
    pub async fn extend_studio(&self, studio_id: &str) -> Result<(), ApiError> {
        let path = format!("/studios/{}/extend", urlencode(studio_id));
        self.client.post_empty_no_response(&path).await
    }

    /// Get the authenticated user's access rights for a studio
    pub async fn get_access_settings(&self, studio_id: &str) -> Result<models::ServerAccess, ApiError> {
        let path = format!("/studios/{}/access", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Update a studio's banner image (also used for its JackTrip Radio
    /// broadcast banner). The payload is the raw image bytes; the endpoint
    /// responds `200` with no body.
    pub async fn update_banner(
        &self,
        studio_id: &str,
        image: Vec<u8>,
        content_type: &str,
    ) -> Result<(), ApiError> {
        let path = format!("/studios/{}/banner", urlencode(studio_id));
        self.client.put_bytes(&path, image, content_type).await
    }

    /// Get all mixers, keyed by mixer name (`GET /mixers` returns a map)
    pub async fn list_mixers(&self) -> Result<std::collections::HashMap<String, models::Mixer>, ApiError> {
        self.client.get("/mixers").await
    }

    /// Get a LiveKit token for the studio
    pub async fn get_livekit_token(&self, studio_id: &str) -> Result<models::LiveKitTokenResponse, ApiError> {
        let path = format!("/studios/{}/lktoken", urlencode(studio_id));
        self.client.post_empty(&path).await
    }

    /// Send an invite for a studio
    pub async fn send_invite(&self, studio_id: &str, invite: &models::InviteRequest) -> Result<(), ApiError> {
        let path = format!("/studios/{}/invite", urlencode(studio_id));
        self.client.post_no_response(&path, invite).await
    }

    /// Submit feedback for a studio session
    pub async fn submit_feedback(&self, studio_id: &str, feedback: &models::FeedbackRequest) -> Result<(), ApiError> {
        let path = format!("/studios/{}/feedback", urlencode(studio_id));
        self.client.post_no_response(&path, feedback).await
    }

    /// Get chat session for a studio
    pub async fn get_chat(&self, studio_id: &str, chat_id: &str) -> Result<models::ChatSession, ApiError> {
        let path = format!("/studios/{}/chat/{}", urlencode(studio_id), urlencode(chat_id));
        self.client.get(&path).await
    }

    /// Get participants in a studio
    pub async fn get_participants(&self, studio_id: &str) -> Result<Vec<models::Participant>, ApiError> {
        let path = format!("/studios/{}/participants", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Get a single studio participant's full user metadata by user ID.
    ///
    /// The spec returns a complete [`models::User`] (not the lighter
    /// session-scoped `Participant` from [`Self::get_participants`]).
    pub async fn get_participant(&self, studio_id: &str, user_id: &str) -> Result<models::User, ApiError> {
        let path = format!(
            "/studios/{}/participants/{}",
            urlencode(studio_id),
            urlencode(user_id)
        );
        self.client.get(&path).await
    }

    /// Get the current session for a studio
    pub async fn get_session(&self, studio_id: &str) -> Result<models::Session, ApiError> {
        let path = format!("/studios/{}/session", urlencode(studio_id));
        self.client.get(&path).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[wasm_bindgen]
impl StudiosApi {
    #[wasm_bindgen(js_name = listStudios)]
    pub async fn list_studios_js(&self) -> Result<JsValue, ApiError> {
        let studios = self.list_studios().await?;
        to_js_value(&studios)
    }

    #[wasm_bindgen(js_name = createStudio)]
    pub async fn create_studio_js(&self, studio: models::Server) -> Result<models::ServerWithSubscription, ApiError> {
        self.create_studio(&studio).await
    }

    #[wasm_bindgen(js_name = getStudio)]
    pub async fn get_studio_js(&self, studio_id: String) -> Result<models::ServerWithSubscription, ApiError> {
        self.get_studio(&studio_id).await
    }

    #[wasm_bindgen(js_name = updateStudio)]
    pub async fn update_studio_js(&self, studio_id: String, studio: models::Server) -> Result<models::ServerWithSubscription, ApiError> {
        self.update_studio(&studio_id, &studio).await
    }

    #[wasm_bindgen(js_name = deleteStudio)]
    pub async fn delete_studio_js(&self, studio_id: String) -> Result<(), ApiError> {
        self.delete_studio(&studio_id).await
    }

    #[wasm_bindgen(js_name = extendStudio)]
    pub async fn extend_studio_js(&self, studio_id: String) -> Result<(), ApiError> {
        self.extend_studio(&studio_id).await
    }

    #[wasm_bindgen(js_name = getAccessSettings)]
    pub async fn get_access_settings_js(&self, studio_id: String) -> Result<models::ServerAccess, ApiError> {
        self.get_access_settings(&studio_id).await
    }

    #[wasm_bindgen(js_name = updateBanner)]
    pub async fn update_banner_js(
        &self,
        studio_id: String,
        image: Vec<u8>,
        content_type: String,
    ) -> Result<(), ApiError> {
        self.update_banner(&studio_id, image, &content_type).await
    }

    #[wasm_bindgen(js_name = listMixers)]
    pub async fn list_mixers_js(&self) -> Result<JsValue, ApiError> {
        let mixers = self.list_mixers().await?;
        to_js_value(&mixers)
    }

    #[wasm_bindgen(js_name = getLivekitToken)]
    pub async fn get_livekit_token_js(&self, studio_id: String) -> Result<models::LiveKitTokenResponse, ApiError> {
        self.get_livekit_token(&studio_id).await
    }

    #[wasm_bindgen(js_name = sendInvite)]
    pub async fn send_invite_js(&self, studio_id: String, invite: models::InviteRequest) -> Result<(), ApiError> {
        self.send_invite(&studio_id, &invite).await
    }

    #[wasm_bindgen(js_name = submitFeedback)]
    pub async fn submit_feedback_js(&self, studio_id: String, feedback: models::FeedbackRequest) -> Result<(), ApiError> {
        self.submit_feedback(&studio_id, &feedback).await
    }

    #[wasm_bindgen(js_name = getChat)]
    pub async fn get_chat_js(&self, studio_id: String, chat_id: String) -> Result<models::ChatSession, ApiError> {
        self.get_chat(&studio_id, &chat_id).await
    }

    #[wasm_bindgen(js_name = getParticipants)]
    pub async fn get_participants_js(&self, studio_id: String) -> Result<JsValue, ApiError> {
        let participants = self.get_participants(&studio_id).await?;
        to_js_value(&participants)
    }

    #[wasm_bindgen(js_name = getParticipant)]
    pub async fn get_participant_js(&self, studio_id: String, user_id: String) -> Result<models::User, ApiError> {
        self.get_participant(&studio_id, &user_id).await
    }

    #[wasm_bindgen(js_name = getSession)]
    pub async fn get_session_js(&self, studio_id: String) -> Result<models::Session, ApiError> {
        self.get_session(&studio_id).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::api::test_helpers::{assert_http_status, mock_api, mock_empty, mock_json};

    fn api(client: &ApiClient) -> StudiosApi {
        StudiosApi::from_client(client)
    }

    #[tokio::test]
    async fn test_list_studios_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios",
            200,
            r#"[{"id":"studio1","name":"Test Studio"}]"#,
        )
        .await;

        let result = api(&client).list_studios().await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].server.id, Some("studio1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_studios_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/studios", 401, "Unauthorized").await;

        let err = api(&client).list_studios().await.unwrap_err();
        assert_http_status(err, 401);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_studio_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/studios",
            200,
            r#"{"id":"new1","name":"Created"}"#,
        )
        .await;

        let body = models::Server::default();
        let studio = api(&client).create_studio(&body).await.unwrap();
        assert_eq!(studio.server.id, Some("new1".to_string()));
        assert_eq!(studio.server.config.name, Some("Created".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_studio_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/studio123",
            200,
            r#"{"id":"studio123","name":"My Studio","enabled":true}"#,
        )
        .await;

        let studio = api(&client).get_studio("studio123").await.unwrap();
        assert_eq!(studio.server.id, Some("studio123".to_string()));
        assert_eq!(studio.server.config.name, Some("My Studio".to_string()));
        assert_eq!(studio.server.config.enabled, Some(true));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_studio_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/studios/nonexistent", 404, "Not Found").await;

        let err = api(&client).get_studio("nonexistent").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_studio_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "PUT",
            "/studios/st1",
            200,
            r#"{"id":"st1","name":"Updated"}"#,
        )
        .await;

        let body = models::Server::default();
        let studio = api(&client).update_studio("st1", &body).await.unwrap();
        assert_eq!(studio.server.id, Some("st1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_studio_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/studios/st1", 204).await;

        api(&client).delete_studio("st1").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_studio_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/studios/st1", 403).await;

        let err = api(&client).delete_studio("st1").await.unwrap_err();
        assert_http_status(err, 403);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_extend_studio_success() {
        // Spec: 202 (extended) or 204 (no extension needed), no JSON body.
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "POST", "/studios/st1/extend", 202).await;

        api(&client).extend_studio("st1").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_access_settings_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/access",
            200,
            r#"{"serverId":"st1","userId":"u1","admin":true,"owner":false}"#,
        )
        .await;

        let settings = api(&client).get_access_settings("st1").await.unwrap();
        assert_eq!(settings.server_id, Some("st1".to_string()));
        assert_eq!(settings.admin, Some(true));
        assert_eq!(settings.owner, Some(false));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_banner_success() {
        // Spec: PUT /studios/{id}/banner uploads raw image bytes, 200 no body.
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "PUT", "/studios/st1/banner", 200).await;

        api(&client)
            .update_banner("st1", b"\x89PNG\r\n".to_vec(), "image/png")
            .await
            .unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_banner_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "PUT", "/studios/st1/banner", 403).await;

        let err = api(&client)
            .update_banner("st1", b"img".to_vec(), "image/png")
            .await
            .unwrap_err();
        assert_http_status(err, 403);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_mixers_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/mixers", 200, r#"{"default":{"type":"sclang"}}"#).await;

        let result = api(&client).list_mixers().await.unwrap();
        assert_eq!(result.get("default").and_then(|m| m.mixer_type.clone()), Some("sclang".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_livekit_token_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/studios/st1/lktoken",
            200,
            r#"{"token":"tok123","url":"wss://lk"}"#,
        )
        .await;

        let resp = api(&client).get_livekit_token("st1").await.unwrap();
        assert_eq!(resp.token, Some("tok123".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_send_invite_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "POST", "/studios/st1/invite", 204).await;

        let body = models::InviteRequest::default();
        api(&client).send_invite("st1", &body).await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_submit_feedback_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "POST", "/studios/st1/feedback", 204).await;

        let body = models::FeedbackRequest::default();
        api(&client).submit_feedback("st1", &body).await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_chat_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/chat/chat1",
            200,
            r#"{"id":"chat1","roomId":"r1"}"#,
        )
        .await;

        let chat = api(&client).get_chat("st1", "chat1").await.unwrap();
        assert_eq!(chat.id, Some("chat1".to_string()));
        assert_eq!(chat.room_id, Some("r1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_participants_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/participants",
            200,
            r#"[{"userId":"u1","name":"Alice"}]"#,
        )
        .await;

        let result = api(&client).get_participants("st1").await.unwrap();
        assert_eq!(result[0].user_id, Some("u1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_participant_success() {
        // Spec: GET /studios/{id}/participants/{userId} returns a full User.
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/participants/u1",
            200,
            r#"{"user_id":"u1","name":"Alice","nickname":"al"}"#,
        )
        .await;

        let user = api(&client).get_participant("st1", "u1").await.unwrap();
        assert_eq!(user.user_id, Some("u1".to_string()));
        assert_eq!(user.name, Some("Alice".to_string()));
        assert_eq!(user.nickname, Some("al".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_participant_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/studios/st1/participants/u9", 404, "Not Found").await;

        let err = api(&client).get_participant("st1", "u9").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_session_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/session",
            200,
            r#"{"id":"sess1","studioId":"st1"}"#,
        )
        .await;

        let session = api(&client).get_session("st1").await.unwrap();
        assert_eq!(session.id, Some("sess1".to_string()));
        assert_eq!(session.studio_id, Some("st1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_session_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/studios/st1/session", 500, "boom").await;

        let err = api(&client).get_session("st1").await.unwrap_err();
        assert_http_status(err, 500);
        mock.assert_async().await;
    }
}
