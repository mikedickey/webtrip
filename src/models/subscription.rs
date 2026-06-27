//! Studio subscription (membership) model
//!
//! A "subscription" here is **not** a billing plan — it represents a user who is
//! a member of a studio. Consumed by the studio-subscriptions API module.

use super::ModifiedAtTime;
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// A studio membership ("subscription") linking a user to a studio.
///
/// Note the mixed wire casing: `user_id` and `updated_at` are snake_case while
/// `serverId` is camelCase, so every field uses an explicit serde rename.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Subscription {
    /// Shared creation/update timestamps (`createdAt`, `updatedAt`)
    #[serde(flatten)]
    pub modified: ModifiedAtTime,

    /// User ID
    #[serde(rename = "user_id", skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// User name
    #[serde(rename = "name", skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// User nickname
    #[serde(rename = "nickname", skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,

    /// URL to profile picture
    #[serde(rename = "picture", skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,

    /// User email address
    #[serde(rename = "email", skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,

    /// RFC3339-formatted timestamp of the last user-profile update
    #[serde(rename = "updated_at", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,

    /// Studio ID
    #[serde(rename = "serverId", skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,

    /// Whether or not the user is an admin
    #[serde(rename = "admin", skip_serializing_if = "Option::is_none")]
    pub admin: Option<bool>,

    /// Studio subscription status ("Active" | "Deleted")
    #[serde(rename = "status", skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::test_utils::roundtrip;

    #[test]
    fn subscription_mixed_casing_fixture() {
        let json = r#"{
          "user_id": "auth0|abc",
          "name": "Ada Lovelace",
          "nickname": "ada",
          "picture": "https://example.com/ada.png",
          "email": "ada@example.com",
          "updated_at": "2026-06-14T00:00:00Z",
          "serverId": "studio-1",
          "admin": true,
          "status": "Active",
          "createdAt": "2026-06-01T00:00:00Z",
          "updatedAt": "2026-06-10T00:00:00Z"
        }"#;
        let s: Subscription = serde_json::from_str(json).unwrap();
        assert_eq!(s.user_id.as_deref(), Some("auth0|abc"));
        assert_eq!(s.server_id.as_deref(), Some("studio-1"));
        assert_eq!(s.updated_at.as_deref(), Some("2026-06-14T00:00:00Z"));
        assert_eq!(s.admin, Some(true));
        assert_eq!(s.status.as_deref(), Some("Active"));
        // Flattened ModifiedAtTime timestamps.
        assert_eq!(s.modified.created_at.as_deref(), Some("2026-06-01T00:00:00Z"));
        assert_eq!(s.modified.updated_at.as_deref(), Some("2026-06-10T00:00:00Z"));

        let out = roundtrip(&s);
        // Mixed casing preserved exactly on the wire.
        assert!(out.contains("\"user_id\":"));
        assert!(out.contains("\"updated_at\":"));
        assert!(out.contains("\"serverId\":"));
        assert!(out.contains("\"createdAt\":"));
        assert!(out.contains("\"updatedAt\":"));
        assert!(!out.contains("\"userId\":"));
    }
}
