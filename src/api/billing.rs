//! Billing API endpoints
//!
//! Subscription and payment management.

use super::{to_js_value, ApiClient, ApiError, urlencode};
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
    /// Get available subscription plans
    pub async fn get_plans(&self) -> Result<Vec<models::Plan>, ApiError> {
        self.client.get("/billing/plans").await
    }

    /// Get the billing portal URL
    pub async fn get_portal(&self, user_id: &str) -> Result<models::BillingPortalResponse, ApiError> {
        let path = format!("/users/{}/billing/portal", urlencode(user_id));
        self.client.get(&path).await
    }

    /// Get subscription information for a user
    pub async fn get_subscription(&self, user_id: &str) -> Result<models::Subscription, ApiError> {
        let path = format!("/users/{}/subscription", urlencode(user_id));
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

    /// Modify an existing subscription
    pub async fn modify_subscription(
        &self,
        user_id: &str,
        modify_request: &models::ModifySubscriptionRequest,
    ) -> Result<models::Subscription, ApiError> {
        let path = format!("/users/{}/subscription", urlencode(user_id));
        self.client.put(&path, modify_request).await
    }

    /// Cancel a subscription
    pub async fn cancel_subscription(&self, user_id: &str) -> Result<models::Subscription, ApiError> {
        let path = format!("/users/{}/subscription", urlencode(user_id));
        self.client.delete_with_response(&path).await
    }

    /// Reactivate a canceled subscription
    pub async fn reactivate_subscription(&self, user_id: &str) -> Result<models::Subscription, ApiError> {
        let path = format!("/users/{}/subscription/reactivate", urlencode(user_id));
        self.client.post_empty(&path).await
    }

    /// Redeem a coupon code
    pub async fn redeem_coupon(
        &self,
        user_id: &str,
        coupon_request: &models::CouponRequest,
    ) -> Result<models::CouponResponse, ApiError> {
        let path = format!("/users/{}/subscription/coupon", urlencode(user_id));
        self.client.post(&path, coupon_request).await
    }

    /// Get available entitlements
    pub async fn get_entitlements(&self) -> Result<Vec<models::Entitlement>, ApiError> {
        self.client.get("/billing/entitlements").await
    }

    /// Apply a promo code
    pub async fn apply_promo(
        &self,
        user_id: &str,
        promo_request: &models::PromoRequest,
    ) -> Result<models::PromoResponse, ApiError> {
        let path = format!("/users/{}/subscription/promo", urlencode(user_id));
        self.client.post(&path, promo_request).await
    }

    /// List all invoices for a user
    pub async fn list_invoices(
        &self,
        user_id: &str,
        cursor: Option<&str>,
        limit: Option<i32>,
    ) -> Result<models::InvoiceListResponse, ApiError> {
        let path = format!("/users/{}/invoices", urlencode(user_id));

        #[derive(Serialize)]
        struct Query<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            cursor: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            limit: Option<i32>,
        }

        if cursor.is_some() || limit.is_some() {
            self.client.get_with_query(&path, &Query { cursor, limit }).await
        } else {
            self.client.get(&path).await
        }
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
    pub async fn get_portal_js(&self, user_id: String) -> Result<models::BillingPortalResponse, ApiError> {
        self.get_portal(&user_id).await
    }

    #[wasm_bindgen(js_name = getSubscription)]
    pub async fn get_subscription_js(&self, user_id: String) -> Result<models::Subscription, ApiError> {
        self.get_subscription(&user_id).await
    }

    #[wasm_bindgen(js_name = createCheckout)]
    pub async fn create_checkout_js(
        &self,
        user_id: String,
        checkout_request: models::CheckoutRequest,
    ) -> Result<models::CheckoutResponse, ApiError> {
        self.create_checkout(&user_id, &checkout_request).await
    }

    #[wasm_bindgen(js_name = modifySubscription)]
    pub async fn modify_subscription_js(
        &self,
        user_id: String,
        modify_request: models::ModifySubscriptionRequest,
    ) -> Result<models::Subscription, ApiError> {
        self.modify_subscription(&user_id, &modify_request).await
    }

    #[wasm_bindgen(js_name = cancelSubscription)]
    pub async fn cancel_subscription_js(&self, user_id: String) -> Result<models::Subscription, ApiError> {
        self.cancel_subscription(&user_id).await
    }

    #[wasm_bindgen(js_name = reactivateSubscription)]
    pub async fn reactivate_subscription_js(&self, user_id: String) -> Result<models::Subscription, ApiError> {
        self.reactivate_subscription(&user_id).await
    }

    #[wasm_bindgen(js_name = redeemCoupon)]
    pub async fn redeem_coupon_js(
        &self,
        user_id: String,
        coupon_request: models::CouponRequest,
    ) -> Result<models::CouponResponse, ApiError> {
        self.redeem_coupon(&user_id, &coupon_request).await
    }

    #[wasm_bindgen(js_name = getEntitlements)]
    pub async fn get_entitlements_js(&self) -> Result<JsValue, ApiError> {
        let entitlements = self.get_entitlements().await?;
        to_js_value(&entitlements)
    }

    #[wasm_bindgen(js_name = applyPromo)]
    pub async fn apply_promo_js(
        &self,
        user_id: String,
        promo_request: models::PromoRequest,
    ) -> Result<models::PromoResponse, ApiError> {
        self.apply_promo(&user_id, &promo_request).await
    }

    #[wasm_bindgen(js_name = listInvoices)]
    pub async fn list_invoices_js(
        &self,
        user_id: String,
        cursor: Option<String>,
        limit: Option<i32>,
    ) -> Result<models::InvoiceListResponse, ApiError> {
        self.list_invoices(&user_id, cursor.as_deref(), limit).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use mockito;

    #[tokio::test]
    async fn test_get_plans_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/billing/plans")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"[{"id":"basic","name":"Basic Plan","price":999}]"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = BillingApi::from_client(&client);
        let result = api.get_plans().await;

        assert!(result.is_ok());
        let plans = result.unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].id, Some("basic".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_plans_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/billing/plans")
            .with_status(500)
            .with_body("Internal Server Error")
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = BillingApi::from_client(&client);
        let result = api.get_plans().await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Http { status, .. } => assert_eq!(status, 500),
            _ => panic!("Expected HTTP error"),
        }
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_subscription_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/users/user123/subscription")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"sub123","planId":"pro","status":"active"}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = BillingApi::from_client(&client);
        let result = api.get_subscription("user123").await;

        assert!(result.is_ok());
        let subscription = result.unwrap();
        assert_eq!(subscription.id, Some("sub123".to_string()));
        assert_eq!(subscription.plan_id, Some("pro".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_subscription_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/users/nonexistent/subscription")
            .with_status(404)
            .with_body("Not Found")
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let api = BillingApi::from_client(&client);
        let result = api.get_subscription("nonexistent").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Http { status, .. } => assert_eq!(status, 404),
            _ => panic!("Expected HTTP error"),
        }
        mock.assert_async().await;
    }
}
