//! Billing API endpoints
//!
//! Stripe-backed billing. Plan changes (upgrade/downgrade/cancel/reactivate)
//! happen via the Stripe-hosted billing portal, so this module only exposes the
//! plan-pricing lookup plus the portal/checkout redirect surfaces.

use super::{ApiClient, ApiError, urlencode};
use crate::models;
use serde::Serialize;
use wasm_bindgen::prelude::*;

// =============================================================================
// Billing API
// =============================================================================

api_module_struct!(BillingApi);

// =============================================================================
// Rust API (primary interface)
// =============================================================================

impl BillingApi {
    /// Resolve the Stripe price for a plan / pricing mode (`GET /users/{userId}/plans`).
    ///
    /// `plan` is required by the spec; `pricing_mode` and `force_stripe_test_mode`
    /// are optional.
    pub async fn get_plans(
        &self,
        user_id: &str,
        plan: &str,
        pricing_mode: Option<&str>,
        force_stripe_test_mode: Option<&str>,
    ) -> Result<models::PlanPrice, ApiError> {
        let path = format!("/users/{}/plans", urlencode(user_id));

        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Query<'a> {
            plan: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            pricing_mode: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            force_stripe_test_mode: Option<&'a str>,
        }

        self.client
            .get_with_query(&path, &Query { plan, pricing_mode, force_stripe_test_mode })
            .await
    }

    /// Create a Stripe billing-portal session and return its redirect URL
    /// (`POST /users/{userId}/billing`).
    pub async fn get_portal(
        &self,
        user_id: &str,
        request: &models::BillingPortalRequest,
    ) -> Result<models::Redirect, ApiError> {
        let path = format!("/users/{}/billing", urlencode(user_id));
        self.client.post(&path, request).await
    }

    /// Create a Stripe checkout session and return its redirect URL
    /// (`POST /users/{userId}/checkout`).
    pub async fn create_checkout(
        &self,
        user_id: &str,
        checkout_request: &models::CheckoutRequest,
    ) -> Result<models::Redirect, ApiError> {
        let path = format!("/users/{}/checkout", urlencode(user_id));
        self.client.post(&path, checkout_request).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[wasm_bindgen]
impl BillingApi {
    #[wasm_bindgen(js_name = getPlans)]
    pub async fn get_plans_js(
        &self,
        user_id: String,
        plan: String,
        pricing_mode: Option<String>,
        force_stripe_test_mode: Option<String>,
    ) -> Result<models::PlanPrice, ApiError> {
        self.get_plans(&user_id, &plan, pricing_mode.as_deref(), force_stripe_test_mode.as_deref())
            .await
    }

    #[wasm_bindgen(js_name = getPortal)]
    pub async fn get_portal_js(
        &self,
        user_id: String,
        request: models::BillingPortalRequest,
    ) -> Result<models::Redirect, ApiError> {
        self.get_portal(&user_id, &request).await
    }

    #[wasm_bindgen(js_name = createCheckout)]
    pub async fn create_checkout_js(
        &self,
        user_id: String,
        checkout_request: models::CheckoutRequest,
    ) -> Result<models::Redirect, ApiError> {
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
        let mock = server
            .mock("GET", "/users/u1/plans")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("plan".into(), "pro".into()),
                mockito::Matcher::UrlEncoded("pricingMode".into(), "yearly".into()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"plan":"pro","priceID":"price_abc"}"#)
            .create_async()
            .await;

        let resolved = api(&client).get_plans("u1", "pro", Some("yearly"), None).await.unwrap();
        assert_eq!(resolved.plan, Some("pro".to_string()));
        assert_eq!(resolved.price_id, Some("price_abc".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_plans_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/users/u1/plans", 500, "boom").await;

        let err = api(&client).get_plans("u1", "pro", None, None).await.unwrap_err();
        assert_http_status(err, 500);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_portal_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/users/u1/billing",
            200,
            r#"{"redirect":"https://portal.example.com"}"#,
        )
        .await;

        let req = models::BillingPortalRequest {
            callback_url: Some("https://app/done".to_string()),
        };
        let resp = api(&client).get_portal("u1", &req).await.unwrap();
        assert_eq!(resp.redirect, Some("https://portal.example.com".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_checkout_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/users/u1/checkout",
            200,
            r#"{"redirect":"https://checkout.example.com"}"#,
        )
        .await;

        let req = models::CheckoutRequest {
            plan: "pro".to_string(),
            callback_url: "https://app/done".to_string(),
            ..Default::default()
        };
        let resp = api(&client).create_checkout("u1", &req).await.unwrap();
        assert_eq!(resp.redirect, Some("https://checkout.example.com".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_checkout_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "POST", "/users/u1/checkout", 400, "bad").await;

        let req = models::CheckoutRequest::default();
        let err = api(&client).create_checkout("u1", &req).await.unwrap_err();
        assert_http_status(err, 400);
        mock.assert_async().await;
    }
}
