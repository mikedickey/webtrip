//! Shared support for browser (`wasm-bindgen-test`) tests in the library crate.
//!
//! ## Convention for browser tests
//!
//! Browser tests live **inline** in each module's `#[cfg(test)] mod tests`
//! block, gated on `#[cfg(target_arch = "wasm32")]`, and annotated with
//! `#[wasm_bindgen_test]`. They run in headless Chrome via `npm run test:wasm`.
//!
//! `wasm_bindgen_test_configure!(run_in_browser)` must appear **exactly once
//! per test binary** or every test silently falls back to Node and is skipped.
//! For the library's unit-test binary that single invocation lives **here**, so
//! individual modules (`ring_buffer`, `webrtc`, `engine`, …) must NOT repeat it
//! — they only add `#[wasm_bindgen_test]` functions and reuse the helpers below.
//!
//! Standalone integration test files under `tests/*.rs` are separate binaries
//! and therefore each need their own `wasm_bindgen_test_configure!` call.

use wasm_bindgen_test::wasm_bindgen_test_configure;

wasm_bindgen_test_configure!(run_in_browser);

/// Yield for roughly `ms` milliseconds inside a browser test.
///
/// Races a `setTimeout`-backed promise so an `async` test can hand control
/// back to the event loop, letting queued microtasks and `Atomics.waitAsync`
/// continuations run before subsequent assertions. Shared so any async browser
/// test that must wait for a callback (the audio-callback loop today, more
/// later) uses one implementation rather than re-rolling the timer dance.
pub(crate) async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _reject| {
        web_sys::window()
            .expect("window must exist in a browser test")
            .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms)
            .expect("setTimeout should schedule the resolver");
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

/// Assert that an SDP blob is well-formed.
///
/// A minimally valid session description is non-empty and contains the
/// mandatory `v=` (protocol version) and `m=` (media) lines per RFC 4566.
/// Shared so any transport/browser test (WebRTC today, more later) can reuse
/// the same notion of "well-formed SDP" rather than re-checking ad hoc.
pub(crate) fn assert_valid_sdp(sdp: &str) {
    assert!(!sdp.is_empty(), "SDP must not be empty");
    assert!(
        sdp.lines().any(|line| line.starts_with("v=")),
        "SDP must contain a v= (version) line, got:\n{sdp}"
    );
    assert!(
        sdp.lines().any(|line| line.starts_with("m=")),
        "SDP must contain an m= (media) line, got:\n{sdp}"
    );
}
