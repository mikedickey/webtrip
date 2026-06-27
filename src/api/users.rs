//! Users API endpoints
//!
//! User profile management, preferences, and related operations.

use super::{to_js_value, PaginationQuery, ApiClient, ApiError, urlencode};
use crate::models;
use serde::Serialize;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

// =============================================================================
// Users API
// =============================================================================

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

    /// Get all regions available to a user, keyed by region identifier.
    pub async fn get_user_regions(&self, user_id: &str) -> Result<HashMap<String, models::Region>, ApiError> {
        let path = format!("/users/{}/regions", urlencode(user_id));
        self.client.get(&path).await
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
        
        if page.is_some() || limit.is_some() {
            self.client.get_with_query(&path, &PaginationQuery { page, limit }).await
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
        
        if page.is_some() || limit.is_some() {
            self.client.get_with_query(&path, &PaginationQuery { page, limit }).await
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
        to_js_value(&notifications)
    }

    #[wasm_bindgen(js_name = getConversations)]
    pub async fn get_conversations_js(&self, user_id: String) -> Result<JsValue, ApiError> {
        let conversations = self.get_conversations(&user_id).await?;
        to_js_value(&conversations)
    }

    #[wasm_bindgen(js_name = getUnreadMessagesCount)]
    pub async fn get_unread_messages_count_js(&self, user_id: String) -> Result<models::UnreadMessagesResponse, ApiError> {
        self.get_unread_messages_count(&user_id).await
    }

    #[wasm_bindgen(js_name = getReferrals)]
    pub async fn get_referrals_js(&self, user_id: String) -> Result<JsValue, ApiError> {
        let referrals = self.get_referrals(&user_id).await?;
        to_js_value(&referrals)
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
    use crate::api::test_helpers::{assert_http_status, mock_api, mock_empty, mock_json};

    fn api(client: &ApiClient) -> UsersApi {
        UsersApi::from_client(client)
    }

    #[tokio::test]
    async fn test_get_current_user_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/me",
            200,
            r#"{"user_id":"user123","name":"Test User","nickname":"tester"}"#,
        )
        .await;

        let user = api(&client).get_current_user().await.unwrap();
        assert_eq!(user.user_id, Some("user123".to_string()));
        assert_eq!(user.name, Some("Test User".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_current_user_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/users/me", 401, "Unauthorized").await;

        let err = api(&client).get_current_user().await.unwrap_err();
        assert_http_status(err, 401);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_search_users_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users",
            200,
            r#"[{"user_id":"user1","name":"John Doe"}]"#,
        )
        .await;

        let users = api(&client).search_users("john").await.unwrap();
        assert_eq!(users.len(), 1);
        assert_eq!(users[0].user_id, Some("user1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_search_users_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/users", 400, "Bad Request").await;

        let err = api(&client).search_users("test").await.unwrap_err();
        assert_http_status(err, 400);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_user_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1",
            200,
            r#"{"user_id":"u1","name":"Alice"}"#,
        )
        .await;

        let user = api(&client).get_user("u1").await.unwrap();
        assert_eq!(user.user_id, Some("u1".to_string()));
        assert_eq!(user.name, Some("Alice".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_user_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/users/missing", 404, "nope").await;

        let err = api(&client).get_user("missing").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_user_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "PUT",
            "/users/u1",
            200,
            r#"{"name":"Updated","bio":"hello"}"#,
        )
        .await;

        let metadata = models::UserMetadata {
            name: Some("Updated".to_string()),
            ..Default::default()
        };
        let result = api(&client).update_user("u1", &metadata).await.unwrap();
        assert_eq!(result.name, Some("Updated".to_string()));
        assert_eq!(result.bio, Some("hello".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_user_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/users/u1", 204).await;

        api(&client).delete_user("u1").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_user_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/users/u1", 403).await;

        let err = api(&client).delete_user("u1").await.unwrap_err();
        assert_http_status(err, 403);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_user_regions_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/regions",
            200,
            r#"{"gcloud-us-ut-slc":{"label":"USA - Salt Lake City, UT"}}"#,
        )
        .await;

        let regions = api(&client).get_user_regions("u1").await.unwrap();
        assert_eq!(regions.len(), 1);
        let region = regions.get("gcloud-us-ut-slc").expect("region present");
        assert_eq!(region.label, Some("USA - Salt Lake City, UT".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_notifications_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/notifications",
            200,
            r#"[{"id":"n1","title":"Hi"}]"#,
        )
        .await;

        let result = api(&client).get_notifications("u1").await.unwrap();
        assert_eq!(result[0].id, Some("n1".to_string()));
        assert_eq!(result[0].title, Some("Hi".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_conversations_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/conversations",
            200,
            r#"[{"id":"conv1","userId":"u2"}]"#,
        )
        .await;

        let result = api(&client).get_conversations("u1").await.unwrap();
        assert_eq!(result[0].id, Some("conv1".to_string()));
        assert_eq!(result[0].user_id, Some("u2".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_unread_messages_count_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/unread-messages",
            200,
            r#"{"count":7}"#,
        )
        .await;

        let result = api(&client).get_unread_messages_count("u1").await.unwrap();
        assert_eq!(result.count, Some(7));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_referrals_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/referrals",
            200,
            r#"[{"id":"r1","code":"ABC"}]"#,
        )
        .await;

        let result = api(&client).get_referrals("u1").await.unwrap();
        assert_eq!(result[0].id, Some("r1".to_string()));
        assert_eq!(result[0].code, Some("ABC".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_referral_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/users/u1/referrals",
            200,
            r#"{"id":"r2","code":"NEW"}"#,
        )
        .await;

        let referral = api(&client).create_referral("u1").await.unwrap();
        assert_eq!(referral.id, Some("r2".to_string()));
        assert_eq!(referral.code, Some("NEW".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_user_channels_with_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/channels-paginated",
            200,
            r#"{"_meta":{"total":1,"pages":1,"current":2,"count":1,"limit":10},"results":[{"id":"c1"}]}"#,
        )
        .await;

        let result = api(&client)
            .get_user_channels("u1", Some(2), Some(10))
            .await
            .unwrap();
        assert_eq!(result.results.len(), 1);
        assert_eq!(result.meta.current, 2);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_user_channels_no_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/channels-paginated",
            200,
            r#"{"_meta":{"total":0,"pages":0,"current":1,"count":0,"limit":10},"results":[]}"#,
        )
        .await;

        let result = api(&client).get_user_channels("u1", None, None).await.unwrap();
        assert_eq!(result.results.len(), 0);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_user_follows_with_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/follows-paginated",
            200,
            r#"{"_meta":{"total":1,"pages":1,"current":1,"count":1,"limit":10},"results":[{"id":"c1"}]}"#,
        )
        .await;

        let result = api(&client)
            .get_user_follows("u1", Some(1), None)
            .await
            .unwrap();
        assert_eq!(result.results.len(), 1);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_user_follows_no_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/follows-paginated",
            200,
            r#"{"_meta":{"total":0,"pages":0,"current":1,"count":0,"limit":10},"results":[]}"#,
        )
        .await;

        let result = api(&client).get_user_follows("u1", None, None).await.unwrap();
        assert_eq!(result.results.len(), 0);
        mock.assert_async().await;
    }
}
