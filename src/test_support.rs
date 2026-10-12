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

// Calls to session `start_engine` observed by browser tests. Distinguishes a
// post-dial cancel (engine never starts) from a cancel at `finish_connect`
// (engine was created then released).
std::thread_local! {
    static START_ENGINE_CALLS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Record that `start_engine` was entered (session tests only).
pub(crate) fn note_start_engine() {
    START_ENGINE_CALLS.with(|c| c.set(c.get().saturating_add(1)));
}

/// Take and reset the [`note_start_engine`] counter.
pub(crate) fn take_start_engine_calls() -> u32 {
    START_ENGINE_CALLS.with(|c| c.replace(0))
}

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

/// What a test can observe of a [`MockTransport`] after handing it to the
/// session (which takes ownership of the transport itself).
#[derive(Default)]
pub(crate) struct MockProbe {
    /// Calls to `connect`
    pub(crate) connects: std::cell::Cell<u32>,
    /// Calls to `close`
    pub(crate) closes: std::cell::Cell<u32>,
    /// Calls to `tick` (the audio-callback network loop)
    pub(crate) ticks: std::cell::Cell<u32>,
    /// The state-change callback the session installed
    on_state_change: std::cell::RefCell<Option<js_sys::Function>>,
    /// Run once, just before a `connect` resolves (after any gate opens)
    on_connected: std::cell::RefCell<Option<Box<dyn FnOnce()>>>,
}

impl MockProbe {
    /// Report a transport-level state string to the session, as a real
    /// transport's event handlers would.
    pub(crate) fn report(&self, state: &str) {
        let callback = self.on_state_change.borrow().clone();
        let callback = callback.expect("the session installs a state callback before connecting");
        let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(state));
    }

    /// Run `hook` just before the next `connect` resolves.
    pub(crate) fn on_connected(&self, hook: impl FnOnce() + 'static) {
        *self.on_connected.borrow_mut() = Some(Box::new(hook));
    }
}

/// A promise the test opens by calling the returned function, for holding a
/// [`MockTransport`] operation pending.
pub(crate) fn gate() -> (js_sys::Promise, js_sys::Function) {
    let (promise, open, _reject) = crate::audio::make_promise();
    (promise, open)
}

/// Minimal in-process [`Transport`] for driving the session's connect/disconnect
/// lifecycle in unit tests without a live hub.
///
/// It performs no network I/O and touches no browser APIs: `connect`
/// transitions to `Connected`, `close` to `Closed`, and `tick` discards
/// whatever the worklet has queued for sending. Either operation can be held
/// pending on a [`gate`], and [`probe`](Self::probe) observes it after the
/// session takes ownership. Pass it to
/// [`WebTripSession::connect_with_test_transport`] so a test can exercise the
/// real connect → `AudioEngine` → `disconnect` path.
///
/// [`WebTripSession::connect_with_test_transport`]: crate::session::WebTripSession::connect_with_test_transport
pub(crate) struct MockTransport {
    state: crate::audio::transport::TransportState,
    /// The session's buffers, from `set_audio_buffers`.
    audio_buffers: Option<crate::audio::transport::AudioBufferConfig>,
    /// One packet of scratch for draining the send ring buffer.
    drain_scratch: Vec<f32>,
    probe: std::rc::Rc<MockProbe>,
    connect_gate: Option<js_sys::Promise>,
    close_gate: Option<js_sys::Promise>,
}

impl MockTransport {
    pub(crate) fn new() -> Self {
        Self {
            state: crate::audio::transport::TransportState::Disconnected,
            audio_buffers: None,
            drain_scratch: Vec::new(),
            probe: Default::default(),
            connect_gate: None,
            close_gate: None,
        }
    }

    pub(crate) fn probe(&self) -> std::rc::Rc<MockProbe> {
        self.probe.clone()
    }

    /// Hold `connect` pending until `gate` resolves.
    pub(crate) fn with_connect_gate(mut self, gate: js_sys::Promise) -> Self {
        self.connect_gate = Some(gate);
        self
    }

    /// Hold `close` pending until `gate` resolves.
    pub(crate) fn with_close_gate(mut self, gate: js_sys::Promise) -> Self {
        self.close_gate = Some(gate);
        self
    }
}

/// Await `gate`, if any.
async fn pass(gate: Option<js_sys::Promise>) {
    if let Some(gate) = gate {
        let _ = wasm_bindgen_futures::JsFuture::from(gate).await;
    }
}

impl crate::audio::transport::Transport for MockTransport {
    fn transport_type(&self) -> crate::audio::transport::TransportType {
        // The concrete type is irrelevant to the lifecycle under test; report
        // the default so no production `TransportType::Mock` variant is needed.
        crate::audio::transport::TransportType::WebRTC
    }

    fn set_audio_buffers(&mut self, config: crate::audio::transport::AudioBufferConfig) {
        self.drain_scratch.resize(config.samples_per_packet(), 0.0);
        self.audio_buffers = Some(config);
    }

    fn set_on_state_change(&mut self, callback: js_sys::Function) {
        *self.probe.on_state_change.borrow_mut() = Some(callback);
    }

    /// Drain the send ring buffer, as `WebRtcTransport` does before its data
    /// channel opens, so the has-data flag clears and the callback loop's
    /// `Atomics.waitAsync` can sleep. A no-op tick left the flag at 1 once the
    /// worklet wrote: `waitAsync` then returned synchronously forever, the loop
    /// never yielded, and the renderer hung (the intermittent CI
    /// `coverage:wasm` "Timed out receiving message from renderer").
    fn tick(&mut self) {
        self.probe.ticks.set(self.probe.ticks.get() + 1);
        let ring = self.audio_buffers.and_then(|buffers| buffers.local_to_network.as_ref());
        if let Some(ring) = ring {
            while ring.read(&mut self.drain_scratch) {}
        }
    }

    fn state(&self) -> crate::audio::transport::TransportState {
        self.state
    }

    fn connect(
        &mut self,
        _server: &str,
        _port: u16,
        _client_name: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), JsValue>> + '_>> {
        self.probe.connects.set(self.probe.connects.get() + 1);
        let gate = self.connect_gate.take();
        Box::pin(async move {
            pass(gate).await;
            self.state = crate::audio::transport::TransportState::Connected;
            let hook = self.probe.on_connected.borrow_mut().take();
            if let Some(hook) = hook {
                hook();
            }
            Ok(())
        })
    }

    fn close(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + '_>> {
        self.probe.closes.set(self.probe.closes.get() + 1);
        self.state = crate::audio::transport::TransportState::Closed;
        Box::pin(pass(self.close_gate.take()))
    }
}
