//! Billing API endpoints
//!
//! Subscription and payment management.

use super::{ApiClient, ApiError, urlencode};
use crate::models;
use serde::Serialize;
use wasm_bindgen::prelude::*;

// =============================================================================
// Billing API
// =============================================================================

/// Billing API for subscription management
#[wasm_bindgen]
pub struct BillingApi {
    client: ApiClient,
}

impl BillingApi {
    pub(crate) fn from_client(client: &ApiClient) -> Self {
        Self {
            client: client.clone(),
        }
    }
}

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
    #[wasm_bindgen(constructor)]
    pub fn new(client: &ApiClient) -> Self {
        Self::from_client(client)
    }

    #[wasm_bindgen(js_name = getPlans)]
    pub async fn get_plans_js(&self) -> Result<JsValue, ApiError> {
        let plans = self.get_plans().await?;
        serde_wasm_bindgen::to_value(&plans).map_err(|e| ApiError::Serialization(e.to_string()))
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
        serde_wasm_bindgen::to_value(&entitlements).map_err(|e| ApiError::Serialization(e.to_string()))
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
