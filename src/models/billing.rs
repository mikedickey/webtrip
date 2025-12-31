//! Billing and subscription models

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// User billing information
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct BillingInfo {
    /// User ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Current subscription plan
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<Plan>,

    /// Subscription status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    /// Stripe customer ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stripe_customer_id: Option<String>,

    /// Current billing period start (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_start: Option<String>,

    /// Current billing period end (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_end: Option<String>,

    /// Whether the subscription will cancel at period end
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_at_period_end: Option<bool>,
}

/// Subscription plan
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// Plan ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Plan name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Plan description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Price in cents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<i32>,

    /// Currency code (e.g., "usd")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,

    /// Billing interval (month, year)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<String>,

    /// Maximum studios allowed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_studios: Option<i32>,

    /// Maximum devices allowed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_devices: Option<i32>,

    /// Maximum musicians per studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_musicians: Option<i32>,

    /// Studio duration limit in hours
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studio_hours: Option<i32>,

    /// Recording storage limit in GB
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_storage_gb: Option<i32>,

    /// Whether broadcasting is enabled
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcast_enabled: Option<bool>,

    /// Available cloud regions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub regions: Option<Vec<String>>,

    /// Plan features list
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<String>>,
}

/// Subscription details
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    /// Subscription ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Plan ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,

    /// Subscription status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    /// Current period start (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_period_start: Option<String>,

    /// Current period end (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_period_end: Option<String>,

    /// Whether subscription will cancel at period end
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_at_period_end: Option<bool>,

    /// Cancellation timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canceled_at: Option<String>,
}

/// Usage information
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Total studio hours used
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studio_hours: Option<f64>,

    /// Number of active studios
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_studios: Option<i32>,

    /// Number of registered devices
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<i32>,

    /// Recording storage used in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording_storage: Option<i64>,

    /// Billing period start (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_start: Option<String>,

    /// Billing period end (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_end: Option<String>,
}

/// Usage response with limits
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct UsageResponse {
    /// Current usage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,

    /// Plan limits
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits: Option<Plan>,
}

