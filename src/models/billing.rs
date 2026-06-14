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

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T>(v: &T) -> String
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let s = serde_json::to_string(v).expect("serialize");
        let back: T = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(v, &back);
        s
    }

    #[test]
    fn billing_info_fixture_known_good() {
        // Fixture modeled after docs/api/billing.md.
        let json = r#"{
          "userId": "user-1",
          "plan": {
            "id": "pro",
            "name": "Pro",
            "price": 1999,
            "currency": "usd",
            "interval": "month",
            "maxStudios": 3,
            "maxDevices": 5,
            "maxMusicians": 10,
            "studioHours": 100,
            "recordingStorageGb": 50,
            "broadcastEnabled": true,
            "regions": ["us-west-2", "us-east-1"],
            "features": ["broadcast", "recording"]
          },
          "status": "active",
          "stripeCustomerId": "cus_abc",
          "periodStart": "2026-06-01T00:00:00Z",
          "periodEnd": "2026-07-01T00:00:00Z",
          "cancelAtPeriodEnd": false
        }"#;
        let b: BillingInfo = serde_json::from_str(json).unwrap();
        assert_eq!(b.plan.as_ref().and_then(|p| p.id.as_deref()), Some("pro"));
        assert_eq!(b.plan.as_ref().and_then(|p| p.max_studios), Some(3));

        let out = serde_json::to_string(&b).unwrap();
        assert!(out.contains("\"userId\":"));
        assert!(out.contains("\"stripeCustomerId\":"));
        assert!(out.contains("\"cancelAtPeriodEnd\":"));
        assert!(out.contains("\"recordingStorageGb\":"));
    }

    #[test]
    fn plan_roundtrip_full() {
        let p = Plan {
            id: Some("free".into()),
            name: Some("Free".into()),
            description: Some("Free tier".into()),
            price: Some(0),
            currency: Some("usd".into()),
            interval: Some("month".into()),
            max_studios: Some(1),
            max_devices: Some(1),
            max_musicians: Some(2),
            studio_hours: Some(5),
            recording_storage_gb: Some(1),
            broadcast_enabled: Some(false),
            regions: Some(vec!["us-west-2".into()]),
            features: Some(vec!["basic".into()]),
        };
        roundtrip(&p);
    }

    #[test]
    fn subscription_roundtrip_camel_case() {
        let s = Subscription {
            id: Some("sub_1".into()),
            plan_id: Some("pro".into()),
            status: Some("active".into()),
            current_period_start: Some("2026-06-01T00:00:00Z".into()),
            current_period_end: Some("2026-07-01T00:00:00Z".into()),
            cancel_at_period_end: Some(false),
            canceled_at: None,
        };
        let out = roundtrip(&s);
        assert!(out.contains("\"planId\":"));
        assert!(out.contains("\"currentPeriodStart\":"));
        assert!(out.contains("\"cancelAtPeriodEnd\":"));
    }

    #[test]
    fn usage_and_usage_response_roundtrip() {
        let u = Usage {
            studio_hours: Some(12.5),
            active_studios: Some(2),
            devices: Some(3),
            recording_storage: Some(1_500_000_000),
            period_start: Some("2026-06-01T00:00:00Z".into()),
            period_end: Some("2026-07-01T00:00:00Z".into()),
        };
        let out = roundtrip(&u);
        assert!(out.contains("\"studioHours\":12.5"));
        assert!(out.contains("\"recordingStorage\":1500000000"));

        let r = UsageResponse {
            usage: Some(u),
            limits: Some(Plan { id: Some("free".into()), ..Default::default() }),
        };
        roundtrip(&r);
    }
}

