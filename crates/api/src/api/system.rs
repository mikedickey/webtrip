//! System API endpoints.
//!
//! Health checks, URL-shortener redirects, cloud region information, and
//! analytics collection. Obtain a [`SystemApi`] with [`ApiClient::system`].
//!
//! **Authentication:** none of these endpoints require a bearer token, so they
//! work on a freshly constructed [`ApiClient`].
//!
//! # Example
//!
//! ```rust,no_run
//! use webtrip_api::api::{ApiClient, ApiError};
//!
//! # async fn run() -> Result<(), ApiError> {
//! let client = ApiClient::new();
//!
//! let ping = client.system().ping().await?;
//! println!("API version: {:?}", ping.version);
//!
//! // Keyed by region identifier, e.g. "us-west-1".
//! let regions = client.system().list_regions().await?;
//! for (id, region) in &regions {
//!     println!("{id}: {:?}", region.label);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! From JavaScript (the `webtrip` package):
//!
//! ```js
//! import init, { ApiClient } from 'webtrip';
//!
//! await init();
//! const client = new ApiClient();
//!
//! const ping = await client.system().ping();
//! const ip = await client.system().getMyIp();     // e.g. "203.0.113.1"
//! const regions = await client.system().listRegions(); // { [id]: Region }
//! for (const [id, region] of Object.entries(regions)) console.log(id, region.label);
//! ```

#[cfg(feature = "wasm")]
use super::to_js_value;
use super::{ApiClient, ApiError, urlencode};
use crate::models;
use std::collections::HashMap;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

// =============================================================================
// System API
// =============================================================================

api_module_struct!(SystemApi);

// =============================================================================
// Rust API (primary interface)
// =============================================================================

impl SystemApi {
    /// Check API health and version (`GET /ping`).
    pub async fn ping(&self) -> Result<models::Ping, ApiError> {
        self.client.get("/ping").await
    }

    /// Resolve a URL-shortened redirect for a destination identifier
    /// (`GET /redirect/{destination}`), e.g. `"openapi"` resolves to the
    /// OpenAPI spec URL.
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

    /// Get the client's public IP address as seen by the server
    /// (`GET /getmyip`), e.g. `"203.0.113.1"`.
    pub async fn get_my_ip(&self) -> Result<String, ApiError> {
        self.client.get("/getmyip").await
    }

    /// List all cloud regions available for hosting studios.
    ///
    /// `GET /regions` returns a map of region identifier → [`models::Region`]
    /// (not a list); the identifier is the map key, not a field of the region.
    pub async fn list_regions(&self) -> Result<HashMap<String, models::Region>, ApiError> {
        self.client.get("/regions").await
    }

    /// Get details for a single region by identifier (`GET /regions/{region}`).
    pub async fn get_region(&self, region: &str) -> Result<models::Region, ApiError> {
        let path = format!("/regions/{}", urlencode(region));
        self.client.get(&path).await
    }

    /// Submit an analytics event (`POST /collect`). The server returns no body.
    pub async fn collect_analytics(&self, event: &models::AnalyticsEvent) -> Result<(), ApiError> {
        self.client.post_no_response("/collect", event).await
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[cfg(feature = "wasm")]
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

    /// List all cloud regions. Resolves to a plain object mapping region
    /// identifier → `Region`; see [`SystemApi::list_regions`].
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
