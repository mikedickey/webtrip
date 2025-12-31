//! System API endpoints
//!
//! Health checks, region information, analytics, and other system-level operations.

use super::{ApiClient, ApiError, urlencode};
use crate::models;
use std::collections::HashMap;
use wasm_bindgen::prelude::*;

// =============================================================================
// System API
// =============================================================================

/// System API for health checks, regions, and analytics
#[wasm_bindgen]
pub struct SystemApi {
    client: ApiClient,
}

impl SystemApi {
    pub(crate) fn from_client(client: &ApiClient) -> Self {
        Self {
            client: client.clone(),
        }
    }
}

// =============================================================================
// Rust API (primary interface)
// =============================================================================

impl SystemApi {
    /// Check system health
    pub async fn ping(&self) -> Result<models::Ping, ApiError> {
        self.client.get("/ping").await
    }

    /// Get redirect URL for a destination
    /// 
    /// Note: This endpoint returns a 307 redirect. This method will follow
    /// the redirect and return the final URL as a string.
    pub async fn get_redirect_url(&self, destination: &str) -> Result<String, ApiError> {
        let path = format!("/redirect/{}", urlencode(destination));
        // For redirects, we might want to just return the redirect URL
        // The actual implementation depends on how you want to handle 307 redirects
        self.client.get(&path).await
    }

    /// Get the client's public IP address
    pub async fn get_my_ip(&self) -> Result<String, ApiError> {
        self.client.get("/getmyip").await
    }

    /// List all available cloud regions
    ///
    /// Returns a Vec of regions with IDs. The API returns a map, but we convert
    /// it to a Vec with the region ID included in each Region object.
    pub async fn list_regions(&self) -> Result<Vec<models::Region>, ApiError> {
        let map: HashMap<String, models::Region> = self.client.get("/regions").await?;
        
        let regions: Vec<models::Region> = map
            .into_iter()
            .map(|(id, mut region)| {
                region.id = Some(id);
                region
            })
            .collect();
        
        Ok(regions)
    }

    /// Get details for a specific region
    pub async fn get_region(&self, region: &str) -> Result<models::Region, ApiError> {
        let path = format!("/regions/{}", urlencode(region));
        self.client.get(&path).await
    }

    /// Submit an analytics event
    pub async fn collect_analytics(&self, event: &models::AnalyticsEvent) -> Result<(), ApiError> {
        let _: serde_json::Value = self.client.post("/collect", event).await?;
        Ok(())
    }
}

// =============================================================================
// JavaScript API (wasm_bindgen wrappers)
// =============================================================================

#[wasm_bindgen]
impl SystemApi {
    /// Create a new System API client
    #[wasm_bindgen(constructor)]
    pub fn new(client: &ApiClient) -> Self {
        Self::from_client(client)
    }

    /// Check system health
    #[wasm_bindgen(js_name = ping)]
    pub async fn ping_js(&self) -> Result<models::Ping, ApiError> {
        self.ping().await
    }

    /// Get redirect URL for a destination
    #[wasm_bindgen(js_name = getRedirectUrl)]
    pub async fn get_redirect_url_js(&self, destination: String) -> Result<String, ApiError> {
        self.get_redirect_url(&destination).await
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
        serde_wasm_bindgen::to_value(&regions).map_err(|e| ApiError::Serialization(e.to_string()))
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
