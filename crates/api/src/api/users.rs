//! Users API endpoints.
//!
//! User profiles, notifications, conversations, referrals, and channel
//! membership. Obtain a [`UsersApi`] with [`ApiClient::users`].
//!
//! **Authentication:** every endpoint requires a bearer token
//! ([`ApiClient::set_bearer_token`]). Most take a `user_id`; call
//! [`UsersApi::get_current_user`] first to learn the caller's own ID
//! ([`models::User::user_id`], which is `user_id` in JavaScript too).
//!
//! # Example
//!
//! ```rust,no_run
//! use webtrip_api::api::{ApiClient, ApiError};
//!
//! # async fn run() -> Result<(), ApiError> {
//! let client = ApiClient::new();
//! client.set_bearer_token("your-jwt-token".to_string());
//!
//! let me = client.users().get_current_user().await?;
//! if let Some(user_id) = me.user_id.as_deref() {
//!     let unread = client.users().get_unread_messages_count(user_id).await?;
//!     println!("unread: {:?}", unread.count);
//!
//!     // First page of 20 channels the user follows.
//!     let follows = client.users().get_user_follows(user_id, Some(1), Some(20)).await?;
//!     println!("{} of {}", follows.results.len(), follows.meta.total);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! From JavaScript (the `webtrip` package):
//!
//! ```js
//! import init, { ApiClient } from 'webtrip';
//!
//! await init();
//! const client = new ApiClient();
//! client.setBearerToken('your-jwt-token');
//!
//! const me = await client.users().getCurrentUser();
//! const { count } = await client.users().getUnreadMessagesCount(me.user_id);
//! const follows = await client.users().getUserFollows(me.user_id, 1, 20);
//! ```

#[cfg(feature = "wasm")]
use super::to_js_value;
use super::{PaginationQuery, ApiClient, ApiError, urlencode};
use crate::models;
use std::collections::HashMap;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

// =============================================================================
// Users API
// =============================================================================

api_module_struct!(UsersApi);

// =============================================================================
// Rust API (primary interface)
// =============================================================================

impl UsersApi {
    /// Get the currently authenticated user (`GET /users/me`).
    pub async fn get_current_user(&self) -> Result<models::User, ApiError> {
        self.client.get("/users/me").await
    }

