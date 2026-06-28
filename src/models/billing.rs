//! Billing models
//!
//! Billing is Stripe-backed: plan changes happen via the hosted billing portal,
//! so these models only describe what the client needs to read.

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// User billing information
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct BillingInfo {
    /// Stripe customer ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_id: Option<String>,

    /// Current subscription plan
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,

    /// Subscription status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    /// Current billing period end date
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period_end: Option<String>,
}

/// Subscription plan
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// Plan identifier
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Plan name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Monthly price in cents
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,

    /// Maximum number of musicians allowed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_musicians: Option<i32>,

    /// Included studio minutes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studio_minutes: Option<f64>,
}

/// Resolved plan pricing (`GET /users/{userId}/plans`).
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct PlanPrice {
    /// Resolved plan name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,

    /// Stripe price ID for the requested plan and pricing mode
    #[serde(rename = "priceID", skip_serializing_if = "Option::is_none")]
    pub price_id: Option<String>,
}

/// Coupon redemption record
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Redemption {
    /// Coupon code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// User ID of the redeemer
    #[serde(rename = "ownerID", skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,

    /// Monthly minutes granted
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monthly_minutes: Option<i32>,

    /// Timestamp when the redemption expires (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,

    /// Timestamp when the coupon was redeemed (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redeemed_at: Option<String>,
}

/// Studio utilization statistics
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Earliest RFC3339-formatted timestamp of results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub earliest: Option<String>,

    /// Latest RFC3339-formatted timestamp of results
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,

    /// Aggregated result of data within the time span
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
}

/// Usage API response model
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct UsageResponse {
    /// Aggregated usage summary
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Usage>,

    /// List of usage data points per day
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Vec<Usage>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_utils::roundtrip;

    #[test]
    fn billing_info_fixture_known_good() {
        let json = r#"{
          "customerId": "cus_abc",
          "plan": "pro",
          "status": "active",
          "periodEnd": "2026-07-01T00:00:00Z"
        }"#;
        let b: BillingInfo = serde_json::from_str(json).unwrap();
        assert_eq!(b.customer_id.as_deref(), Some("cus_abc"));
        assert_eq!(b.plan.as_deref(), Some("pro"));
        assert_eq!(b.status.as_deref(), Some("active"));

        let out = roundtrip(&b);
        assert!(out.contains("\"customerId\":"));
        assert!(out.contains("\"periodEnd\":"));
    }

    #[test]
    fn plan_fixture_known_good() {
        let json = r#"{
          "id": "pro",
          "name": "Pro",
          "price": 1999,
          "maxMusicians": 10,
          "studioMinutes": 6000
        }"#;
        let p: Plan = serde_json::from_str(json).unwrap();
        assert_eq!(p.id.as_deref(), Some("pro"));
        assert_eq!(p.price, Some(1999.0));
        assert_eq!(p.max_musicians, Some(10));
        assert_eq!(p.studio_minutes, Some(6000.0));

        let out = roundtrip(&p);
        assert!(out.contains("\"maxMusicians\":"));
        assert!(out.contains("\"studioMinutes\":"));
    }

    #[test]
    fn plan_price_renames_price_id() {
        let json = r#"{"plan":"pro","priceID":"price_abc"}"#;
        let p: PlanPrice = serde_json::from_str(json).unwrap();
        assert_eq!(p.plan.as_deref(), Some("pro"));
        assert_eq!(p.price_id.as_deref(), Some("price_abc"));

        let out = roundtrip(&p);
        // priceID keeps its uppercase casing on the wire.
        assert!(out.contains("\"priceID\":"));
        assert!(!out.contains("\"priceId\":"));
    }

    #[test]
    fn redemption_renames_owner_id() {
        let json = r#"{
          "code": "FREEMONTH",
          "ownerID": "user-1",
          "monthlyMinutes": 600,
          "expiresAt": "2026-12-01T00:00:00Z",
          "redeemedAt": "2026-06-01T00:00:00Z"
        }"#;
        let r: Redemption = serde_json::from_str(json).unwrap();
        assert_eq!(r.owner_id.as_deref(), Some("user-1"));
        assert_eq!(r.monthly_minutes, Some(600));

        let out = roundtrip(&r);
        // ownerID keeps its uppercase casing on the wire.
        assert!(out.contains("\"ownerID\":"));
        assert!(out.contains("\"monthlyMinutes\":"));
        assert!(!out.contains("\"ownerId\":"));
    }

    #[test]
    fn usage_and_usage_response_roundtrip() {
        let json = r#"{
          "summary": {"earliest": "2026-06-01T00:00:00Z", "latest": "2026-06-30T00:00:00Z", "total": 34.5},
          "details": [
            {"earliest": "2026-06-01T00:00:00Z", "latest": "2026-06-02T00:00:00Z", "total": 1.5}
          ]
        }"#;
        let u: UsageResponse = serde_json::from_str(json).unwrap();
        assert_eq!(u.summary.as_ref().and_then(|s| s.total), Some(34.5));
        assert_eq!(u.details.as_ref().map(|d| d.len()), Some(1));

        let out = roundtrip(&u);
        assert!(out.contains("\"summary\":"));
        assert!(out.contains("\"details\":"));
        assert!(out.contains("\"earliest\":"));
    }
}
