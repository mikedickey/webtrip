//! Billing models
//!
//! Billing is Stripe-backed: plan changes happen via the hosted billing portal,
//! so these models only describe what the client needs to read.

use serde::{Deserialize, Serialize};
#[cfg(feature = "wasm")]
use tsify_next::Tsify;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// Resolved plan pricing (`GET /users/{userId}/plans`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
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
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
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

/// Studio usage over one time span.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Start of the span (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub earliest: Option<String>,

    /// End of the span (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,

    /// Musician minutes used within the span
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<f64>,
}

/// Studio usage (`GET /usage`): a total plus one [`Usage`] per day.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
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
}
