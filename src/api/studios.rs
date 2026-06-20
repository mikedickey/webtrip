//! Studios API endpoints
//!
//! Virtual studio management, configuration, and related operations.

use super::{api_module_struct, to_js_value, ApiClient, ApiError, urlencode};
use crate::models;
use wasm_bindgen::prelude::*;

// =============================================================================
// Studios API
// =============================================================================

/// Studios API for virtual studio management
api_module_struct!(StudiosApi);

// =============================================================================
// Rust API (primary interface)
// =============================================================================

impl StudiosApi {
    /// List all studios for the authenticated user
    pub async fn list_studios(&self) -> Result<Vec<models::Studio>, ApiError> {
        self.client.get("/studios").await
    }

    /// Create a new studio
    pub async fn create_studio(&self, studio: &models::Studio) -> Result<models::Studio, ApiError> {
        self.client.post("/studios", studio).await
    }

    /// Get a studio by ID
    pub async fn get_studio(&self, studio_id: &str) -> Result<models::Studio, ApiError> {
        let path = format!("/studios/{}", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Update a studio's configuration
    pub async fn update_studio(&self, studio_id: &str, studio: &models::Studio) -> Result<models::Studio, ApiError> {
        let path = format!("/studios/{}", urlencode(studio_id));
        self.client.put(&path, studio).await
    }

    /// Delete a studio
    pub async fn delete_studio(&self, studio_id: &str) -> Result<(), ApiError> {
        let path = format!("/studios/{}", urlencode(studio_id));
        self.client.delete(&path).await
    }

    /// Extend a studio's expiration time
    pub async fn extend_studio(&self, studio_id: &str) -> Result<models::Studio, ApiError> {
        let path = format!("/studios/{}/extend", urlencode(studio_id));
        self.client.post_empty(&path).await
    }

    /// Get access settings for a studio
    pub async fn get_access_settings(&self, studio_id: &str) -> Result<models::AccessSettings, ApiError> {
        let path = format!("/studios/{}/access", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Update access settings for a studio
    pub async fn update_access_settings(
        &self,
        studio_id: &str,
        settings: &models::AccessSettings,
    ) -> Result<models::AccessSettings, ApiError> {
        let path = format!("/studios/{}/access", urlencode(studio_id));
        self.client.put(&path, settings).await
    }

    /// Get the mixer configuration for a studio
    pub async fn get_mixer(&self, studio_id: &str) -> Result<models::Mixer, ApiError> {
        let path = format!("/studios/{}/mixer", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Update the mixer configuration for a studio
    pub async fn update_mixer(&self, studio_id: &str, mixer: &models::Mixer) -> Result<models::Mixer, ApiError> {
        let path = format!("/studios/{}/mixer", urlencode(studio_id));
        self.client.put(&path, mixer).await
    }

    /// Get all mixers
    pub async fn list_mixers(&self) -> Result<Vec<models::Mixer>, ApiError> {
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
    pub async fn create_studio_js(&self, studio: models::Studio) -> Result<models::Studio, ApiError> {
        self.create_studio(&studio).await
    }

    #[wasm_bindgen(js_name = getStudio)]
    pub async fn get_studio_js(&self, studio_id: String) -> Result<models::Studio, ApiError> {
        self.get_studio(&studio_id).await
    }

    #[wasm_bindgen(js_name = updateStudio)]
    pub async fn update_studio_js(&self, studio_id: String, studio: models::Studio) -> Result<models::Studio, ApiError> {
        self.update_studio(&studio_id, &studio).await
    }

    #[wasm_bindgen(js_name = deleteStudio)]
    pub async fn delete_studio_js(&self, studio_id: String) -> Result<(), ApiError> {
        self.delete_studio(&studio_id).await
    }

    #[wasm_bindgen(js_name = extendStudio)]
    pub async fn extend_studio_js(&self, studio_id: String) -> Result<models::Studio, ApiError> {
        self.extend_studio(&studio_id).await
    }

    #[wasm_bindgen(js_name = getAccessSettings)]
    pub async fn get_access_settings_js(&self, studio_id: String) -> Result<models::AccessSettings, ApiError> {
        self.get_access_settings(&studio_id).await
    }

    #[wasm_bindgen(js_name = updateAccessSettings)]
    pub async fn update_access_settings_js(
        &self,
        studio_id: String,
        settings: models::AccessSettings,
    ) -> Result<models::AccessSettings, ApiError> {
        self.update_access_settings(&studio_id, &settings).await
    }

    #[wasm_bindgen(js_name = getMixer)]
    pub async fn get_mixer_js(&self, studio_id: String) -> Result<models::Mixer, ApiError> {
        self.get_mixer(&studio_id).await
    }

    #[wasm_bindgen(js_name = updateMixer)]
    pub async fn update_mixer_js(&self, studio_id: String, mixer: models::Mixer) -> Result<models::Mixer, ApiError> {
        self.update_mixer(&studio_id, &mixer).await
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

    #[wasm_bindgen(js_name = getSession)]
    pub async fn get_session_js(&self, studio_id: String) -> Result<models::Session, ApiError> {
        self.get_session(&studio_id).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use mockito;

    #[tokio::test]
    async fn test_list_studios_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/studios")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[{"id":"studio1","name":"Test Studio"}]"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = StudiosApi::from_client(&client);
        let result = api.list_studios().await;

        assert!(result.is_ok());
        let studios = result.unwrap();
        assert_eq!(studios.len(), 1);
        assert_eq!(studios[0].id, Some("studio1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_studios_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/studios")
            .with_status(401)
            .with_body("Unauthorized")
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = StudiosApi::from_client(&client);
        let result = api.list_studios().await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Http { status, .. } => assert_eq!(status, 401),
            _ => panic!("Expected HTTP error"),
        }
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_studio_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/studios/studio123")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"studio123","name":"My Studio","enabled":true}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = StudiosApi::from_client(&client);
        let result = api.get_studio("studio123").await;

        assert!(result.is_ok());
        let studio = result.unwrap();
        assert_eq!(studio.id, Some("studio123".to_string()));
        assert_eq!(studio.name, Some("My Studio".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_studio_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/studios/nonexistent")
            .with_status(404)
            .with_body("Not Found")
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = StudiosApi::from_client(&client);
        let result = api.get_studio("nonexistent").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Http { status, .. } => assert_eq!(status, 404),
            _ => panic!("Expected HTTP error"),
        }
        mock.assert_async().await;
    }
}
