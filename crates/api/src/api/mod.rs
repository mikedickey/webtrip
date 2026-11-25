//! Client for the JackTrip Virtual Studio REST API.
//!
//! [`ApiClient`] holds the configuration (base URL, bearer token, user agent,
//! timeout, custom headers) and a pooled HTTP client. Its accessor methods hand out one
//! typed API object per endpoint group; every endpoint is an `async fn`
//! returning [`ApiResult`]. The same surface is exported to JavaScript through
//! `wasm-bindgen` when the `wasm` feature is on (the `webtrip` npm package),
//! with camelCase method names and TypeScript types generated for the
//! [`models`].
//!
//! | Accessor | Module | Covers |
//! |----------|--------|--------|
//! | [`ApiClient::system`] | [`system`] | Health checks, regions, redirects, analytics |
//! | [`ApiClient::users`] | [`users`] | User profiles, notifications, referrals |
//! | [`ApiClient::studios`] | [`studios`] | Virtual studio management |
//! | [`ApiClient::devices`] | [`devices`] | JackTrip hardware devices |
//! | [`ApiClient::events`] | [`events`] | Upcoming broadcasts and events |
//! | [`ApiClient::streams`] | [`streams`] | Live streams, channels, and conversations |
//! | [`ApiClient::recordings`] | [`recordings`] | Recorded content |
//! | [`ApiClient::billing`] | [`billing`] | Stripe billing, plans, redemptions, usage |
//! | [`ApiClient::subscriptions`] | [`subscriptions`] | Studio memberships (not billing) |
//! | [`ApiClient::tracks`] | [`tracks`] | Backing tracks for a studio |
//!
//! # Authentication
//!
//! Most endpoints require a JWT, sent as `Authorization: Bearer <token>` once
//! [`ApiClient::set_bearer_token`] has been called. A few (e.g.
//! [`system::SystemApi::ping`]) work without one. With no token set, no
//! `Authorization` header is sent; in the browser the underlying `fetch` then
//! carries the page's cookies only when the API is same-origin with the page,
//! so a cross-origin host needs an explicit token.
//!
//! # Quick start (Rust)
//!
//! ```rust,no_run
//! use webtrip_api::api::{ApiClient, ApiError};
//!
//! # async fn run() -> Result<(), ApiError> {
//! let client = ApiClient::with_base_url("https://test.jacktrip.com/api".to_string());
//!
//! // Unauthenticated
//! let ping = client.system().ping().await?;
//! println!("API version: {:?}", ping.version);
//!
//! // Authenticated
//! client.set_bearer_token("your-jwt-token".to_string());
//! let user = client.users().get_current_user().await?;
//! let studios = client.studios().list_studios().await?;
//! println!("{:?} has {} studios", user.name, studios.len());
//! # Ok(())
//! # }
//! ```
//!
//! # Quick start (JavaScript)
//!
//! ```js
//! import init, { ApiClient } from 'webtrip';
//!
//! await init(); // once, before using any export
//!
//! const client = ApiClient.withBaseUrl('https://test.jacktrip.com/api');
//! const ping = await client.system().ping();
//!
//! client.setBearerToken('your-jwt-token');
//! const user = await client.users().getCurrentUser();
//! const studios = await client.studios().listStudios();
//! ```
//!
//! # Errors
//!
//! Rust callers get an [`ApiError`] and can match on its variants. In
//! JavaScript the returned promise rejects with the error's [`Display`](fmt::Display)
//! string (e.g. `"HTTP 401 error: Unauthorized"`), so callers distinguish cases
//! by prefix:
//!
//! ```js
//! try {
//!   await client.users().getCurrentUser();
//! } catch (error) {
//!   if (error.startsWith('HTTP 401')) {
//!     // token missing or expired
//!   } else if (error.startsWith('Request error')) {
//!     // network failure
//!   }
//! }
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

