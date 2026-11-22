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

#[cfg(test)]
mod tests {
    use super::*;

    use super::super::test_utils::roundtrip;

    #[test]
    fn user_fixture_matches_wire_format() {
        // Fixture modeled after docs/api/users.md (auth0-style user). Note
        // user_id / user_metadata / app_metadata are snake_case on the wire
        // even though the rest of the struct is camelCase.
        let json = r#"{
          "user_id": "auth0|abc123",
          "name": "Ada Lovelace",
          "nickname": "ada",
          "picture": "https://example.com/ada.png",
          "user_metadata": {"email": "ada@example.com", "location": "London"},
          "app_metadata": {"plan": "pro", "admin": true}
        }"#;
        let u: User = serde_json::from_str(json).unwrap();
        assert_eq!(u.user_id.as_deref(), Some("auth0|abc123"));
        assert_eq!(u.user_metadata.as_ref().and_then(|m| m.email.as_deref()), Some("ada@example.com"));
        assert_eq!(u.app_metadata.as_ref().and_then(|m| m.admin), Some(true));

        let s = serde_json::to_string(&u).unwrap();
        assert!(s.contains("\"user_id\":"));
        assert!(s.contains("\"user_metadata\":"));
        assert!(s.contains("\"app_metadata\":"));
        assert!(!s.contains("\"userId\":"));
    }

    #[test]
    fn notification_type_field_renames_to_type() {
        let n = Notification {
            id: Some("n1".into()),
            notification_type: Some("studio_invite".into()),
            title: Some("Welcome".into()),
            message: Some("Hello".into()),
            read: Some(false),
            created_at: Some("2026-06-14T00:00:00Z".into()),
        };
        let s = roundtrip(&n);
        assert!(s.contains("\"type\":\"studio_invite\""));
        assert!(!s.contains("notificationType"));
        let raw = r#"{"id":"x","type":"like","read":true}"#;
        let n2: Notification = serde_json::from_str(raw).unwrap();
        assert_eq!(n2.notification_type.as_deref(), Some("like"));
    }
}
