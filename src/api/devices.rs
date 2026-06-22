//! Devices API endpoints
//!
//! JackTrip device management and configuration.

use super::{to_js_value, ApiClient, ApiError, urlencode};
use crate::models;
use serde::Serialize;
use wasm_bindgen::prelude::*;

// =============================================================================
// Devices API
// =============================================================================

api_module_struct!(DevicesApi);

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
    #[wasm_bindgen(js_name = listDevices)]
    pub async fn list_devices_js(&self) -> Result<JsValue, ApiError> {
        let devices = self.list_devices().await?;
        to_js_value(&devices)
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
        to_js_value(&devices)
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::api::test_helpers::{assert_http_status, mock_api, mock_empty, mock_json};

    fn api(client: &ApiClient) -> DevicesApi {
        DevicesApi::from_client(client)
    }

    #[tokio::test]
    async fn test_list_devices_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/devices",
            200,
            r#"[{"id":"device1","name":"Test Device"}]"#,
        )
        .await;

        let result = api(&client).list_devices().await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, Some("device1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_devices_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/devices", 403, "Forbidden").await;

        let err = api(&client).list_devices().await.unwrap_err();
        assert_http_status(err, 403);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_register_device_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/devices",
            200,
            r#"{"id":"dev123","name":"New Device"}"#,
        )
        .await;

        let body = models::Device::default();
        let device = api(&client).register_device(&body).await.unwrap();
        assert_eq!(device.id, Some("dev123".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_register_device_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "POST", "/devices", 400, "bad").await;

        let body = models::Device::default();
        let err = api(&client).register_device(&body).await.unwrap_err();
        assert_http_status(err, 400);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_device_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/devices/dev123",
            200,
            r#"{"id":"dev123","name":"My Device"}"#,
        )
        .await;

        let device = api(&client).get_device("dev123").await.unwrap();
        assert_eq!(device.id, Some("dev123".to_string()));
        assert_eq!(device.name, Some("My Device".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_device_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/devices/nonexistent", 404, "Not Found").await;

        let err = api(&client).get_device("nonexistent").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_device_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "PUT",
            "/devices/dev123",
            200,
            r#"{"id":"dev123","name":"Renamed"}"#,
        )
        .await;

        let body = models::Device::default();
        let device = api(&client).update_device("dev123", &body).await.unwrap();
        assert_eq!(device.name, Some("Renamed".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_device_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/devices/dev123", 204).await;

        api(&client).delete_device("dev123").await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_device_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "DELETE", "/devices/dev123", 404).await;

        let err = api(&client).delete_device("dev123").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_send_heartbeat_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/devices/dev123/heartbeat",
            200,
            r#"{"device":{"id":"dev123"}}"#,
        )
        .await;

        let body = models::HeartbeatRequest::default();
        let config = api(&client).send_heartbeat("dev123", &body).await.unwrap();
        assert_eq!(config.device.unwrap().id, Some("dev123".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_studio_devices_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/studios/st1/devices",
            200,
            r#"[{"id":"dev1"}]"#,
        )
        .await;

        let result = api(&client).list_studio_devices("st1").await.unwrap();
        assert_eq!(result[0].id, Some("dev1".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_capture_volume_with_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "PUT", "/studios/st1/captureVolume", 204).await;

        api(&client)
            .update_capture_volume("st1", Some(0), Some(100))
            .await
            .unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_capture_volume_without_params() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "PUT", "/studios/st1/captureVolume", 204).await;

        api(&client)
            .update_capture_volume("st1", None, None)
            .await
            .unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_capture_volume_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "PUT", "/studios/st1/captureVolume", 403).await;

        let err = api(&client)
            .update_capture_volume("st1", Some(0), Some(100))
            .await
            .unwrap_err();
        assert_http_status(err, 403);
        mock.assert_async().await;
    }
}