// Macro to generate API module struct + constructors boilerplate
macro_rules! api_module_struct {
    ($name:ident) => {
        #[cfg_attr(feature = "wasm", wasm_bindgen)]
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

        #[cfg_attr(feature = "wasm", wasm_bindgen)]
        impl $name {
            #[cfg_attr(feature = "wasm", wasm_bindgen(constructor))]
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
pub mod subscriptions;
pub mod system;
pub mod tracks;
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

/// Default `User-Agent`: `webtrip/<crate version>`.
pub const DEFAULT_USER_AGENT: &str = concat!("webtrip/", env!("CARGO_PKG_VERSION"));

/// Entry point for the JackTrip Virtual Studio API.
///
/// Holds the configuration applied to every request and a pooled HTTP client.
/// The accessors ([`system`](Self::system), [`users`](Self::users), ...) return
/// API objects that share this client's configuration and connection pool, so
/// they are cheap to create and always see its current settings: a bearer
/// token set after an API object was obtained applies to that object's next
/// request. Cloning an `ApiClient` likewise yields another handle to the same
/// configuration.
///
/// The configuration sits behind a lock, so a client can be shared across
/// threads and reconfigured from any of them; a request reads the settings
/// once, when it is built.
///
/// | Setting | Default | Rust | JavaScript |
/// |---------|---------|------|------------|
/// | Base URL | [`DEFAULT_BASE_URL`] | [`set_base_url`](Self::set_base_url) / [`get_base_url`](Self::get_base_url) | `setBaseUrl` / `getBaseUrl` |
/// | Bearer token | none | [`set_bearer_token`](Self::set_bearer_token) / [`clear_bearer_token`](Self::clear_bearer_token) / [`has_bearer_token`](Self::has_bearer_token) | `setBearerToken` / `clearBearerToken` / `hasBearerToken` |
/// | User agent | [`DEFAULT_USER_AGENT`] | [`set_user_agent`](Self::set_user_agent) | `setUserAgent` |
/// | Timeout | [`DEFAULT_TIMEOUT_MS`] | [`set_timeout_ms`](Self::set_timeout_ms) / [`get_timeout_ms`](Self::get_timeout_ms) | `setTimeoutMs` / `getTimeoutMs` |
/// | Extra headers | none | [`add_header`](Self::add_header) / [`remove_header`](Self::remove_header) / [`clear_headers`](Self::clear_headers) | `addHeader` / `removeHeader` / `clearHeaders` |
///
/// # Example
///
/// ```rust,no_run
/// use webtrip_api::api::{ApiClient, ApiError};
///
/// # async fn run() -> Result<(), ApiError> {
/// let client = ApiClient::new();
/// client.set_bearer_token("your-jwt-token".to_string());
/// client.add_header("X-Request-Id".to_string(), "abc123".to_string());
/// let user = client.users().get_current_user().await?;
/// # Ok(())
/// # }
/// ```
///
/// ```js
/// const client = new ApiClient();
/// client.setBearerToken('your-jwt-token');
/// client.addHeader('X-Request-Id', 'abc123');
/// const user = await client.users().getCurrentUser();
/// ```
#[cfg_attr(feature = "wasm", wasm_bindgen)]
#[derive(Debug, Clone)]
pub struct ApiClient {
    http: reqwest::Client,
    config: Arc<RwLock<ClientConfig>>,
}

/// The settings every request is built from, shared by an [`ApiClient`], its
/// clones and the API objects obtained from it.
#[derive(Debug, Clone)]
struct ClientConfig {
    base_url: String,
    bearer_token: Option<String>,
    user_agent: Option<String>,
    timeout_ms: u32,
    headers: HashMap<String, String>,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            bearer_token: None,
            user_agent: Some(DEFAULT_USER_AGENT.to_string()),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            headers: HashMap::new(),
        }
    }
}

#[cfg_attr(feature = "wasm", wasm_bindgen)]
impl ApiClient {
    /// Create a client pointed at [`DEFAULT_BASE_URL`] with no bearer token.
    #[cfg_attr(feature = "wasm", wasm_bindgen(constructor))]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a client for a different API host, e.g.
    /// `https://test.jacktrip.com/api`.
    ///
    /// Endpoint paths are appended verbatim, so pass the URL without a
    /// trailing slash.
    #[cfg_attr(feature = "wasm", wasm_bindgen(js_name = withBaseUrl))]
    pub fn with_base_url(base_url: String) -> Self {
        let client = Self::default();
        client.set_base_url(base_url);
        client
    }

    /// Set the base URL (no trailing slash; see [`with_base_url`](Self::with_base_url)).
    #[cfg_attr(feature = "wasm", wasm_bindgen(js_name = setBaseUrl))]
    pub fn set_base_url(&self, base_url: String) {
        self.config_mut().base_url = base_url;
    }

    /// Get the base URL
    #[cfg_attr(feature = "wasm", wasm_bindgen(js_name = getBaseUrl))]
    pub fn get_base_url(&self) -> String {
        self.config().base_url.clone()
    }

    /// Set the JWT sent as `Authorization: Bearer <token>` on every request.
    #[cfg_attr(feature = "wasm", wasm_bindgen(js_name = setBearerToken))]
    pub fn set_bearer_token(&self, token: String) {
        self.config_mut().bearer_token = Some(token);
    }

    /// Clear the bearer token; subsequent requests carry no `Authorization` header.
    #[cfg_attr(feature = "wasm", wasm_bindgen(js_name = clearBearerToken))]
    pub fn clear_bearer_token(&self) {
        self.config_mut().bearer_token = None;
    }

    /// Check if a bearer token is set
    #[cfg_attr(feature = "wasm", wasm_bindgen(js_name = hasBearerToken))]
    pub fn has_bearer_token(&self) -> bool {
        self.config().bearer_token.is_some()
    }

    /// Set the `User-Agent` header (default [`DEFAULT_USER_AGENT`]).
    ///
    /// Some browsers (notably Chromium) silently drop a `User-Agent` set on a
    /// `fetch`, so in the browser this may have no effect.
    #[cfg_attr(feature = "wasm", wasm_bindgen(js_name = setUserAgent))]
    pub fn set_user_agent(&self, user_agent: String) {
        self.config_mut().user_agent = Some(user_agent);
    }

    /// Set the request timeout in milliseconds (default [`DEFAULT_TIMEOUT_MS`]).
    ///
    /// It bounds a whole request, from sending it to reading the response
    /// body; a request that runs out fails with [`ApiError::Request`]. `0`
    /// disables the timeout.
    #[cfg_attr(feature = "wasm", wasm_bindgen(js_name = setTimeoutMs))]
    pub fn set_timeout_ms(&self, timeout_ms: u32) {
        self.config_mut().timeout_ms = timeout_ms;
    }

    /// Get the request timeout in milliseconds (`0`: no timeout)
    #[cfg_attr(feature = "wasm", wasm_bindgen(js_name = getTimeoutMs))]
    pub fn get_timeout_ms(&self) -> u32 {
        self.config().timeout_ms
    }

    /// Add a header sent on every request, replacing any earlier value for `key`.
    #[cfg_attr(feature = "wasm", wasm_bindgen(js_name = addHeader))]
    pub fn add_header(&self, key: String, value: String) {
        self.config_mut().headers.insert(key, value);
    }

    /// Remove a custom header
    #[cfg_attr(feature = "wasm", wasm_bindgen(js_name = removeHeader))]
    pub fn remove_header(&self, key: &str) {
        self.config_mut().headers.remove(key);
    }

    /// Clear all custom headers
    #[cfg_attr(feature = "wasm", wasm_bindgen(js_name = clearHeaders))]
    pub fn clear_headers(&self) {
        self.config_mut().headers.clear();
    }

    // =========================================================================
    // API Accessors - return typed API objects
    // =========================================================================

    /// Get the System API
    #[cfg_attr(feature = "wasm", wasm_bindgen)]
    pub fn system(&self) -> system::SystemApi {
        system::SystemApi::from_client(self)
    }

    /// Get the Users API
    #[cfg_attr(feature = "wasm", wasm_bindgen)]
    pub fn users(&self) -> users::UsersApi {
        users::UsersApi::from_client(self)
    }

    /// Get the Billing API
    #[cfg_attr(feature = "wasm", wasm_bindgen)]
    pub fn billing(&self) -> billing::BillingApi {
        billing::BillingApi::from_client(self)
    }

    /// Get the Studios API
    #[cfg_attr(feature = "wasm", wasm_bindgen)]
    pub fn studios(&self) -> studios::StudiosApi {
        studios::StudiosApi::from_client(self)
    }

    /// Get the Devices API
    #[cfg_attr(feature = "wasm", wasm_bindgen)]
    pub fn devices(&self) -> devices::DevicesApi {
        devices::DevicesApi::from_client(self)
    }

    /// Get the Events API
    #[cfg_attr(feature = "wasm", wasm_bindgen)]
    pub fn events(&self) -> events::EventsApi {
        events::EventsApi::from_client(self)
    }

    /// Get the Streams API
    #[cfg_attr(feature = "wasm", wasm_bindgen)]
    pub fn streams(&self) -> streams::StreamsApi {
        streams::StreamsApi::from_client(self)
    }

    /// Get the Recordings API
    #[cfg_attr(feature = "wasm", wasm_bindgen)]
    pub fn recordings(&self) -> recordings::RecordingsApi {
        recordings::RecordingsApi::from_client(self)
    }

    /// Get the Subscriptions (studio memberships) API
    #[cfg_attr(feature = "wasm", wasm_bindgen)]
    pub fn subscriptions(&self) -> subscriptions::SubscriptionsApi {
        subscriptions::SubscriptionsApi::from_client(self)
    }

    /// Get the Backing Tracks API
    #[cfg_attr(feature = "wasm", wasm_bindgen)]
    pub fn tracks(&self) -> tracks::TracksApi {
        tracks::TracksApi::from_client(self)
    }
}

impl Default for ApiClient {
    fn default() -> Self {
        let http = reqwest::Client::builder()
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            http,
            config: Arc::new(RwLock::new(ClientConfig::default())),
        }
    }
}

