//! Billing API endpoints
//!
//! Stripe-backed billing. Plan changes (upgrade/downgrade/cancel/reactivate)
//! happen via the Stripe-hosted billing portal, so this module only exposes the
//! read/redirect surfaces.
//!
//! NOTE (WEB-28, Task B): the surviving endpoints below still point at the
//! pre-reconciliation paths (`/billing/plans`, `/users/{id}/billing/portal`,
//! `/users/{id}/subscription/checkout`). Task B reconciles them with the new
//! spec paths (`/users/{id}/plans`, `/users/{id}/billing`, `/users/{id}/checkout`)
//! and adds `/redemptions` + `/usage`.

use super::{to_js_value, ApiClient, ApiError, urlencode};
use crate::models;
use wasm_bindgen::prelude::*;

// =============================================================================
// Billing API
// =============================================================================

api_module_struct!(BillingApi);

// =============================================================================
// Rust API (primary interface)
// =============================================================================

impl BillingApi {
    /// Get available subscription plans
    pub async fn get_plans(&self) -> Result<Vec<models::Plan>, ApiError> {
        self.client.get("/billing/plans").await
    }

    /// Get the billing portal URL
    pub async fn get_portal(&self, user_id: &str) -> Result<models::Redirect, ApiError> {
        let path = format!("/users/{}/billing/portal", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Create a checkout session
    pub async fn create_checkout(
        &self,
        user_id: &str,
        checkout_request: &models::CheckoutRequest,
    ) -> Result<models::CheckoutResponse, ApiError> {
        let path = format!("/users/{}/subscription/checkout", urlencode(user_id));
        self.client.post(&path, checkout_request).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[wasm_bindgen]
impl BillingApi {
    #[wasm_bindgen(js_name = getPlans)]
    pub async fn get_plans_js(&self) -> Result<JsValue, ApiError> {
        let plans = self.get_plans().await?;
        to_js_value(&plans)
    }

    #[wasm_bindgen(js_name = getPortal)]
    pub async fn get_portal_js(&self, user_id: String) -> Result<models::Redirect, ApiError> {
        self.get_portal(&user_id).await
    }

    #[wasm_bindgen(js_name = createCheckout)]
    pub async fn create_checkout_js(
        &self,
        user_id: String,
        checkout_request: models::CheckoutRequest,
    ) -> Result<models::CheckoutResponse, ApiError> {
        self.create_checkout(&user_id, &checkout_request).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::api::test_helpers::{assert_http_status, mock_api, mock_json};

    fn api(client: &ApiClient) -> BillingApi {
        BillingApi::from_client(client)
    }

    #[tokio::test]
    async fn test_get_plans_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/billing/plans",
            200,
            r#"[{"id":"basic","name":"Basic Plan","price":999}]"#,
        )
        .await;

        let plans = api(&client).get_plans().await.unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].id, Some("basic".to_string()));
        assert_eq!(plans[0].price, Some(999.0));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_plans_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/billing/plans", 500, "boom").await;

        let err = api(&client).get_plans().await.unwrap_err();
        assert_http_status(err, 500);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_portal_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/billing/portal",
            200,
            r#"{"redirect":"https://portal.example.com"}"#,
        )
        .await;

        let resp = api(&client).get_portal("u1").await.unwrap();
        assert_eq!(resp.redirect, Some("https://portal.example.com".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_checkout_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/users/u1/subscription/checkout",
            200,
            r#"{"url":"https://checkout.example.com","sessionId":"cs_1"}"#,
        )
        .await;

        let req = models::CheckoutRequest::default();
        let resp = api(&client).create_checkout("u1", &req).await.unwrap();
        assert_eq!(resp.session_id, Some("cs_1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_checkout_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "POST", "/users/u1/subscription/checkout", 400, "bad").await;

        let req = models::CheckoutRequest::default();
        let err = api(&client).create_checkout("u1", &req).await.unwrap_err();
        assert_http_status(err, 400);
        mock.assert_async().await;
    }
}
