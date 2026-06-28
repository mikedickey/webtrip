//! Cloud region models

use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Cloud region details
///
/// Represents a cloud region where JackTrip servers can be deployed.
/// The API returns a map of region IDs (strings) to Region objects.
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct Region {
    /// Geographic group (e.g., "Americas", "Europe", "Asia", "Oceania", "Africa")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,

    /// Cloud provider (e.g., "azure", "gcloud", "ec2", "lumen")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,

    /// Cloud provider's internal region code (e.g., "uaenorth", "us-east4")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    /// Human-readable label (e.g., "UAE - Dubai", "USA - Salt Lake City, UT")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// VM image ID for x86_64 instances
    #[serde(default, rename = "imageId", skip_serializing_if = "Option::is_none")]
    pub image_id: Option<String>,

    /// VM image ID for ARM instances
    #[serde(default, rename = "armImageId", skip_serializing_if = "Option::is_none")]
    pub arm_image_id: Option<String>,

    /// VM image ID for remote-root instances
    #[serde(default, rename = "remoteRootImageId", skip_serializing_if = "Option::is_none")]
    pub remote_root_image_id: Option<String>,

    /// Subnet identifier
    #[serde(default, rename = "subnetId", skip_serializing_if = "Option::is_none")]
    pub subnet_id: Option<String>,

    /// Provider security-group identifiers applied to studio VMs
    #[serde(default, rename = "securityGroupIds", skip_serializing_if = "Option::is_none")]
    pub security_group_ids: Option<Vec<String>>,

    /// Available instance types for this region
    #[serde(default, rename = "instanceTypes", skip_serializing_if = "Option::is_none")]
    pub instance_types: Option<Vec<InstanceType>>,

    /// Whether this region is currently active/available
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<bool>,

    /// Latitude coordinate for map display
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,

    /// Longitude coordinate for map display
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,

    /// Cloud host URL for this region
    #[serde(default, rename = "cloudHost", skip_serializing_if = "Option::is_none")]
    pub cloud_host: Option<String>,

    /// Availability zone within the region where studios are provisioned
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub zone: Option<String>,

    /// Name of the provider-managed volume attached to studios in this region
    #[serde(default, rename = "volumeName", skip_serializing_if = "Option::is_none")]
    pub volume_name: Option<String>,

    /// Type of the provider-managed volume attached to studios in this region
    #[serde(default, rename = "volumeType", skip_serializing_if = "Option::is_none")]
    pub volume_type: Option<String>,

    /// Server-side autoscaling parameters (only populated on single-region responses)
    #[serde(default, rename = "scaleParams", skip_serializing_if = "Option::is_none")]
    pub scale_params: Option<serde_json::Value>,
}

/// Instance type available in a region
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub struct InstanceType {
    /// Instance type identifier (e.g., "Standard_B1s", "t2d-standard-1")
    #[serde(default)]
    pub id: String,

    /// Instance family (e.g., "Bs", "t2d-standard", "n2-highcpu")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,

    /// Number of virtual CPUs
    #[serde(default, rename = "vCPU", skip_serializing_if = "Option::is_none")]
    pub vcpu: Option<u32>,

    /// Maximum number of participants supported
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn region_fixture_known_good() {
        // Fixture modeled after the system regions endpoint described in
        // docs/api/system.md. Region uses explicit per-field renames rather
        // than rename_all; verify those are honoured on the wire.
        let json = r#"{
          "group": "Americas",
          "provider": "gcloud",
          "region": "us-west3",
          "label": "USA - Salt Lake City, UT",
          "imageId": "img-abc",
          "armImageId": "img-arm-abc",
          "subnetId": "sub-1",
          "instanceTypes": [
            {"id": "n2-highcpu-2", "family": "n2-highcpu", "vCPU": 2, "max": 4}
          ],
          "active": true,
          "latitude": 40.76,
          "longitude": -111.89,
          "cloudHost": "https://gcloud.example.com"
        }"#;
        let r: Region = serde_json::from_str(json).unwrap();
        assert_eq!(r.region.as_deref(), Some("us-west3"));
        assert_eq!(r.instance_types.as_ref().map(|v| v.len()), Some(1));
        assert_eq!(
            r.instance_types.as_ref().and_then(|v| v.first()).map(|i| i.id.as_str()),
            Some("n2-highcpu-2")
        );

        let out = serde_json::to_string(&r).unwrap();
        assert!(out.contains("\"imageId\":"));
        assert!(out.contains("\"armImageId\":"));
        assert!(out.contains("\"subnetId\":"));
        assert!(out.contains("\"instanceTypes\":"));
        assert!(out.contains("\"cloudHost\":"));
        assert!(out.contains("\"vCPU\":2"));
        // Region uses field-level renames, so snake_case keys for unrenamed
        // fields should still appear (e.g. "group", "label")
        assert!(out.contains("\"group\":\"Americas\""));
    }
}

