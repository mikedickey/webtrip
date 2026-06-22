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
    pub async fn get_portal(&self, user_id: &str) -> Result<models::UrlResponse, ApiError> {
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
    pub async fn get_portal_js(&self, user_id: String) -> Result<models::UrlResponse, ApiError> {
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
        assert_eq!(plans[0].price, Some(999));
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
            r#"{"url":"https://portal.example.com"}"#,
        )
        .await;

        let resp = api(&client).get_portal("u1").await.unwrap();
        assert_eq!(resp.url, Some("https://portal.example.com".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_subscription_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/user123/subscription",
            200,
            r#"{"id":"sub123","planId":"pro","status":"active"}"#,
        )
        .await;

        let sub = api(&client).get_subscription("user123").await.unwrap();
        assert_eq!(sub.id, Some("sub123".to_string()));
        assert_eq!(sub.plan_id, Some("pro".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_subscription_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/users/nonexistent/subscription", 404, "nope").await;

        let err = api(&client).get_subscription("nonexistent").await.unwrap_err();
        assert_http_status(err, 404);
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

    #[tokio::test]
    async fn test_modify_subscription_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "PUT",
            "/users/u1/subscription",
            200,
            r#"{"id":"sub1","status":"active"}"#,
        )
        .await;

        let req = models::ModifySubscriptionRequest::default();
        let sub = api(&client).modify_subscription("u1", &req).await.unwrap();
        assert_eq!(sub.id, Some("sub1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_cancel_subscription_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "DELETE",
            "/users/u1/subscription",
            200,
            r#"{"id":"sub1","status":"canceled"}"#,
        )
        .await;

        let sub = api(&client).cancel_subscription("u1").await.unwrap();
        assert_eq!(sub.status, Some("canceled".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_reactivate_subscription_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/users/u1/subscription/reactivate",
            200,
            r#"{"id":"sub1","status":"active"}"#,
        )
        .await;

        let sub = api(&client).reactivate_subscription("u1").await.unwrap();
        assert_eq!(sub.status, Some("active".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_redeem_coupon_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/users/u1/subscription/coupon",
            200,
            r#"{"valid":true,"discount":10.0}"#,
        )
        .await;

        let req = models::CouponRequest::default();
        let resp = api(&client).redeem_coupon("u1", &req).await.unwrap();
        assert_eq!(resp.valid, Some(true));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_entitlements_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/billing/entitlements",
            200,
            r#"[{"id":"e1","enabled":true}]"#,
        )
        .await;

        let result = api(&client).get_entitlements().await.unwrap();
        assert_eq!(result[0].id, Some("e1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_apply_promo_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/users/u1/subscription/promo",
            200,
            r#"{"applied":true,"description":"50% off"}"#,
        )
        .await;

        let req = models::PromoRequest::default();
        let resp = api(&client).apply_promo("u1", &req).await.unwrap();
        assert_eq!(resp.applied, Some(true));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_apply_promo_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "POST", "/users/u1/subscription/promo", 422, "invalid").await;

        let req = models::PromoRequest::default();
        let err = api(&client).apply_promo("u1", &req).await.unwrap_err();
        assert_http_status(err, 422);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_invoices_with_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/invoices",
            200,
            r#"{"invoices":[{"id":"inv1"}],"hasMore":true,"cursor":"next"}"#,
        )
        .await;

        let result = api(&client)
            .list_invoices("u1", Some("abc"), Some(10))
            .await
            .unwrap();
        assert_eq!(result.invoices.unwrap().len(), 1);
        assert_eq!(result.has_more, Some(true));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_invoices_no_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/users/u1/invoices",
            200,
            r#"{"invoices":[]}"#,
        )
        .await;

        let result = api(&client).list_invoices("u1", None, None).await.unwrap();
        assert_eq!(result.invoices.unwrap().len(), 0);
        mock.assert_async().await;
    }
}
