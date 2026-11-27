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
//!     let unread = client.users().get_unread_messages(user_id).await?;
//!     println!("{} unread messages", unread.len());
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
//! const unread = await client.users().getUnreadMessages(me.user_id);
//! const follows = await client.users().getUserFollows(me.user_id, 1, 20);
//! ```

#[cfg(feature = "wasm")]
use super::to_js_value;
use super::{PaginationQuery, ApiClient, ApiError, urlencode};
use crate::models;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

api_module_struct!(
    /// User profile, notification, conversation, referral and channel
    /// endpoints; obtain with [`ApiClient::users`].
    UsersApi
);

/// `PUT /users/{userId}` wraps the metadata in a `user_metadata` key, both in
/// the request and the response.
#[derive(Serialize, Deserialize)]
struct UserMetadataEnvelope<M> {
    user_metadata: M,
}

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

    /// Update a user's editable profile metadata (`PUT /users/{user_id}`),
    /// returning the stored metadata. Only fields that are `Some` are sent.
    ///
    /// # Errors
    ///
    /// [`ApiError::Http`] `409` on a conflicting update.
    pub async fn update_user(
        &self,
        user_id: &str,
        metadata: &models::UserMetadata,
    ) -> Result<models::UserMetadata, ApiError> {
        let path = format!("/users/{}", urlencode(user_id));
        let stored: UserMetadataEnvelope<models::UserMetadata> =
            self.client.put(&path, &UserMetadataEnvelope { user_metadata: metadata }).await?;
        Ok(stored.user_metadata)
    }

    /// Delete a user account (`DELETE /users/{user_id}`).
    ///
    /// Succeeds both when the account is deleted (`204`) and when it is only
    /// marked for deletion pending a subscription cancellation (`202`).
    ///
    /// # Errors
    ///
    /// [`ApiError::Http`] `406` while the user has a studio running, `412`
    /// while an outstanding Stripe balance prevents deletion.
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

    /// Get a page of a user's conversations (`GET /users/{user_id}/conversations`).
    ///
    /// `stream_id` narrows the results to the conversation with one studio.
    /// Pagination behaves as in [`get_user_channels`](Self::get_user_channels).
    pub async fn get_conversations(
        &self,
        user_id: &str,
        stream_id: Option<&str>,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedConversations, ApiError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Query<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            stream_id: Option<&'a str>,
            #[serde(flatten)]
            page: PaginationQuery,
        }

        let path = format!("/users/{}/conversations", urlencode(user_id));
        self.client
            .get_with_query(&path, &Query { stream_id, page: PaginationQuery { page, limit } })
            .await
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
    /// (`GET /users/{user_id}/hubspot-token`), used to identify the visitor to
    /// the HubSpot Conversations widget. Users may only fetch their own token.
    pub async fn get_hubspot_token(&self, user_id: &str) -> Result<models::HubSpotToken, ApiError> {
        let path = format!("/users/{}/hubspot-token", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get the user's unread studio messages
    /// (`GET /users/{user_id}/unread-messages`); its length is the unread count.
    pub async fn get_unread_messages(&self, user_id: &str) -> Result<Vec<models::UnreadMessage>, ApiError> {
        let path = format!("/users/{}/unread-messages", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get how many referrals the user has made (`GET /users/{user_id}/referrals`).
    pub async fn get_referral_count(&self, user_id: &str) -> Result<models::ReferralCount, ApiError> {
        let path = format!("/users/{}/referrals", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Record a referral for the user (`POST /users/{user_id}/referrals`).
    ///
    /// # Errors
    ///
    /// [`ApiError::Http`] `409` if the referral already exists.
    pub async fn create_referral(&self, user_id: &str) -> Result<(), ApiError> {
        let path = format!("/users/{}/referrals", urlencode(user_id));
        self.client.post_empty_no_response(&path).await
    }

    /// Get a page of channels the user is a member of
    /// (`GET /users/{user_id}/channels-paginated`).
    ///
    /// `page` (1-indexed) and `limit` are sent as query parameters only when
    /// set; otherwise the server's defaults apply. The response's
    /// [`models::PaginationMeta`] describes the totals.
    pub async fn get_user_channels(
        &self,
        user_id: &str,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedChannels, ApiError> {
        let path = format!("/users/{}/channels-paginated", urlencode(user_id));
        self.client.get_with_query(&path, &PaginationQuery { page, limit }).await
    }

    /// Get a page of channels the user follows
    /// (`GET /users/{user_id}/follows-paginated`).
    ///
    /// Pagination behaves as in [`get_user_channels`](Self::get_user_channels).
    pub async fn get_user_follows(
        &self,
        user_id: &str,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedChannels, ApiError> {
        let path = format!("/users/{}/follows-paginated", urlencode(user_id));
        self.client.get_with_query(&path, &PaginationQuery { page, limit }).await
    }
}

#[cfg(feature = "wasm")]
#[wasm_bindgen]
impl UsersApi {
    /// See [`UsersApi::get_current_user`].
    #[wasm_bindgen(js_name = getCurrentUser)]
    pub async fn get_current_user_js(&self) -> Result<models::User, ApiError> {
        self.get_current_user().await
    }

    /// See [`UsersApi::get_user`].
    #[wasm_bindgen(js_name = getUser)]
    pub async fn get_user_js(&self, user_id: String) -> Result<models::User, ApiError> {
        self.get_user(&user_id).await
    }

    /// See [`UsersApi::update_user`].
    #[wasm_bindgen(js_name = updateUser)]
    pub async fn update_user_js(
        &self,
        user_id: String,
        metadata: models::UserMetadata,
    ) -> Result<models::UserMetadata, ApiError> {
        self.update_user(&user_id, &metadata).await
    }

    /// See [`UsersApi::delete_user`].
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

    /// Resolves to `Notification[]`; see [`UsersApi::get_notifications`].
    #[wasm_bindgen(js_name = getNotifications)]
    pub async fn get_notifications_js(&self, user_id: String) -> Result<JsValue, ApiError> {
        let notifications = self.get_notifications(&user_id).await?;
        to_js_value(&notifications)
    }

    /// See [`UsersApi::get_conversations`].
    #[wasm_bindgen(js_name = getConversations)]
    pub async fn get_conversations_js(
        &self,
        user_id: String,
        stream_id: Option<String>,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedConversations, ApiError> {
        self.get_conversations(&user_id, stream_id.as_deref(), page, limit).await
    }

    /// See [`UsersApi::get_conversation`].
    #[wasm_bindgen(js_name = getConversation)]
    pub async fn get_conversation_js(
        &self,
        user_id: String,
        stream_id: String,
    ) -> Result<models::Conversation, ApiError> {
        self.get_conversation(&user_id, &stream_id).await
    }

    /// See [`UsersApi::get_hubspot_token`].
    #[wasm_bindgen(js_name = getHubspotToken)]
    pub async fn get_hubspot_token_js(&self, user_id: String) -> Result<models::HubSpotToken, ApiError> {
        self.get_hubspot_token(&user_id).await
    }

    /// Resolves to `UnreadMessage[]`; see [`UsersApi::get_unread_messages`].
    #[wasm_bindgen(js_name = getUnreadMessages)]
    pub async fn get_unread_messages_js(&self, user_id: String) -> Result<JsValue, ApiError> {
        let unread = self.get_unread_messages(&user_id).await?;
        to_js_value(&unread)
    }

    /// See [`UsersApi::get_referral_count`].
    #[wasm_bindgen(js_name = getReferralCount)]
    pub async fn get_referral_count_js(&self, user_id: String) -> Result<models::ReferralCount, ApiError> {
        self.get_referral_count(&user_id).await
    }

    /// See [`UsersApi::create_referral`].
    #[wasm_bindgen(js_name = createReferral)]
    pub async fn create_referral_js(&self, user_id: String) -> Result<(), ApiError> {
        self.create_referral(&user_id).await
    }

    /// See [`UsersApi::get_user_channels`].
    #[wasm_bindgen(js_name = getUserChannels)]
    pub async fn get_user_channels_js(
        &self,
        user_id: String,
        page: Option<i32>,
        limit: Option<i32>,
    ) -> Result<models::PaginatedChannels, ApiError> {
        self.get_user_channels(&user_id, page, limit).await
    }

    /// See [`UsersApi::get_user_follows`].
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
    use mockito::Matcher;

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
    async fn test_get_user_encodes_auth0_id_in_path() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/users/auth0%7Cu1", 200, r#"{"user_id":"auth0|u1"}"#).await;

        let user = api(&client).get_user("auth0|u1").await.unwrap();
        assert_eq!(user.user_id.as_deref(), Some("auth0|u1"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_user_wraps_metadata_in_envelope() {
        // Regression: the metadata was sent and read bare, but the API wraps
        // both in `{"user_metadata": …}`.
        let (mut server, client) = mock_api().await;
        let mock = server
            .mock("PUT", "/users/u1")
            .match_body(Matcher::Json(serde_json::json!({"user_metadata": {"display_name": "Updated"}})))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user_metadata":{"display_name":"Updated","tags":["jazz"]}}"#)
            .create_async()
            .await;

        let metadata = models::UserMetadata { display_name: Some("Updated".to_string()), ..Default::default() };
        let stored = api(&client).update_user("u1", &metadata).await.unwrap();
        assert_eq!(stored.display_name.as_deref(), Some("Updated"));
        assert_eq!(stored.tags, Some(vec!["jazz".to_string()]));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_user_accepts_202_and_reports_412() {
        let (mut server, client) = mock_api().await;
        let pending = mock_empty(&mut server, "DELETE", "/users/u1", 202).await;
        api(&client).delete_user("u1").await.unwrap();
        pending.assert_async().await;

        let balance = mock_json(&mut server, "DELETE", "/users/u2", 412, r#"{"error":"balance"}"#).await;
        assert_http_status(api(&client).delete_user("u2").await.unwrap_err(), 412);
        balance.assert_async().await;
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
        let region = regions.get("gcloud-us-ut-slc").expect("region present");
        assert_eq!(region.label, Some("USA - Salt Lake City, UT".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_conversations_is_paginated_and_filters_by_stream() {
        // Regression: decoded as a bare array, but the API returns a
        // paginated envelope.
        let (mut server, client) = mock_api().await;
        let mock = server
            .mock("GET", "/users/u1/conversations")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("streamId".into(), "s1".into()),
                Matcher::UrlEncoded("page".into(), "2".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"_meta":{"total":1,"pages":1,"current":2,"count":1,"limit":20},"results":[{"streamId":"s1","text":"hi"}]}"#)
            .create_async()
            .await;

        let page = api(&client).get_conversations("u1", Some("s1"), Some(2), None).await.unwrap();
        assert_eq!(page.results[0].text.as_deref(), Some("hi"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_hubspot_token_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/users/u1/hubspot-token", 200, r#"{"token":"hs-visitor-abc"}"#).await;

        let result = api(&client).get_hubspot_token("u1").await.unwrap();
        assert_eq!(result.token, Some("hs-visitor-abc".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_unread_messages_is_a_list() {
        // Regression: decoded as `{count}`, but the API returns the messages.
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/unread-messages",
            200,
            r#"[{"conversationStreamId":"s1","message":"hi"},{"conversationStreamId":"s2","message":"yo"}]"#,
        )
        .await;

        let unread = api(&client).get_unread_messages("u1").await.unwrap();
        assert_eq!(unread.len(), 2);
        assert_eq!(unread[1].message.as_deref(), Some("yo"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_referrals_count_and_create() {
        // Regression: the count was decoded as a list of referrals, and
        // creating one expected a body from a 204 response.
        let (mut server, client) = mock_api().await;
        let count = mock_json(&mut server, "GET", "/users/u1/referrals", 200, r#"{"count":3}"#).await;
        let create = mock_empty(&mut server, "POST", "/users/u1/referrals", 204).await;

        assert_eq!(api(&client).get_referral_count("u1").await.unwrap().count, Some(3));
        api(&client).create_referral("u1").await.unwrap();
        count.assert_async().await;
        create.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_user_channels_sends_only_set_pagination_params() {
        let (mut server, client) = mock_api().await;
        let with_params = server
            .mock("GET", "/users/u1/channels-paginated")
            .match_query(Matcher::AllOf(vec![
                Matcher::UrlEncoded("page".into(), "2".into()),
                Matcher::UrlEncoded("limit".into(), "10".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"_meta":{"total":11,"pages":2,"current":2,"count":1,"limit":10},"results":[{"id":"c1"}]}"#)
            .create_async()
            .await;

        let result = api(&client).get_user_channels("u1", Some(2), Some(10)).await.unwrap();
        assert_eq!(result.meta.current, 2);
        with_params.assert_async().await;

        let bare = server
            .mock("GET", "/users/u1/follows-paginated")
            .match_query(Matcher::Exact(String::new()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"_meta":{"total":0,"pages":0,"current":0,"count":0,"limit":20},"results":[]}"#)
            .create_async()
            .await;
        assert!(api(&client).get_user_follows("u1", None, None).await.unwrap().results.is_empty());
        bare.assert_async().await;
    }
}
