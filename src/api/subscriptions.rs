//! Studio subscriptions (memberships) API endpoints
//!
//! A "subscription" here is **not** a billing plan (see `billing.rs` for that) —
//! it represents a user's membership of a studio. These endpoints span two URL
//! roots (`/users/{userId}/subscriptions` and `/studios/{studioId}/subscriptions*`)
//! but model the single membership concept, so they live together here.

use super::{to_js_value, ApiClient, ApiError, urlencode};
use crate::models;
use wasm_bindgen::prelude::*;

// =============================================================================
// Subscriptions API
// =============================================================================

api_module_struct!(SubscriptionsApi);

// =============================================================================
// Rust API (primary interface)
// =============================================================================

impl SubscriptionsApi {
    /// List the studios a user is a member of.
    pub async fn list_user_subscriptions(
        &self,
        user_id: &str,
    ) -> Result<Vec<models::Subscription>, ApiError> {
        let path = format!("/users/{}/subscriptions", urlencode(user_id));
        self.client.get(&path).await
    }

    /// List a studio's members.
    pub async fn list_studio_subscriptions(
        &self,
        studio_id: &str,
    ) -> Result<Vec<models::Subscription>, ApiError> {
        let path = format!("/studios/{}/subscriptions", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Add a member to a studio (create a membership).
    pub async fn create_subscription(
        &self,
        studio_id: &str,
        request: &models::CreateSubscriptionRequest,
    ) -> Result<models::Subscription, ApiError> {
        let path = format!("/studios/{}/subscriptions", urlencode(studio_id));
        self.client.post(&path, request).await
    }

    /// Describe a single membership between a studio and a user.
    pub async fn get_subscription(
        &self,
        studio_id: &str,
        user_id: &str,
    ) -> Result<models::Subscription, ApiError> {
        let path = format!(
            "/studios/{}/subscriptions/{}",
            urlencode(studio_id),
            urlencode(user_id)
        );
        self.client.get(&path).await
    }

    /// Update a membership (e.g. admin flag / status). The updated membership is
    /// sent as the request body, mirroring the studio/device update endpoints.
    pub async fn update_subscription(
        &self,
        studio_id: &str,
        user_id: &str,
        subscription: &models::Subscription,
    ) -> Result<models::Subscription, ApiError> {
        let path = format!(
            "/studios/{}/subscriptions/{}",
            urlencode(studio_id),
            urlencode(user_id)
        );
        self.client.put(&path, subscription).await
    }

    /// Remove a member from a studio.
    pub async fn delete_subscription(
        &self,
        studio_id: &str,
        user_id: &str,
    ) -> Result<(), ApiError> {
        let path = format!(
            "/studios/{}/subscriptions/{}",
            urlencode(studio_id),
            urlencode(user_id)
        );
        self.client.delete(&path).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[wasm_bindgen]
impl SubscriptionsApi {
    #[wasm_bindgen(js_name = listUserSubscriptions)]
    pub async fn list_user_subscriptions_js(&self, user_id: String) -> Result<JsValue, ApiError> {
        let subscriptions = self.list_user_subscriptions(&user_id).await?;
        to_js_value(&subscriptions)
    }

    #[wasm_bindgen(js_name = listStudioSubscriptions)]
    pub async fn list_studio_subscriptions_js(
        &self,
        studio_id: String,
    ) -> Result<JsValue, ApiError> {
        let subscriptions = self.list_studio_subscriptions(&studio_id).await?;
        to_js_value(&subscriptions)
    }

    #[wasm_bindgen(js_name = createSubscription)]
    pub async fn create_subscription_js(
        &self,
        studio_id: String,
        request: models::CreateSubscriptionRequest,
    ) -> Result<models::Subscription, ApiError> {
        self.create_subscription(&studio_id, &request).await
    }

    #[wasm_bindgen(js_name = getSubscription)]
    pub async fn get_subscription_js(
        &self,
        studio_id: String,
        user_id: String,
    ) -> Result<models::Subscription, ApiError> {
        self.get_subscription(&studio_id, &user_id).await
    }

    #[wasm_bindgen(js_name = updateSubscription)]
    pub async fn update_subscription_js(
        &self,
        studio_id: String,
        user_id: String,
        subscription: models::Subscription,
    ) -> Result<models::Subscription, ApiError> {
        self.update_subscription(&studio_id, &user_id, &subscription).await
    }

    #[wasm_bindgen(js_name = deleteSubscription)]
    pub async fn delete_subscription_js(
        &self,
        studio_id: String,
        user_id: String,
    ) -> Result<(), ApiError> {
        self.delete_subscription(&studio_id, &user_id).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::api::test_helpers::{assert_http_status, mock_api, mock_empty, mock_json};

    fn api(client: &ApiClient) -> SubscriptionsApi {
        SubscriptionsApi::from_client(client)
    }

    const SUBSCRIPTION_JSON: &str = r#"{
        "user_id": "auth0|abc",
        "name": "Ada Lovelace",
        "nickname": "ada",
        "picture": "https://example.com/ada.png",
        "email": "ada@example.com",
        "updated_at": "2026-06-14T00:00:00Z",
        "serverId": "studio-1",
        "admin": true,
        "status": "Active",
        "createdAt": "2026-06-01T00:00:00Z",
        "updatedAt": "2026-06-10T00:00:00Z"
    }"#;

    #[tokio::test]
    async fn test_list_user_subscriptions_success() {
        let (mut server, client) = mock_api().await;
        let body = format!("[{SUBSCRIPTION_JSON}]");
        let mock = mock_json(&mut server, "GET", "/users/u1/subscriptions", 200, &body).await;

        let result = api(&client).list_user_subscriptions("u1").await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].server_id.as_deref(), Some("studio-1"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_user_subscriptions_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/users/u1/subscriptions", 401, "Unauthorized").await;

        let err = api(&client).list_user_subscriptions("u1").await.unwrap_err();
        assert_http_status(err, 401);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_studio_subscriptions_success() {
        let (mut server, client) = mock_api().await;
        let body = format!("[{SUBSCRIPTION_JSON}]");
        let mock = mock_json(&mut server, "GET", "/studios/st1/subscriptions", 200, &body).await;

        let result = api(&client).list_studio_subscriptions("st1").await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].user_id.as_deref(), Some("auth0|abc"));
        assert_eq!(result[0].admin, Some(true));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_studio_subscriptions_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/studios/st1/subscriptions", 500, "boom").await;

        let err = api(&client).list_studio_subscriptions("st1").await.unwrap_err();
        assert_http_status(err, 500);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_subscription_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/studios/st1/subscriptions",
            201,
            SUBSCRIPTION_JSON,
        )
        .await;

        let request = models::CreateSubscriptionRequest {
            user_id: Some("auth0|abc".into()),
            ..Default::default()
        };
        let result = api(&client).create_subscription("st1", &request).await.unwrap();
        assert_eq!(result.user_id.as_deref(), Some("auth0|abc"));
        assert_eq!(result.status.as_deref(), Some("Active"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_subscription_conflict_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "POST", "/studios/st1/subscriptions", 409, "already subscribed").await;

        let request = models::CreateSubscriptionRequest::default();
        let err = api(&client).create_subscription("st1", &request).await.unwrap_err();
        assert_http_status(err, 409);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_subscription_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/subscriptions/u1",
            200,
            SUBSCRIPTION_JSON,
        )
        .await;

        let result = api(&client).get_subscription("st1", "u1").await.unwrap();
        assert_eq!(result.user_id.as_deref(), Some("auth0|abc"));
        assert_eq!(result.server_id.as_deref(), Some("studio-1"));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_subscription_not_found() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/studios/st1/subscriptions/u1", 404, "Not Found").await;

        let err = api(&client).get_subscription("st1", "u1").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_subscription_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "PUT",
            "/studios/st1/subscriptions/u1",
            200,
            SUBSCRIPTION_JSON,
        )
        .await;

        let body = models::Subscription {
            admin: Some(true),
            ..Default::default()
        };
        let result = api(&client).update_subscription("st1", "u1", &body).await.unwrap();
        assert_eq!(result.admin, Some(true));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_subscription_forbidden() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "PUT", "/studios/st1/subscriptions/u1", 403, "Forbidden").await;

        let body = models::Subscription::default();
        let err = api(&client).update_subscription("st1", "u1", &body).await.unwrap_err();
        assert_http_status(err, 403);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_subscription_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/studios/st1/subscriptions/u1", 204).await;

        api(&client).delete_subscription("st1", "u1").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_subscription_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/studios/st1/subscriptions/u1", 404).await;

        let err = api(&client).delete_subscription("st1", "u1").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }
}
