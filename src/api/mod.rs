//! JackTrip Virtual Studio API Client
//!
//! A clean, ergonomic Rust API client for the JackTrip Virtual Studio platform.
//! All functions are async and exposed to JavaScript via wasm-bindgen.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use wasm_bindgen::prelude::*;

// Macro to generate API module struct + constructors boilerplate
macro_rules! api_module_struct {
    ($name:ident) => {
        #[wasm_bindgen]
        pub struct $name {
            client: ApiClient,
        }

        impl $name {
            pub(crate) fn from_client(client: &ApiClient) -> Self {
                Self {
                    client: client.clone(),
                }
            }
        }

        #[wasm_bindgen]
        impl $name {
            #[wasm_bindgen(constructor)]
            pub fn new() -> Self {
                Self {
                    client: ApiClient::new(),
                }
            }
        }
    };
}

// Re-export all API modules
pub mod billing;
pub mod devices;
pub mod events;
pub mod recordings;
pub mod streams;
pub mod studios;
pub mod system;
pub mod users;

// Re-export models from the existing models module
pub use crate::models;

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) mod test_helpers;

/// Default timeout in milliseconds (10 seconds)
pub const DEFAULT_TIMEOUT_MS: u32 = 10_000;

/// Default base URL for the JackTrip API
pub const DEFAULT_BASE_URL: &str = "https://www.jacktrip.com/api";

// =============================================================================
// API Client
// =============================================================================

/// API Client for JackTrip Virtual Studio
///
/// The main entry point for making API requests. Holds configuration and
/// reuses HTTP connections across multiple API calls.
///
/// # Example (Rust)
/// ```ignore
/// let client = ApiClient::new();
/// client.set_bearer_token("your-jwt-token".into());
/// let user = client.users().get_current_user().await?;
/// ```
///
/// # Example (JavaScript)
/// ```javascript
/// const client = new ApiClient();
/// client.setBearerToken("your-jwt-token");
/// const user = await client.users().getCurrentUser();
/// ```
#[wasm_bindgen]
#[derive(Debug)]
pub struct ApiClient {
    http: reqwest::Client,
    base_url: String,
    bearer_token: Option<String>,
    user_agent: Option<String>,
    timeout_ms: u32,
    #[wasm_bindgen(skip)]
    pub headers: HashMap<String, String>,
}

#[wasm_bindgen]
impl ApiClient {
    /// Create a new API client with default settings
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an API client with a custom base URL
    #[wasm_bindgen(js_name = withBaseUrl)]
    pub fn with_base_url(base_url: String) -> Self {
        Self {
            base_url,
            ..Self::default()
        }
    }

    /// Set the base URL
    #[wasm_bindgen(js_name = setBaseUrl)]
    pub fn set_base_url(&mut self, base_url: String) {
        self.base_url = base_url;
    }

    /// Get the base URL
    #[wasm_bindgen(js_name = getBaseUrl)]
    pub fn get_base_url(&self) -> String {
        self.base_url.clone()
    }

    /// Set the bearer token for authentication
    #[wasm_bindgen(js_name = setBearerToken)]
    pub fn set_bearer_token(&mut self, token: String) {
        self.bearer_token = Some(token);
    }

    /// Clear the bearer token
    #[wasm_bindgen(js_name = clearBearerToken)]
    pub fn clear_bearer_token(&mut self) {
        self.bearer_token = None;
    }

    /// Check if a bearer token is set
    #[wasm_bindgen(js_name = hasBearerToken)]
    pub fn has_bearer_token(&self) -> bool {
        self.bearer_token.is_some()
    }

    /// Set the user agent string
    #[wasm_bindgen(js_name = setUserAgent)]
    pub fn set_user_agent(&mut self, user_agent: String) {
        self.user_agent = Some(user_agent);
    }

    /// Set the request timeout in milliseconds
    #[wasm_bindgen(js_name = setTimeoutMs)]
    pub fn set_timeout_ms(&mut self, timeout_ms: u32) {
        self.timeout_ms = timeout_ms;
    }

    /// Get the request timeout in milliseconds
    #[wasm_bindgen(js_name = getTimeoutMs)]
    pub fn get_timeout_ms(&self) -> u32 {
        self.timeout_ms
    }

    /// Add a custom header
    #[wasm_bindgen(js_name = addHeader)]
    pub fn add_header(&mut self, key: String, value: String) {
        self.headers.insert(key, value);
    }

