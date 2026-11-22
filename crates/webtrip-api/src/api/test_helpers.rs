//! Shared helpers for the native (`mockito`-based) API client tests.
//!
//! Every API module's `#[cfg(test)]` block exercises its endpoints the same
//! way: stand up a mock HTTP server, register a canned response for a
//! `method + path`, point an [`ApiClient`] at it, and assert on the decoded
//! result. These helpers hold that shared boilerplate in one place so the
//! per-module tests stay focused on the request/response contract rather than
//! re-rolling the server/client setup (see the "No Code Duplication" rule in
//! AGENTS.md).

use super::{ApiClient, ApiError};
use mockito::{Matcher, Mock, ServerGuard};

/// Start a mock HTTP server and an [`ApiClient`] already pointed at it.
///
/// Returns the guard (which must stay alive for the duration of the test) and
/// the client. Register expectations with [`mock_json`] / [`mock_empty`].
pub(crate) async fn mock_api() -> (ServerGuard, ApiClient) {
    let server = mockito::Server::new_async().await;
    let client = ApiClient::with_base_url(server.url());
    (server, client)
}

/// Register a JSON response for `method path`, returning the [`Mock`] so the
/// caller can `.assert_async().await` that the endpoint was hit.
///
/// `path` accepts anything convertible into a [`Matcher`] — a `&str`/`String`
/// for an exact path, or an explicit matcher (e.g. [`Matcher::Regex`]) for
/// paths with encoded segments. The query string is ignored so callers can
/// match on path alone regardless of serialized query parameters.
pub(crate) async fn mock_json<P: Into<Matcher>>(
    server: &mut ServerGuard,
    method: &str,
    path: P,
    status: usize,
    body: &str,
) -> Mock {
    server
        .mock(method, path)
        .match_query(Matcher::Any)
        .with_status(status)
        .with_header("content-type", "application/json")
        .with_body(body)
        .create_async()
        .await
}

/// Register a bodyless response for `method path` (e.g. `204 No Content` for
/// follow/unfollow/delete style endpoints), returning the [`Mock`].
pub(crate) async fn mock_empty<P: Into<Matcher>>(
    server: &mut ServerGuard,
    method: &str,
    path: P,
    status: usize,
) -> Mock {
    server
        .mock(method, path)
        .match_query(Matcher::Any)
        .with_status(status)
        .create_async()
        .await
}

/// Assert that `err` is an [`ApiError::Http`] carrying the expected status code.
pub(crate) fn assert_http_status(err: ApiError, expected: u16) {
    match err {
        ApiError::Http { status, .. } => assert_eq!(status, expected),
        other => panic!("expected HTTP {expected} error, got {other:?}"),
    }
}
