//! Users API endpoints
//!
//! User profile management, preferences, and related operations.

use super::{api_module_struct, to_js_value, ApiClient, ApiError, urlencode};
use crate::models;
use serde::Serialize;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

// =============================================================================
// Users API
// =============================================================================

/// Users API for profile and account management
api_module_struct!(UsersApi);

// =============================================================================
// Rust API (primary interface)
// =============================================================================

impl UsersApi {
    /// Get the currently authenticated user
    pub async fn get_current_user(&self) -> Result<models::User, ApiError> {
        self.client.get("/users/me").await
    }

    /// Search for users
    pub async fn search_users(&self, query: &str) -> Result<Vec<models::User>, ApiError> {
        #[derive(Serialize)]
        struct Query<'a> {
            q: &'a str,
        }
        self.client.get_with_query("/users", &Query { q: query }).await
    }

    /// Get a user by ID
    pub async fn get_user(&self, user_id: &str) -> Result<models::User, ApiError> {
        let path = format!("/users/{}", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Update a user's metadata
    pub async fn update_user(
        &self,
        user_id: &str,
        metadata: &models::UserMetadata,
    ) -> Result<models::UserMetadata, ApiError> {
        let path = format!("/users/{}", urlencode(user_id));
        self.client.put(&path, metadata).await
    }

    /// Delete a user account
    pub async fn delete_user(&self, user_id: &str) -> Result<(), ApiError> {
        let path = format!("/users/{}", urlencode(user_id));
        self.client.delete(&path).await
    }

    /// Get all regions available to a user
    pub async fn get_user_regions(&self, user_id: &str) -> Result<Vec<models::Region>, ApiError> {
        let path = format!("/users/{}/regions", urlencode(user_id));
        let map: HashMap<String, models::Region> = self.client.get(&path).await?;
        Ok(map.into_iter().map(|(id, mut r)| { r.id = Some(id); r }).collect())
    }

    /// Get a user's notifications
    pub async fn get_notifications(&self, user_id: &str) -> Result<Vec<models::Notification>, ApiError> {
        let path = format!("/users/{}/notifications", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get a user's conversations
    pub async fn get_conversations(&self, user_id: &str) -> Result<Vec<models::Conversation>, ApiError> {
        let path = format!("/users/{}/conversations", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get unread message count for a user
    pub async fn get_unread_messages_count(&self, user_id: &str) -> Result<models::UnreadMessagesResponse, ApiError> {
        let path = format!("/users/{}/unread-messages", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get a user's referrals
    pub async fn get_referrals(&self, user_id: &str) -> Result<Vec<models::Referral>, ApiError> {
        let path = format!("/users/{}/referrals", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Create a new referral
    pub async fn create_referral(&self, user_id: &str) -> Result<models::Referral, ApiError> {
        let path = format!("/users/{}/referrals", urlencode(user_id));
        self.client.post_empty(&path).await
    }

    /// Get paginated channels the user is a member of
    pub async fn get_user_channels(
        &self,
        user_id: &str,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedChannels, ApiError> {
        let path = format!("/users/{}/channels-paginated", urlencode(user_id));
        
        #[derive(Serialize)]
        struct Query {
            #[serde(skip_serializing_if = "Option::is_none")]
            page: Option<i32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<i32>,
        }
        
        if page.is_some() || limit.is_some() {
            self.client.get_with_query(&path, &Query { page, limit }).await
        } else {
            self.client.get(&path).await
        }
    }

    /// Get paginated channels the user follows
    pub async fn get_user_follows(
        &self,
        user_id: &str,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedChannels, ApiError> {
        let path = format!("/users/{}/follows-paginated", urlencode(user_id));
        
        #[derive(Serialize)]
        struct Query {
            #[serde(skip_serializing_if = "Option::is_none")]
            page: Option<i32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<i32>,
        }
        
        if page.is_some() || limit.is_some() {
            self.client.get_with_query(&path, &Query { page, limit }).await
        } else {
            self.client.get(&path).await
        }
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[wasm_bindgen]
impl UsersApi {
    #[wasm_bindgen(js_name = getCurrentUser)]
    pub async fn get_current_user_js(&self) -> Result<models::User, ApiError> {
        self.get_current_user().await
    }

    #[wasm_bindgen(js_name = searchUsers)]
    pub async fn search_users_js(&self, query: String) -> Result<JsValue, ApiError> {
        let users = self.search_users(&query).await?;
        to_js_value(&users)
    }

    #[wasm_bindgen(js_name = getUser)]
    pub async fn get_user_js(&self, user_id: String) -> Result<models::User, ApiError> {
        self.get_user(&user_id).await
    }

    #[wasm_bindgen(js_name = updateUser)]
    pub async fn update_user_js(
        &self,
        user_id: String,
        metadata: models::UserMetadata,
    ) -> Result<models::UserMetadata, ApiError> {
        self.update_user(&user_id, &metadata).await
    }

    #[wasm_bindgen(js_name = deleteUser)]
    pub async fn delete_user_js(&self, user_id: String) -> Result<(), ApiError> {
        self.delete_user(&user_id).await
    }

    #[wasm_bindgen(js_name = getUserRegions)]
    pub async fn get_user_regions_js(&self, user_id: String) -> Result<JsValue, ApiError> {
        let regions = self.get_user_regions(&user_id).await?;
        to_js_value(&regions)
    }

    #[wasm_bindgen(js_name = getNotifications)]
    pub async fn get_notifications_js(&self, user_id: String) -> Result<JsValue, ApiError> {
        let notifications = self.get_notifications(&user_id).await?;
        serde_wasm_bindgen::to_value(&notifications).map_err(|e| ApiError::Serialization(e.to_string()))
    }

    #[wasm_bindgen(js_name = getConversations)]
    pub async fn get_conversations_js(&self, user_id: String) -> Result<JsValue, ApiError> {
        let conversations = self.get_conversations(&user_id).await?;
        serde_wasm_bindgen::to_value(&conversations).map_err(|e| ApiError::Serialization(e.to_string()))
    }

    #[wasm_bindgen(js_name = getUnreadMessagesCount)]
    pub async fn get_unread_messages_count_js(&self, user_id: String) -> Result<models::UnreadMessagesResponse, ApiError> {
        self.get_unread_messages_count(&user_id).await
    }

    #[wasm_bindgen(js_name = getReferrals)]
    pub async fn get_referrals_js(&self, user_id: String) -> Result<JsValue, ApiError> {
        let referrals = self.get_referrals(&user_id).await?;
        serde_wasm_bindgen::to_value(&referrals).map_err(|e| ApiError::Serialization(e.to_string()))
    }

    #[wasm_bindgen(js_name = createReferral)]
    pub async fn create_referral_js(&self, user_id: String) -> Result<models::Referral, ApiError> {
        self.create_referral(&user_id).await
    }

    #[wasm_bindgen(js_name = getUserChannels)]
    pub async fn get_user_channels_js(
        &self,
        user_id: String,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedChannels, ApiError> {
        self.get_user_channels(&user_id, page, limit).await
    }

    #[wasm_bindgen(js_name = getUserFollows)]
    pub async fn get_user_follows_js(
        &self,
        user_id: String,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedChannels, ApiError> {
        self.get_user_follows(&user_id, page, limit).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use mockito;

    #[tokio::test]
    async fn test_get_current_user_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/users/me")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user_id":"user123","name":"Test User","nickname":"tester"}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = UsersApi::from_client(&client);
        let result = api.get_current_user().await;

        assert!(result.is_ok());
        let user = result.unwrap();
        assert_eq!(user.user_id, Some("user123".to_string()));
        assert_eq!(user.name, Some("Test User".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_current_user_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/users/me")
            .with_status(401)
            .with_body("Unauthorized")
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = UsersApi::from_client(&client);
        let result = api.get_current_user().await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Http { status, .. } => assert_eq!(status, 401),
            _ => panic!("Expected HTTP error"),
        }
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_search_users_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/users")
            .match_query(mockito::Matcher::UrlEncoded("q".into(), "john".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[{"user_id":"user1","name":"John Doe"}]"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = UsersApi::from_client(&client);
        let result = api.search_users("john").await;

        assert!(result.is_ok());
        let users = result.unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].user_id, Some("user1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_search_users_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/users")
            .match_query(mockito::Matcher::Any)
            .with_status(400)
            .with_body("Bad Request")
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = UsersApi::from_client(&client);
        let result = api.search_users("test").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Http { status, .. } => assert_eq!(status, 400),
            _ => panic!("Expected HTTP error"),
        }
        mock.assert_async().await;
    }
}