    /// Remove a custom header
    #[wasm_bindgen(js_name = removeHeader)]
    pub fn remove_header(&mut self, key: &str) {
        self.headers.remove(key);
    }

    /// Clear all custom headers
    #[wasm_bindgen(js_name = clearHeaders)]
    pub fn clear_headers(&mut self) {
        self.headers.clear();
    }

    // =========================================================================
    // API Accessors - return typed API objects
    // =========================================================================

    /// Get the System API
    #[wasm_bindgen]
    pub fn system(&self) -> system::SystemApi {
        system::SystemApi::from_client(self)
    }

    /// Get the Users API
    #[wasm_bindgen]
    pub fn users(&self) -> users::UsersApi {
        users::UsersApi::from_client(self)
    }

    /// Get the Billing API
    #[wasm_bindgen]
    pub fn billing(&self) -> billing::BillingApi {
        billing::BillingApi::from_client(self)
    }

    /// Get the Studios API
    #[wasm_bindgen]
    pub fn studios(&self) -> studios::StudiosApi {
        studios::StudiosApi::from_client(self)
    }

    /// Get the Devices API
    #[wasm_bindgen]
    pub fn devices(&self) -> devices::DevicesApi {
        devices::DevicesApi::from_client(self)
    }

    /// Get the Events API
    #[wasm_bindgen]
    pub fn events(&self) -> events::EventsApi {
        events::EventsApi::from_client(self)
    }

    /// Get the Streams API
    #[wasm_bindgen]
    pub fn streams(&self) -> streams::StreamsApi {
        streams::StreamsApi::from_client(self)
    }

    /// Get the Recordings API
    #[wasm_bindgen]
    pub fn recordings(&self) -> recordings::RecordingsApi {
        recordings::RecordingsApi::from_client(self)
    }
}

impl Default for ApiClient {
    fn default() -> Self {
        let http = reqwest::Client::builder()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            http,
            base_url: DEFAULT_BASE_URL.to_string(),
            bearer_token: None,
            user_agent: Some("jacktrip-web/1.0".to_string()),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            headers: HashMap::new(),
        }
    }
}

impl Clone for ApiClient {
    fn clone(&self) -> Self {
        Self {
            http: self.http.clone(),
            base_url: self.base_url.clone(),
            bearer_token: self.bearer_token.clone(),
            user_agent: self.user_agent.clone(),
            timeout_ms: self.timeout_ms,
            headers: self.headers.clone(),
        }
    }
}

// =============================================================================
// Internal HTTP methods on ApiClient
// =============================================================================

