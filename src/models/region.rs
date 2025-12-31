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
    /// Region identifier (e.g., "azure-ae-dubai", "gcloud-us-ut-slc")
    /// This is the key from the API response map, added during conversion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

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

    /// Subnet identifier
    #[serde(default, rename = "subnetId", skip_serializing_if = "Option::is_none")]
    pub subnet_id: Option<String>,

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