    /// Get a user by ID (`GET /users/{user_id}`).
    pub async fn get_user(&self, user_id: &str) -> Result<models::User, ApiError> {
        let path = format!("/users/{}", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Update a user's profile metadata (`PUT /users/{user_id}`), returning
    /// the stored metadata.
    pub async fn update_user(
        &self,
        user_id: &str,
        metadata: &models::UserMetadata,
    ) -> Result<models::UserMetadata, ApiError> {
        let path = format!("/users/{}", urlencode(user_id));
        self.client.put(&path, metadata).await
    }

    /// Delete a user account (`DELETE /users/{user_id}`).
    pub async fn delete_user(&self, user_id: &str) -> Result<(), ApiError> {
        let path = format!("/users/{}", urlencode(user_id));
        self.client.delete(&path).await
    }

    /// Get all regions available to a user (`GET /users/{user_id}/regions`),
    /// keyed by region identifier like [`SystemApi::list_regions`](super::system::SystemApi::list_regions).
    pub async fn get_user_regions(&self, user_id: &str) -> Result<HashMap<String, models::Region>, ApiError> {
        let path = format!("/users/{}/regions", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get a user's notifications (`GET /users/{user_id}/notifications`).
    pub async fn get_notifications(&self, user_id: &str) -> Result<Vec<models::Notification>, ApiError> {
        let path = format!("/users/{}/notifications", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get a user's conversations (`GET /users/{user_id}/conversations`).
    pub async fn get_conversations(&self, user_id: &str) -> Result<Vec<models::Conversation>, ApiError> {
        let path = format!("/users/{}/conversations", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get the conversation between the user and a studio, identified by the
    /// studio's stream/channel ID (`GET /users/{user_id}/conversations/{stream_id}`).
    pub async fn get_conversation(
        &self,
        user_id: &str,
        stream_id: &str,
    ) -> Result<models::Conversation, ApiError> {
        let path = format!("/users/{}/conversations/{}", urlencode(user_id), urlencode(stream_id));
        self.client.get(&path).await
    }

    /// Get a HubSpot visitor identification token for the user
    /// (`GET /users/{user_id}/hubspot-token`).
    ///
    /// Used to identify the visitor to the HubSpot Conversations API on the
    /// frontend. Users may only fetch their own token.
    pub async fn get_hubspot_token(&self, user_id: &str) -> Result<models::HubSpotToken, ApiError> {
        let path = format!("/users/{}/hubspot-token", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get the count of unread messages for a user
    /// (`GET /users/{user_id}/unread-messages`).
    pub async fn get_unread_messages_count(&self, user_id: &str) -> Result<models::UnreadMessagesResponse, ApiError> {
        let path = format!("/users/{}/unread-messages", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get a user's referrals (`GET /users/{user_id}/referrals`).
    pub async fn get_referrals(&self, user_id: &str) -> Result<Vec<models::Referral>, ApiError> {
        let path = format!("/users/{}/referrals", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Create a new referral code for the user (`POST /users/{user_id}/referrals`).
    pub async fn create_referral(&self, user_id: &str) -> Result<models::Referral, ApiError> {
        let path = format!("/users/{}/referrals", urlencode(user_id));
        self.client.post_empty(&path).await
    }

    /// Get a page of channels the user is a member of
    /// (`GET /users/{user_id}/channels-paginated`).
    ///
    /// `page` and `limit` are sent as query parameters only when set; with
    /// both `None` the server's defaults apply. The response's
    /// [`models::PaginationMeta`] describes the totals.
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

    /// Get a page of channels the user follows
    /// (`GET /users/{user_id}/follows-paginated`).
    ///
    /// Pagination behaves as in [`Self::get_user_channels`].
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

#[cfg(feature = "wasm")]
#[wasm_bindgen]
impl UsersApi {
    #[wasm_bindgen(js_name = getCurrentUser)]
    pub async fn get_current_user_js(&self) -> Result<models::User, ApiError> {
        self.get_current_user().await
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

    /// Resolves to a plain object mapping region identifier → `Region`; see
    /// [`UsersApi::get_user_regions`].
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

    #[wasm_bindgen(js_name = getConversation)]
    pub async fn get_conversation_js(
        &self,
        user_id: String,
        stream_id: String,
    ) -> Result<models::Conversation, ApiError> {
        self.get_conversation(&user_id, &stream_id).await
    }

    #[wasm_bindgen(js_name = getHubspotToken)]
    pub async fn get_hubspot_token_js(&self, user_id: String) -> Result<models::HubSpotToken, ApiError> {
        self.get_hubspot_token(&user_id).await
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
    async fn test_get_conversation_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/conversations/s1",
            200,
            r#"{"id":"conv1","userId":"u2"}"#,
        )
        .await;

        let conv = api(&client).get_conversation("u1", "s1").await.unwrap();
        assert_eq!(conv.id, Some("conv1".to_string()));
        assert_eq!(conv.user_id, Some("u2".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_conversation_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/users/u1/conversations/s1", 404, "nope").await;

        let err = api(&client).get_conversation("u1", "s1").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_hubspot_token_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/hubspot-token",
            200,
            r#"{"token":"hs-visitor-abc"}"#,
        )
        .await;

        let result = api(&client).get_hubspot_token("u1").await.unwrap();
        assert_eq!(result.token, Some("hs-visitor-abc".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_hubspot_token_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/users/u1/hubspot-token", 403, "Forbidden").await;

        let err = api(&client).get_hubspot_token("u1").await.unwrap_err();
        assert_http_status(err, 403);
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
            r#"{"_meta":{"total":11,"pages":2,"current":2,"count":1,"limit":10},"results":[{"id":"c1"}]}"#,
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