impl ApiClient {
    /// Build a request with common headers and authentication
    pub(crate) fn build_request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.base_url, path);
        let mut builder = self.http.request(method, &url);

        // Add user agent
        if let Some(ref user_agent) = self.user_agent {
            builder = builder.header(reqwest::header::USER_AGENT, user_agent);
        }

        // Add bearer token authentication
        if let Some(ref token) = self.bearer_token {
            builder = builder.bearer_auth(token);
        }

        // Add custom headers
        for (key, value) in &self.headers {
            builder = builder.header(key, value);
        }

        builder
    }

    /// Execute a GET request and deserialize the response
    pub(crate) async fn get<T: for<'de> Deserialize<'de>>(&self, path: &str) -> ApiResult<T> {
        let response = self
            .build_request(reqwest::Method::GET, path)
            .send()
            .await?;

        self.handle_response(response).await
    }

    /// Execute a GET request with query parameters
    pub(crate) async fn get_with_query<T: for<'de> Deserialize<'de>, Q: Serialize>(
        &self,
        path: &str,
        query: &Q,
    ) -> ApiResult<T> {
        let response = self
            .build_request(reqwest::Method::GET, path)
            .query(query)
            .send()
            .await?;

        self.handle_response(response).await
    }

    /// Execute a POST request with a JSON body
    pub(crate) async fn post<T: for<'de> Deserialize<'de>, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> ApiResult<T> {
        let response = self
            .build_request(reqwest::Method::POST, path)
            .json(body)
            .send()
            .await?;

        self.handle_response(response).await
    }

    /// Execute a POST request with a JSON body, returning nothing
    pub(crate) async fn post_no_response<B: Serialize>(&self, path: &str, body: &B) -> ApiResult<()> {
        let response = self
            .build_request(reqwest::Method::POST, path)
            .json(body)
            .send()
            .await?;

        self.handle_empty_response(response).await
    }

    /// Execute a POST request without a body, returning a typed response
    pub(crate) async fn post_empty<T: for<'de> Deserialize<'de>>(&self, path: &str) -> ApiResult<T> {
        let response = self
            .build_request(reqwest::Method::POST, path)
            .send()
            .await?;

        self.handle_response(response).await
    }

    /// Execute a POST request without a body, returning nothing
    pub(crate) async fn post_empty_no_response(&self, path: &str) -> ApiResult<()> {
        let response = self
            .build_request(reqwest::Method::POST, path)
            .send()
            .await?;

        self.handle_empty_response(response).await
    }

    /// Execute a PUT request with a JSON body
    pub(crate) async fn put<T: for<'de> Deserialize<'de>, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> ApiResult<T> {
        let response = self
            .build_request(reqwest::Method::PUT, path)
            .json(body)
            .send()
            .await?;

        self.handle_response(response).await
    }

    /// Execute a PUT request with a raw binary body (e.g. an image upload),
    /// returning nothing. Used by endpoints like `PUT /studios/{id}/banner`
    /// whose payload is image bytes rather than JSON.
    pub(crate) async fn put_bytes(
        &self,
        path: &str,
        body: Vec<u8>,
        content_type: &str,
    ) -> ApiResult<()> {
        let response = self
            .build_request(reqwest::Method::PUT, path)
            .header(reqwest::header::CONTENT_TYPE, content_type)
            .body(body)
            .send()
            .await?;

        self.handle_empty_response(response).await
    }

    /// Execute a DELETE request
    pub(crate) async fn delete(&self, path: &str) -> ApiResult<()> {
        let response = self
            .build_request(reqwest::Method::DELETE, path)
            .send()
            .await?;

        self.handle_empty_response(response).await
    }

    /// Execute a DELETE request and return the response body
    // Retained as a shared helper for downstream endpoint work (e.g. WEB-35).
    #[allow(dead_code)]
    pub(crate) async fn delete_with_response<T: for<'de> Deserialize<'de>>(&self, path: &str) -> ApiResult<T> {
        let response = self
            .build_request(reqwest::Method::DELETE, path)
            .send()
            .await?;

        self.handle_response(response).await
    }

    /// Execute a PUT request with query parameters (no body)
    pub(crate) async fn put_with_query<Q: Serialize>(&self, path: &str, query: &Q) -> ApiResult<()> {
        let response = self
            .build_request(reqwest::Method::PUT, path)
            .query(query)
            .send()
            .await?;

        self.handle_empty_response(response).await
    }

    /// Handle a response that should contain JSON
    async fn handle_response<T: for<'de> Deserialize<'de>>(
        &self,
        response: reqwest::Response,
    ) -> ApiResult<T> {
        let status = response.status();
        let body = response.text().await?;

        if status.is_success() {
            serde_json::from_str(&body).map_err(ApiError::from)
        } else {
            Err(ApiError::Http {
                status: status.as_u16(),
                message: status.canonical_reason().unwrap_or("Unknown error").to_string(),
                body: Some(body),
            })
        }
    }

    /// Handle a response that should be empty (204 No Content, etc.)
    async fn handle_empty_response(&self, response: reqwest::Response) -> ApiResult<()> {
        let status = response.status();

        if status.is_success() {
            Ok(())
        } else {
            let body = response.text().await.ok();
            Err(ApiError::Http {
                status: status.as_u16(),
                message: status.canonical_reason().unwrap_or("Unknown error").to_string(),
                body,
            })
        }
    }
}

// =============================================================================
// Error Types
// =============================================================================

/// API Error types
#[derive(Debug)]
pub enum ApiError {
    /// Network or request error
    Request(String),
    /// JSON serialization/deserialization error
    Serialization(String),
    /// HTTP error response from the server
    Http {
        status: u16,
        message: String,
        body: Option<String>,
    },
    /// Invalid configuration
    Config(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Request(msg) => write!(f, "Request error: {}", msg),
            ApiError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            ApiError::Http { status, message, .. } => {
                write!(f, "HTTP {} error: {}", status, message)
            }
            ApiError::Config(msg) => write!(f, "Configuration error: {}", msg),
        }
    }
}

