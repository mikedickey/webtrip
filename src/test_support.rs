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

use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test_configure;

wasm_bindgen_test_configure!(run_in_browser);

/// Construct a synthetic `MessageEvent` carrying `data` and dispatch it to
/// `target`, which invokes the target's registered `onmessage` handler
/// synchronously.
///
/// Shared by the WebSocket signaling tests (`HubSignaling`) and the
/// WebTransport main-thread worker-message tests so the message-event
/// construction/dispatch isn't re-implemented per module (per AGENTS.md). Pass
/// a `EventTarget` reference (e.g. `ws.as_ref()` / `worker.as_ref()`); `data`
/// may be any `JsValue` (a string or a plain object).
pub(crate) fn dispatch_message_event(target: &web_sys::EventTarget, data: &JsValue) {
    let init = web_sys::MessageEventInit::new();
    init.set_data(data);
    let event = web_sys::MessageEvent::new_with_event_init_dict("message", &init)
        .expect("MessageEvent construction should succeed");
    target
        .dispatch_event(&event)
        .expect("dispatching the message event should succeed");
}

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

/// Poll `predicate` until it returns `true` or `timeout_ms` elapses, yielding
/// to the event loop for `interval_ms` between checks.
///
/// Returns `true` if the predicate was satisfied within the budget, `false` on
/// timeout. Lets an async browser test wait on an *actual* state transition
/// (e.g. a session returning to `Idle` after teardown) instead of assuming a
/// fixed delay. Shared so timing-dependent `sleep_ms`-then-assert patterns can
/// be replaced with one bounded-wait implementation rather than re-rolling the
/// loop each time.
///
/// A non-positive `interval_ms` is clamped to 1 ms so `elapsed` always advances
/// toward `timeout_ms`; otherwise the loop could spin without ever timing out.
pub(crate) async fn wait_until<F: FnMut() -> bool>(
    mut predicate: F,
    timeout_ms: i32,
    interval_ms: i32,
) -> bool {
    let interval_ms = interval_ms.max(1);
    let mut elapsed = 0;
    loop {
        if predicate() {
            return true;
        }
        if elapsed >= timeout_ms {
            return false;
        }
        sleep_ms(interval_ms).await;
        elapsed += interval_ms;
    }
}

/// Build an `on_state_change` callback that appends each emitted state string
/// to a shared log.
///
/// Returns the shared log plus the live `Closure`, which the caller must keep
/// alive for as long as the producer may fire the callback (dropping it
/// invalidates the `js_sys::Function` derived from it). Shared so any browser
/// test that needs to observe a sequence of state-change strings (session
/// connect/disconnect lifecycle, transport state surface, …) reuses one
/// implementation rather than re-rolling the closure-plus-log dance.
pub(crate) fn recording_state_callback() -> (
    std::rc::Rc<std::cell::RefCell<Vec<String>>>,
    wasm_bindgen::closure::Closure<dyn FnMut(String)>,
) {
    let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log_for_cb = log.clone();
    let closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |state: String| {
        log_for_cb.borrow_mut().push(state);
    }) as Box<dyn FnMut(String)>);
    (log, closure)
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
