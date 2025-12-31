//! User-related models

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// A JackTrip user account
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct User {
    /// Unique user identifier
    #[serde(rename = "user_id", skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// Display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// User nickname
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,

    /// Profile picture URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,

    /// User-editable metadata
    #[serde(rename = "user_metadata", skip_serializing_if = "Option::is_none")]
    pub user_metadata: Option<UserMetadata>,

    /// Application-managed metadata
    #[serde(rename = "app_metadata", skip_serializing_if = "Option::is_none")]
    pub app_metadata: Option<AppMetadata>,
}

/// User-editable profile metadata
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct UserMetadata {
    /// User's email address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// User's display name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// User's location/city
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,

    /// User biography
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bio: Option<String>,

    /// Preferred cloud region
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// User's website URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
}

/// Application-managed user metadata (read-only for users)
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct AppMetadata {
    /// User's subscription plan ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<String>,

    /// Subscription status
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,

    /// Whether user has admin privileges
    #[serde(skip_serializing_if = "Option::is_none")]
    pub admin: Option<bool>,

    /// User's referral code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referral_code: Option<String>,

    /// Stripe customer ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stripe_customer_id: Option<String>,
}

/// A user referral
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Referral {
    /// Referral ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Referral code
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,

    /// User ID of the referrer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referrer_id: Option<String>,

    /// User ID of the referred user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub referred_id: Option<String>,

    /// Creation timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// A user notification
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    /// Notification ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Notification type
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub notification_type: Option<String>,

    /// Notification title
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Notification message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,

    /// Whether the notification has been read
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<bool>,

    /// Creation timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

