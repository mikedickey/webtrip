//! System API endpoints
//!
//! Health checks, region information, analytics, and other system-level operations.

use super::{to_js_value, ApiClient, ApiError, urlencode};
use crate::models;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

// =============================================================================
// System API
// =============================================================================

api_module_struct!(SystemApi);

// =============================================================================
// Rust API (primary interface)
// =============================================================================

impl SystemApi {
    /// Check system health
    pub async fn ping(&self) -> Result<models::Ping, ApiError> {
        self.client.get("/ping").await
    }

    /// Resolve a URL-shortened redirect for a destination identifier
    /// (`GET /redirect/{destination}`).
    pub async fn get_redirect(&self, destination: &str) -> Result<models::Redirect, ApiError> {
        let path = format!("/redirect/{}", urlencode(destination));
        self.client.get(&path).await
    }

    /// Resolve a URL-shortened redirect with a file extension appended to the
    /// resolved URL (`GET /redirect/{destination}/{ext}`).
    pub async fn get_redirect_ext(
        &self,
        destination: &str,
        ext: &str,
    ) -> Result<models::Redirect, ApiError> {
        let path = format!("/redirect/{}/{}", urlencode(destination), urlencode(ext));
        self.client.get(&path).await
    }

    /// Get the client's public IP address
    pub async fn get_my_ip(&self) -> Result<String, ApiError> {
        self.client.get("/getmyip").await
    }

    /// List all available cloud regions
    ///
    /// `GET /regions` returns a map of region identifier → [`models::Region`].
    pub async fn list_regions(&self) -> Result<HashMap<String, models::Region>, ApiError> {
        self.client.get("/regions").await
    }

    /// Get details for a specific region
    pub async fn get_region(&self, region: &str) -> Result<models::Region, ApiError> {
        let path = format!("/regions/{}", urlencode(region));
        self.client.get(&path).await
    }

    /// Submit an analytics event
    pub async fn collect_analytics(&self, event: &models::AnalyticsEvent) -> Result<(), ApiError> {
        self.client.post_no_response("/collect", event).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[wasm_bindgen]
impl SystemApi {
    /// Check system health
    #[wasm_bindgen(js_name = ping)]
    pub async fn ping_js(&self) -> Result<models::Ping, ApiError> {
        self.ping().await
    }

    /// Resolve a URL-shortened redirect for a destination identifier
    #[wasm_bindgen(js_name = getRedirect)]
    pub async fn get_redirect_js(&self, destination: String) -> Result<models::Redirect, ApiError> {
        self.get_redirect(&destination).await
    }

    /// Resolve a URL-shortened redirect with a file extension appended
    #[wasm_bindgen(js_name = getRedirectExt)]
    pub async fn get_redirect_ext_js(
        &self,
        destination: String,
        ext: String,
    ) -> Result<models::Redirect, ApiError> {
        self.get_redirect_ext(&destination, &ext).await
    }

    /// Get the client's public IP address
    #[wasm_bindgen(js_name = getMyIp)]
    pub async fn get_my_ip_js(&self) -> Result<String, ApiError> {
        self.get_my_ip().await
    }

    /// List all available cloud regions
    #[wasm_bindgen(js_name = listRegions)]
    pub async fn list_regions_js(&self) -> Result<JsValue, ApiError> {
        let regions = self.list_regions().await?;
        to_js_value(&regions)
    }

    /// Get details for a specific region
    #[wasm_bindgen(js_name = getRegion)]
    pub async fn get_region_js(&self, region: String) -> Result<models::Region, ApiError> {
        self.get_region(&region).await
    }

    /// Submit an analytics event
    #[wasm_bindgen(js_name = collectAnalytics)]
    pub async fn collect_analytics_js(&self, event: models::AnalyticsEvent) -> Result<(), ApiError> {
        self.collect_analytics(&event).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::api::test_helpers::{assert_http_status, mock_api, mock_empty, mock_json};

    fn api(client: &ApiClient) -> SystemApi {
        SystemApi::from_client(client)
    }

    #[tokio::test]
    async fn test_ping_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/ping",
            200,
            r#"{"status":"ok","version":"1.0.0"}"#,
        )
        .await;

        let ping = api(&client).ping().await.unwrap();
        assert_eq!(ping.status, Some("ok".to_string()));
        assert_eq!(ping.version, Some("1.0.0".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_ping_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/ping", 503, "Service Unavailable").await;

        let err = api(&client).ping().await.unwrap_err();
        assert_http_status(err, 503);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_redirect_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/redirect/openapi",
            200,
            r#"{"redirect":"https://example.com/openapi.json"}"#,
        )
        .await;

        let result = api(&client).get_redirect("openapi").await.unwrap();
        assert_eq!(result.redirect, Some("https://example.com/openapi.json".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_redirect_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/redirect/missing", 404, "nope").await;

        let err = api(&client).get_redirect("missing").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_redirect_ext_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/redirect/avatar/jpg",
            200,
            r#"{"redirect":"https://cdn.example.com/avatar.jpg"}"#,
        )
        .await;

        let result = api(&client).get_redirect_ext("avatar", "jpg").await.unwrap();
        assert_eq!(result.redirect, Some("https://cdn.example.com/avatar.jpg".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_redirect_ext_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/redirect/avatar/jpg", 404, "nope").await;

        let err = api(&client).get_redirect_ext("avatar", "jpg").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_my_ip_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/getmyip", 200, r#""203.0.113.7""#).await;

        let ip = api(&client).get_my_ip().await.unwrap();
        assert_eq!(ip, "203.0.113.7");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_my_ip_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/getmyip", 500, "boom").await;

        let err = api(&client).get_my_ip().await.unwrap_err();
        assert_http_status(err, 500);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_regions_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/regions",
            200,
            r#"{"us-west-1":{"label":"US West","provider":"gcloud"}}"#,
        )
        .await;

        let regions = api(&client).list_regions().await.unwrap();
        assert_eq!(regions.len(), 1);
        let region = regions.get("us-west-1").expect("region present");
        assert_eq!(region.label, Some("US West".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_regions_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/regions", 500, "Internal Server Error").await;

        let err = api(&client).list_regions().await.unwrap_err();
        assert_http_status(err, 500);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_region_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/regions/us-east-1",
            200,
            r#"{"label":"US East","provider":"aws"}"#,
        )
        .await;

        let region = api(&client).get_region("us-east-1").await.unwrap();
        assert_eq!(region.label, Some("US East".to_string()));
        assert_eq!(region.provider, Some("aws".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_region_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/regions/nowhere", 404, "nope").await;

        let err = api(&client).get_region("nowhere").await.unwrap_err();
        assert_http_status(err, 404);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_collect_analytics_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_empty(&mut server, "POST", "/collect", 204).await;

        let event = models::AnalyticsEvent::default();
        api(&client).collect_analytics(&event).await.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_collect_analytics_error() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "POST", "/collect", 400, "bad event").await;

        let event = models::AnalyticsEvent::default();
        let err = api(&client).collect_analytics(&event).await.unwrap_err();
        assert_http_status(err, 400);
        mock.assert_async().await;
    }
}
