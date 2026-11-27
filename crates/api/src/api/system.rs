//! System API endpoints.
//!
//! Health checks, the caller's public IP, cloud region information, and
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
//! println!("API status: {:?}", ping.status);
//!
//! // Keyed by region identifier, e.g. "ec2-us-oregon".
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
//! const ping = await client.system().ping();          // { status: "OK" }
//! const ip = await client.system().getMyIp();         // e.g. "203.0.113.42"
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

api_module_struct!(
    /// Health, client-IP, region and analytics endpoints; obtain with
    /// [`ApiClient::system`].
    SystemApi
);

impl SystemApi {
    /// Check API health (`GET /ping`); a healthy API answers `status: "OK"`.
    pub async fn ping(&self) -> Result<models::Ping, ApiError> {
        self.client.get("/ping").await
    }

    /// Get the caller's public IP address as seen by the server
    /// (`GET /getmyip`), e.g. `"203.0.113.42"`.
    pub async fn get_my_ip(&self) -> Result<String, ApiError> {
        Ok(self.client.get_text("/getmyip").await?.trim().to_string())
    }

    /// List all cloud regions available for hosting studios.
    ///
    /// `GET /regions` returns a map of region identifier → [`models::Region`]
    /// (not a list); the identifier, e.g. `ec2-us-oregon`, is the map key.
    /// List-response regions omit `scaleParams`; use
    /// [`get_region`](Self::get_region) for it.
    pub async fn list_regions(&self) -> Result<HashMap<String, models::Region>, ApiError> {
        self.client.get("/regions").await
    }

    /// Get details for a single region by identifier (`GET /regions/{region}`).
    ///
    /// # Errors
    ///
    /// [`ApiError::Http`] `404` for an unknown region.
    pub async fn get_region(&self, region: &str) -> Result<models::Region, ApiError> {
        let path = format!("/regions/{}", urlencode(region));
        self.client.get(&path).await
    }

    /// Submit an analytics event (`POST /collect`), returning the event as
    /// the server recorded it (with `time` filled in).
    pub async fn collect_analytics(&self, event: &models::AnalyticsEvent) -> Result<models::AnalyticsEvent, ApiError> {
        self.client.post("/collect", event).await
    }
}

#[cfg(feature = "wasm")]
#[wasm_bindgen]
impl SystemApi {
    /// Check API health; see [`SystemApi::ping`].
    #[wasm_bindgen(js_name = ping)]
    pub async fn ping_js(&self) -> Result<models::Ping, ApiError> {
        self.ping().await
    }

    /// Get the caller's public IP address; see [`SystemApi::get_my_ip`].
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

    /// Get details for one region; see [`SystemApi::get_region`].
    #[wasm_bindgen(js_name = getRegion)]
    pub async fn get_region_js(&self, region: String) -> Result<models::Region, ApiError> {
        self.get_region(&region).await
    }

    /// Submit an analytics event; see [`SystemApi::collect_analytics`].
    #[wasm_bindgen(js_name = collectAnalytics)]
    pub async fn collect_analytics_js(&self, event: models::AnalyticsEvent) -> Result<models::AnalyticsEvent, ApiError> {
        self.collect_analytics(&event).await
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::api::test_helpers::{assert_http_status, mock_api, mock_json};

    fn api(client: &ApiClient) -> SystemApi {
        SystemApi::from_client(client)
    }

    #[tokio::test]
    async fn test_ping_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(&mut server, "GET", "/ping", 200, r#"{"status":"OK"}"#).await;

        let ping = api(&client).ping().await.unwrap();
        assert_eq!(ping.status, Some("OK".to_string()));
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
    async fn test_get_my_ip_reads_plain_text_body() {
        // Regression: the body is `text/plain` (`203.0.113.42`), not a JSON
        // string, and was decoded as JSON, so this always failed.
        let (mut server, client) = mock_api().await;
        let mock = server
            .mock("GET", "/getmyip")
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body("203.0.113.42\n")
            .create_async()
            .await;

        let ip = api(&client).get_my_ip().await.unwrap();
        assert_eq!(ip, "203.0.113.42");
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
            r#"{"ec2-us-oregon":{"label":"USA - Boardman, OR","provider":"ec2"}}"#,
        )
        .await;

        let regions = api(&client).list_regions().await.unwrap();
        assert_eq!(regions.len(), 1);
        let region = regions.get("ec2-us-oregon").expect("region present");
        assert_eq!(region.label, Some("USA - Boardman, OR".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_region_success() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "GET",
            "/regions/ec2-us-oregon",
            200,
            r#"{"label":"USA - Boardman, OR","provider":"ec2"}"#,
        )
        .await;

        let region = api(&client).get_region("ec2-us-oregon").await.unwrap();
        assert_eq!(region.provider, Some("ec2".to_string()));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_collect_analytics_returns_recorded_event() {
        let (mut server, client) = mock_api().await;
        let mock = mock_json(
            &mut server,
            "POST",
            "/collect",
            200,
            r#"{"name":"vs_session","time":"2026-06-14T00:00:00Z"}"#,
        )
        .await;

        let event = models::AnalyticsEvent { name: Some("vs_session".into()), ..Default::default() };
        let recorded = api(&client).collect_analytics(&event).await.unwrap();
        assert_eq!(recorded.time.as_deref(), Some("2026-06-14T00:00:00Z"));
        mock.assert_async().await;
    }
}