impl std::error::Error for ApiError {}

impl From<reqwest::Error> for ApiError {
    fn from(err: reqwest::Error) -> Self {
        ApiError::Request(err.to_string())
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        ApiError::Serialization(err.to_string())
    }
}

impl From<ApiError> for JsValue {
    fn from(err: ApiError) -> Self {
        JsValue::from_str(&err.to_string())
    }
}

/// Result type for API operations
pub type ApiResult<T> = Result<T, ApiError>;

// =============================================================================
// Helper Functions
// =============================================================================

/// Pagination query parameters
#[derive(Serialize)]
pub(crate) struct PaginationQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<i32>,
}

/// Convert a value to JsValue using serde_wasm_bindgen
pub(crate) fn to_js_value<T: serde::Serialize>(val: &T) -> Result<JsValue, ApiError> {
    serde_wasm_bindgen::to_value(val).map_err(|e| ApiError::Serialization(e.to_string()))
}

/// URL encode a string for use in URL paths
pub(crate) fn urlencode<T: AsRef<str>>(s: T) -> String {
    url::form_urlencoded::byte_serialize(s.as_ref().as_bytes()).collect()
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use mockito;
    use serde::Deserialize;

    /// Minimal struct for JSON deserialization tests.
    #[derive(Debug, Deserialize, PartialEq)]
    struct SimpleResponse {
        value: String,
    }

    // =========================================================================
    // ApiError::Display
    // =========================================================================

    #[test]
    fn test_api_error_display_request() {
        let err = ApiError::Request("connection refused".to_string());
        assert_eq!(err.to_string(), "Request error: connection refused");
    }

    #[test]
    fn test_api_error_display_serialization() {
        let err = ApiError::Serialization("unexpected end of input".to_string());
        assert_eq!(err.to_string(), "Serialization error: unexpected end of input");
    }

    #[test]
    fn test_api_error_display_http() {
        let err = ApiError::Http {
            status: 404,
            message: "Not Found".to_string(),
            body: None,
        };
        assert_eq!(err.to_string(), "HTTP 404 error: Not Found");
    }

    #[test]
    fn test_api_error_display_http_body_not_included_in_display() {
        // The Display impl omits the body field — only status + message.
        let err = ApiError::Http {
            status: 500,
            message: "Internal Server Error".to_string(),
            body: Some("full error body text".to_string()),
        };
        assert_eq!(err.to_string(), "HTTP 500 error: Internal Server Error");
    }

    #[test]
    fn test_api_error_display_config() {
        let err = ApiError::Config("missing API key".to_string());
        assert_eq!(err.to_string(), "Configuration error: missing API key");
    }

    // =========================================================================
    // urlencode helper
    // =========================================================================

    #[test]
    fn test_urlencode_simple_string_unchanged() {
        assert_eq!(urlencode("hello"), "hello");
    }

    #[test]
    fn test_urlencode_empty_string() {
        assert_eq!(urlencode(""), "");
    }

    #[test]
    fn test_urlencode_space_becomes_plus() {
        // form_urlencoded encodes space as '+', not %20
        assert_eq!(urlencode("hello world"), "hello+world");
    }

    #[test]
    fn test_urlencode_reserved_slash_is_percent_encoded() {
        assert_eq!(urlencode("foo/bar"), "foo%2Fbar");
    }

    #[test]
    fn test_urlencode_unicode_is_percent_encoded_as_utf8() {
        // é = U+00E9 = UTF-8 bytes 0xC3 0xA9
        assert_eq!(urlencode("café"), "caf%C3%A9");
    }

    // =========================================================================
    // State accessors
    // =========================================================================

    #[test]
    fn test_default_base_url() {
        let client = ApiClient::new();
        assert_eq!(client.get_base_url(), DEFAULT_BASE_URL);
    }

    #[test]
    fn test_set_base_url_and_get_base_url() {
        let mut client = ApiClient::new();
        client.set_base_url("https://custom.example.com/api".to_string());
        assert_eq!(client.get_base_url(), "https://custom.example.com/api");
    }

    #[test]
    fn test_with_base_url_constructor() {
        let client = ApiClient::with_base_url("https://staging.example.com/api".to_string());
        assert_eq!(client.get_base_url(), "https://staging.example.com/api");
    }

    #[test]
    fn test_has_bearer_token_false_by_default() {
        let client = ApiClient::new();
        assert!(!client.has_bearer_token());
    }

    #[test]
    fn test_set_bearer_token_makes_has_bearer_token_true() {
        let mut client = ApiClient::new();
        client.set_bearer_token("my-token".to_string());
        assert!(client.has_bearer_token());
    }

    #[test]
    fn test_clear_bearer_token_makes_has_bearer_token_false() {
        let mut client = ApiClient::new();
        client.set_bearer_token("my-token".to_string());
        client.clear_bearer_token();
        assert!(!client.has_bearer_token());
    }

    // =========================================================================
    // Authorization header is actually sent when bearer token is set
    // =========================================================================

    #[tokio::test]
    async fn test_bearer_token_sent_as_authorization_header() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/test")
            .match_header("authorization", "Bearer secret-token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#""ok""#)
            .create_async()
            .await;

        let mut client = ApiClient::with_base_url(server.url());
        client.set_bearer_token("secret-token".to_string());
        let result: ApiResult<String> = client.get("/test").await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_no_bearer_token_means_no_authorization_header() {
        let mut server = mockito::Server::new_async().await;
        // Mockito will only match (and return 200) if no Authorization header is present.
        let mock = server
            .mock("GET", "/test")
            .match_header("authorization", mockito::Matcher::Missing)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#""ok""#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let result: ApiResult<String> = client.get("/test").await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    // =========================================================================
    // handle_response: typed success deserialization
    // =========================================================================

    #[tokio::test]
    async fn test_handle_response_deserializes_json_on_success() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/data")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"value":"hello"}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let result: ApiResult<SimpleResponse> = client.get("/data").await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), SimpleResponse { value: "hello".to_string() });
        mock.assert_async().await;
    }

    // =========================================================================
    // handle_response: non-2xx → ApiError::Http with correct status
    // =========================================================================

    #[tokio::test]
    async fn test_handle_response_404_maps_to_http_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/data")
            .with_status(404)
            .with_body("resource missing")
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let result: ApiResult<SimpleResponse> = client.get("/data").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Http { status, message, body } => {
                assert_eq!(status, 404);
                assert_eq!(message, "Not Found");
                assert_eq!(body, Some("resource missing".to_string()));
            }
            other => panic!("Expected ApiError::Http, got {:?}", other),
        }
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_handle_response_500_maps_to_http_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/data")
            .with_status(500)
            .with_body(r#"{"error":"internal"}"#)
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let result: ApiResult<SimpleResponse> = client.get("/data").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Http { status, .. } => assert_eq!(status, 500),
            other => panic!("Expected ApiError::Http, got {:?}", other),
        }
        mock.assert_async().await;
    }

    // =========================================================================
    // handle_response: malformed body on 2xx → ApiError::Serialization
    // =========================================================================

    #[tokio::test]
    async fn test_handle_response_malformed_json_gives_serialization_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/data")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("not valid json {{{{")
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let result: ApiResult<SimpleResponse> = client.get("/data").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Serialization(_) => {}
            other => panic!("Expected ApiError::Serialization, got {:?}", other),
        }
        mock.assert_async().await;
    }

    // =========================================================================
    // handle_empty_response: 2xx → Ok(())
    // =========================================================================

    #[tokio::test]
    async fn test_handle_empty_response_204_returns_ok() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("DELETE", "/item/42")
            .with_status(204)
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let result = client.delete("/item/42").await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_handle_empty_response_200_also_returns_ok() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("DELETE", "/item/42")
            .with_status(200)
            .with_body("deleted")
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let result = client.delete("/item/42").await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    // =========================================================================
    // handle_empty_response: error status → ApiError::Http
    // =========================================================================

    #[tokio::test]
    async fn test_handle_empty_response_403_gives_http_error() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("DELETE", "/item/42")
            .with_status(403)
            .with_body("Forbidden body")
            .create_async()
            .await;

        let client = ApiClient::with_base_url(server.url());
        let result = client.delete("/item/42").await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ApiError::Http { status, message, body } => {
                assert_eq!(status, 403);
                assert_eq!(message, "Forbidden");
                assert_eq!(body, Some("Forbidden body".to_string()));
            }
            other => panic!("Expected ApiError::Http, got {:?}", other),
        }
        mock.assert_async().await;
    }
}