// =============================================================================
// Internal HTTP methods on ApiClient
// =============================================================================

impl ApiClient {
    // A writer that panicked mid-update can only have left a field
    // half-assigned by a plain store, which is still a valid value, so a
    // poisoned lock is recovered rather than propagated.
    fn config(&self) -> RwLockReadGuard<'_, ClientConfig> {
        self.config.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn config_mut(&self) -> RwLockWriteGuard<'_, ClientConfig> {
        self.config.write().unwrap_or_else(PoisonError::into_inner)
    }

    /// Build a request with common headers and authentication
    pub(crate) fn build_request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let config = self.config();
        let url = format!("{}{}", config.base_url, path);
        let mut builder = self.http.request(method, &url);

        if config.timeout_ms > 0 {
            builder = builder.timeout(Duration::from_millis(u64::from(config.timeout_ms)));
        }

        // Add user agent
        if let Some(ref user_agent) = config.user_agent {
            builder = builder.header(reqwest::header::USER_AGENT, user_agent);
        }

        // Add bearer token authentication
        if let Some(ref token) = config.bearer_token {
            builder = builder.bearer_auth(token);
        }

        // Add custom headers
        for (key, value) in &config.headers {
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

    /// Execute a POST request with a `multipart/form-data` body (e.g. file uploads)
    pub(crate) async fn post_multipart<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> ApiResult<T> {
        let response = self
            .build_request(reqwest::Method::POST, path)
            .multipart(form)
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

/// Error returned by every API call.
///
/// In JavaScript the promise rejects with this error's [`Display`](fmt::Display)
/// string, so the variant is recoverable from its prefix: `Request error:`,
/// `Serialization error:`, `HTTP <status> error:`.
///
/// ```rust,no_run
/// use webtrip_api::api::{ApiClient, ApiError};
///
/// # async fn run(client: &ApiClient) {
/// match client.users().get_user("some-id").await {
///     Ok(user) => println!("{:?}", user.name),
///     Err(ApiError::Http { status: 404, .. }) => println!("no such user"),
///     Err(ApiError::Http { status: 401, .. }) => println!("token missing or expired"),
///     Err(ApiError::Http { status, body, .. }) => println!("HTTP {status}: {body:?}"),
///     Err(e) => println!("{e}"),
/// }
/// # }
/// ```
#[derive(Debug)]
pub enum ApiError {
    /// The request never produced a response: connection refused, DNS
    /// failure, TLS error, or the body could not be read.
    Request(String),
    /// A 2xx response whose body did not deserialize into the expected type
    /// (malformed JSON or a schema mismatch), or a value that could not be
    /// converted to/from JavaScript.
    Serialization(String),
    /// The server answered with a non-2xx status.
    Http {
        /// HTTP status code, e.g. 401 (missing/invalid token), 403
        /// (insufficient permissions), 404, 429 (rate limited).
        status: u16,
        /// Canonical reason phrase for `status` (e.g. `"Not Found"`), not a
        /// server-supplied message.
        message: String,
        /// Raw response body, which usually carries the server's error detail.
        /// Not included in the [`Display`](fmt::Display) output.
        body: Option<String>,
    },
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::Request(msg) => write!(f, "Request error: {}", msg),
            ApiError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            ApiError::Http { status, message, .. } => {
                write!(f, "HTTP {} error: {}", status, message)
            }
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

#[cfg(feature = "wasm")]
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

#[cfg(feature = "wasm")]
/// Convert a value to JsValue using serde_wasm_bindgen
pub(crate) fn to_js_value<T: serde::Serialize>(val: &T) -> Result<JsValue, ApiError> {
    // Maps (e.g. region id → region) become plain objects, not `Map`s, so JS
    // reads them like every other JSON-shaped result.
    let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
    val.serialize(&serializer).map_err(|e| ApiError::Serialization(e.to_string()))
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
    fn test_api_error_display_http_body_not_included_in_display() {
        // The Display impl omits the body field — only status + message.
        let err = ApiError::Http {
            status: 500,
            message: "Internal Server Error".to_string(),
            body: Some("full error body text".to_string()),
        };
        assert_eq!(err.to_string(), "HTTP 500 error: Internal Server Error");
    }

    // =========================================================================
    // urlencode helper
    // =========================================================================

    #[test]
    fn test_urlencode_space_becomes_plus() {
        // form_urlencoded encodes space as '+', not %20
        assert_eq!(urlencode("hello world"), "hello+world");
    }

    #[test]
    fn test_urlencode_reserved_slash_is_percent_encoded() {
        assert_eq!(urlencode("foo/bar"), "foo%2Fbar");
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

        let client = ApiClient::with_base_url(server.url());
        // Regression: API objects (each holds a clone of the client) used to
        // snapshot its configuration, so a token set after one was obtained
        // never reached it. A clone taken *before* the token is set must send it.
        let obtained_earlier = client.clone();
        client.set_bearer_token("secret-token".to_string());
        let result: ApiResult<String> = obtained_earlier.get("/test").await;

        assert!(result.is_ok());
        mock.assert_async().await;
    }

    /// A localhost listener that accepts connections and never answers, kept
    /// open for the rest of the test process.
    fn unresponsive_server() -> std::net::SocketAddr {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept() {
                held.push(stream);
            }
        });
        addr
    }

    #[tokio::test]
    async fn test_timeout_bounds_a_request_to_an_unresponsive_server() {
        // Regression: `set_timeout_ms` was stored but never applied, so this
        // request waited forever.
        let client = ApiClient::with_base_url(format!("http://{}", unresponsive_server()));
        client.set_timeout_ms(200);

        let outcome = tokio::time::timeout(Duration::from_secs(10), client.get::<String>("/slow")).await;
        let err = outcome.expect("request outlived its 200 ms timeout").unwrap_err();
        assert!(matches!(err, ApiError::Request(_)), "{err:?}");
    }

    #[tokio::test]
    async fn test_https_request_starts_a_tls_handshake() {
        use std::io::Read;
        // Regression: reqwest was built with no TLS backend, so a native
        // `https://` request failed before connecting at all. Accept one
        // connection (giving up after a deadline rather than hanging) and
        // check that the client opens with a TLS handshake record.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let first_bytes = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        let mut header = [0u8; 2];
                        stream.read_exact(&mut header).ok()?;
                        return Some(header);
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
            None
        });

        let client = ApiClient::with_base_url(format!("https://{addr}"));
        client.set_timeout_ms(5_000);
        // Fails either way: the server never completes the handshake.
        let _ = client.get::<String>("/x").await;

        // 0x16 = handshake record, 0x03 = TLS major version.
        assert_eq!(first_bytes.join().unwrap(), Some([0x16, 0x03]));
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
