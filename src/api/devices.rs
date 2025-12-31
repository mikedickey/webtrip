//! Devices API endpoints
//!
//! JackTrip device management and configuration.

use super::{ApiClient, ApiError, urlencode};
use crate::models;
use serde::Serialize;
use wasm_bindgen::prelude::*;

// =============================================================================
// Devices API
// =============================================================================

/// Devices API for JackTrip hardware management
#[wasm_bindgen]
pub struct DevicesApi {
    client: ApiClient,
}

impl DevicesApi {
    pub(crate) fn from_client(client: &ApiClient) -> Self {
        Self {
            client: client.clone(),
        }
    }
}

// =============================================================================
// Rust API (primary interface)
// =============================================================================

impl DevicesApi {
    /// List all devices in the account
    pub async fn list_devices(&self) -> Result<Vec<models::Device>, ApiError> {
        self.client.get("/devices").await
    }

    /// Register a new device
    pub async fn register_device(&self, device: &models::Device) -> Result<models::Device, ApiError> {
        self.client.post("/devices", device).await
    }

    /// Get a device by ID
    pub async fn get_device(&self, device_id: &str) -> Result<models::Device, ApiError> {
        let path = format!("/devices/{}", urlencode(device_id));
        self.client.get(&path).await
    }

    /// Update a device's configuration
    pub async fn update_device(&self, device_id: &str, device: &models::Device) -> Result<models::Device, ApiError> {
        let path = format!("/devices/{}", urlencode(device_id));
        self.client.put(&path, device).await
    }

    /// Delete a device
    pub async fn delete_device(&self, device_id: &str) -> Result<(), ApiError> {
        let path = format!("/devices/{}", urlencode(device_id));
        self.client.delete(&path).await
    }

    /// Send a device heartbeat
    pub async fn send_heartbeat(
        &self,
        device_id: &str,
        heartbeat: &models::HeartbeatRequest,
    ) -> Result<models::DeviceAgentConfig, ApiError> {
        let path = format!("/devices/{}/heartbeat", urlencode(device_id));
        self.client.post(&path, heartbeat).await
    }

    /// List devices connected to a studio
    pub async fn list_studio_devices(&self, studio_id: &str) -> Result<Vec<models::Device>, ApiError> {
        let path = format!("/studios/{}/devices", urlencode(studio_id));
        self.client.get(&path).await
    }

    /// Update capture volume for all devices in a studio
    pub async fn update_capture_volume(
        &self,
        studio_id: &str,
        min: Option<i32>,
        max: Option<i32>,
    ) -> Result<(), ApiError> {
        let path = format!("/studios/{}/captureVolume", urlencode(studio_id));

        #[derive(Serialize)]
        struct VolumeParams {
            #[serde(skip_serializing_if = "Option::is_none")]
            min: Option<i32>,
            #[serde(skip_serializing_if = "Option::is_none")]
            max: Option<i32>,
        }

        self.client.put_with_query(&path, &VolumeParams { min, max }).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[wasm_bindgen]
impl DevicesApi {
    #[wasm_bindgen(constructor)]
    pub fn new(client: &ApiClient) -> Self {
        Self::from_client(client)
    }

    #[wasm_bindgen(js_name = listDevices)]
    pub async fn list_devices_js(&self) -> Result<JsValue, ApiError> {
        let devices = self.list_devices().await?;
        serde_wasm_bindgen::to_value(&devices).map_err(|e| ApiError::Serialization(e.to_string()))
    }

    #[wasm_bindgen(js_name = registerDevice)]
    pub async fn register_device_js(&self, device: models::Device) -> Result<models::Device, ApiError> {
        self.register_device(&device).await
    }

    #[wasm_bindgen(js_name = getDevice)]
    pub async fn get_device_js(&self, device_id: String) -> Result<models::Device, ApiError> {
        self.get_device(&device_id).await
    }

    #[wasm_bindgen(js_name = updateDevice)]
    pub async fn update_device_js(&self, device_id: String, device: models::Device) -> Result<models::Device, ApiError> {
        self.update_device(&device_id, &device).await
    }

    #[wasm_bindgen(js_name = deleteDevice)]
    pub async fn delete_device_js(&self, device_id: String) -> Result<(), ApiError> {
        self.delete_device(&device_id).await
    }

    #[wasm_bindgen(js_name = sendHeartbeat)]
    pub async fn send_heartbeat_js(
        &self,
        device_id: String,
        heartbeat: models::HeartbeatRequest,
    ) -> Result<models::DeviceAgentConfig, ApiError> {
        self.send_heartbeat(&device_id, &heartbeat).await
    }

    #[wasm_bindgen(js_name = listStudioDevices)]
    pub async fn list_studio_devices_js(&self, studio_id: String) -> Result<JsValue, ApiError> {
        let devices = self.list_studio_devices(&studio_id).await?;
        serde_wasm_bindgen::to_value(&devices).map_err(|e| ApiError::Serialization(e.to_string()))
    }

    #[wasm_bindgen(js_name = updateCaptureVolume)]
    pub async fn update_capture_volume_js(
        &self,
        studio_id: String,
        min: Option<i32>,
        max: Option<i32>,
    ) -> Result<(), ApiError> {
        self.update_capture_volume(&studio_id, min, max).await
    }
}
