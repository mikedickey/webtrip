//! WebTransport Implementation
//!
//! This module provides a WebTransport-based transport for JackTrip audio streaming.
//! WebTransport offers significant advantages over WebRTC data channels:
//!
//! - **Worker thread support**: Network I/O runs in a dedicated Web Worker, freeing the main thread
//! - **Event-driven**: Uses QUIC's async datagram API (no polling required)
//! - **Lower latency**: QUIC avoids head-of-line blocking that SCTP can experience
//! - **Simpler setup**: No SDP negotiation or ICE candidates needed
//!
//! ## Architecture
//!
//! ```text
//! Main Thread                           Worker Thread
//! ┌──────────────────┐                  ┌────────────────────────┐
//! │ WebTransportImpl │ ──postMessage──> │ webtransport_worker.rs │
//! │   (Transport)    │                  │   - send_loop()        │
//! │                  │ <──postMessage── │   - receive_loop()     │
//! └──────────────────┘                  └────────────────────────┘
//!          │                                       │
//!          │        SharedArrayBuffer              │
//!          ▼                                       ▼
//!    ┌─────────────────────────────────────────────────┐
//!    │  RingBuffer (send)    │    Regulator (receive)  │
//!    └─────────────────────────────────────────────────┘
//! ```
//!
//! ## Browser Support
//!
//! - Chrome 97+ / Edge 97+: Full support
//! - Safari / Firefox: Not yet supported (use WebRTC fallback)

use crate::dependent_module;
use super::transport::{AudioBufferConfig, Transport, TransportState, TransportType, notify_transport_state};
use js_sys::{Object, Reflect};
use serde::Serialize;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

/// Check if WebTransport is available in the current browser
pub fn is_webtransport_available() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }

    #[cfg(target_arch = "wasm32")]
    {
    if let Some(window) = web_sys::window() {
        js_sys::Reflect::has(&window, &JsValue::from_str("WebTransport")).unwrap_or(false)
    } else {
        // Check for worker global scope
        let global = js_sys::global();
        js_sys::Reflect::has(&global, &JsValue::from_str("WebTransport")).unwrap_or(false)
    }
    }
}

/// Percent-encode `s` following JavaScript's `encodeURIComponent` semantics.
///
/// Every byte is escaped as `%XX` except the unreserved set
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`. Multi-byte UTF-8 characters are encoded
/// byte-by-byte. This is the single source of truth for query-parameter
/// encoding in the WebTransport connection URL, replacing the browser-only
/// `js_sys::encode_uri_component` so the URL builder can be tested natively.
fn encode_uri_component(s: &str) -> String {
    fn hex(nibble: u8) -> char {
        char::from(if nibble < 10 { b'0' + nibble } else { b'A' + (nibble - 10) })
    }

    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(hex(b >> 4));
                out.push(hex(b & 0x0f));
            }
        }
    }
    out
}

/// Build the WebTransport connection URL for a hub server.
///
/// Produces `https://{server}:{port}/webtransport`. When `client_name` is
/// non-empty, a percent-encoded `?name=…` query parameter is appended. This is
/// the single source of truth for the connection URL so both the live
/// `connect()` path and tests use identical logic.
fn build_connection_url(server: &str, port: u16, client_name: &str) -> String {
    if client_name.is_empty() {
        format!("https://{}:{}/webtransport", server, port)
    } else {
        let encoded_name = encode_uri_component(client_name);
        format!("https://{}:{}/webtransport?name={}", server, port, encoded_name)
    }
}

/// Compute the absolute URL of the WASM module (`pkg/webtrip.js`) from the
/// current page's `origin` and `pathname`.
///
/// The worker imports the WASM glue from a `blob:` context where relative paths
/// don't resolve, so we resolve it against the directory of the current page
/// (everything up to and including the last `/` in `pathname`). Extracted as a
/// pure function so the path math can be tested without a browser.
fn wasm_module_url(origin: &str, pathname: &str) -> String {
    let base_path = if let Some(last_slash) = pathname.rfind('/') {
        &pathname[..=last_slash]
    } else {
        "/"
    };
    format!("{}{}pkg/webtrip.js", origin, base_path)
}

/// Assemble a human-readable error string from a worker `ErrorEvent`.
///
/// `ErrorEvent` properties may be undefined, null, empty, or (for a synthetic
/// event) the wrong type, so each of `filename`/`lineno`/`message` is read
/// defensively via `Reflect` and only appended when present and meaningful. The
/// base prefix is always included, so the result is never empty. Extracted from
/// the worker `onerror` handler so this defensive parsing is the single source
/// of truth and can be unit-tested with synthetic events (including ones with
/// missing/undefined fields) without constructing a real `ErrorEvent` or worker.
fn assemble_error_event_message(event: &JsValue) -> String {
    let mut msg_parts = vec!["[WebTransport] Worker error".to_string()];

    // filename (may be undefined/null/empty)
    if let Ok(filename_val) = Reflect::get(event, &"filename".into()) {
        if !filename_val.is_undefined() && !filename_val.is_null() {
            if let Some(filename) = filename_val.as_string() {
                if !filename.is_empty() {
                    msg_parts.push(format!("at {}", filename));
                }
            }
        }
    }

    // line number (may be undefined/null or non-positive)
    if let Ok(lineno_val) = Reflect::get(event, &"lineno".into()) {
        if !lineno_val.is_undefined() && !lineno_val.is_null() {
            if let Some(lineno) = lineno_val.as_f64() {
                if lineno > 0.0 {
                    msg_parts.push(format!("line {}", lineno as u32));
                }
            }
        }
    }

    // message (may be undefined/null/empty)
    if let Ok(message_val) = Reflect::get(event, &"message".into()) {
        if !message_val.is_undefined() && !message_val.is_null() {
            if let Some(message) = message_val.as_string() {
                if !message.is_empty() {
                    msg_parts.push(format!("- {}", message));
                }
            }
        }
    }

    msg_parts.join(" ")
}

/// Build the plain-JSON `init` message sent to the worker from the audio buffer
/// configuration.
///
/// Carries only the data that can be represented as plain JSON (the message
/// type plus buffer pointers and audio config). The browser-only fields
/// (`wasmMemory`, `wasmUrl`) are attached separately in [`WebTransportImpl::init_worker`].
/// Pointer/config values are stored as `f64` to match the numeric type the
/// worker reads on the JS side. Extracted so field presence and values can be
/// asserted natively without constructing a real `Worker`.
fn build_worker_init_message(buffers: &AudioBufferConfig) -> serde_json::Value {
    serde_json::json!({
        "type": "init",
        "ringBufferPtr": buffers.local_to_network_ptr as usize as f64,
        "regulatorPtr": buffers.network_to_local_ptr as usize as f64,
        "bufferSize": buffers.buffer_size as f64,
        "channels": buffers.channels as f64,
    })
}

/// WebTransport implementation using a dedicated Web Worker
///
/// This transport spawns a Web Worker that handles all network I/O:
/// - The worker runs async send/receive loops
/// - Communication with main thread is via postMessage (setup only)
/// - Audio data flows through SharedArrayBuffer (RingBuffer/Regulator)
pub struct WebTransportImpl {
    state: TransportState,
    server_url: Option<String>,

    // Worker management
    //
    // `worker` is wrapped in `Rc<RefCell<_>>` so the onmessage closure can take
    // ownership of it and terminate the worker as soon as the worker reports
    // "disconnected" (i.e. after the send loop has flushed the JackTrip exit
    // packets). The `close()` fallback timer also holds a clone and is a no-op
    // if the happy path already terminated the worker.
    worker: Rc<RefCell<Option<web_sys::Worker>>>,
    worker_message_closure: Option<Closure<dyn FnMut(web_sys::MessageEvent)>>,
    worker_error_closure: Option<Closure<dyn FnMut(web_sys::ErrorEvent)>>,

    // Audio buffer configuration
    audio_buffers: Option<AudioBufferConfig>,

    // Callbacks
    on_state_change: Option<js_sys::Function>,

    // State synchronization
    connection_promise_resolve: Rc<RefCell<Option<js_sys::Function>>>,
    connection_promise_reject: Rc<RefCell<Option<js_sys::Function>>>,
    worker_ready_resolve: Rc<RefCell<Option<js_sys::Function>>>,
    worker_ready_reject: Rc<RefCell<Option<js_sys::Function>>>,
    // Resolver for the Promise returned by `close()`. Fired when the worker
    // posts "disconnected" (happy path) or when the 2s fallback timer elapses
    // (worker hung). See the `close()` impl for details.
    close_promise_resolve: Rc<RefCell<Option<js_sys::Function>>>,
}

impl WebTransportImpl {
    /// Create a new WebTransport implementation
    pub fn new() -> Result<Self, JsValue> {
        // Check if WebTransport is supported
        if !is_webtransport_available() {
            return Err("WebTransport not supported in this browser".into());
        }

        Ok(Self {
            state: TransportState::Disconnected,
            server_url: None,
            worker: Rc::new(RefCell::new(None)),
            worker_message_closure: None,
            worker_error_closure: None,
            audio_buffers: None,
            on_state_change: None,
            connection_promise_resolve: Rc::new(RefCell::new(None)),
            connection_promise_reject: Rc::new(RefCell::new(None)),
            worker_ready_resolve: Rc::new(RefCell::new(None)),
            worker_ready_reject: Rc::new(RefCell::new(None)),
            close_promise_resolve: Rc::new(RefCell::new(None)),
        })
    }

    /// Set callback for state changes
    pub fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.on_state_change = Some(callback);
    }

    /// Create and initialize the worker
    fn create_worker(&mut self) -> Result<(), JsValue> {
        // Create worker with module type for ES modules support
        let options = web_sys::WorkerOptions::new();
        options.set_type(web_sys::WorkerType::Module);

        // Load the worker JS file from bundled module (via dependent_module macro)
        // This embeds the file as a Blob URL at build time
        let worker_url = dependent_module!("webtransport_worker.js")?;
        let worker = web_sys::Worker::new_with_options(&worker_url, &options)
            .map_err(|e| {
                web_sys::console::error_1(&format!("[WebTransport] Failed to create worker: {:?}", e).into());
                e
            })?;

        // Set up message handler
        let state_change_cb = self.on_state_change.clone();
        let connection_resolve = self.connection_promise_resolve.clone();
        let connection_reject = self.connection_promise_reject.clone();
        let worker_ready_resolve = self.worker_ready_resolve.clone();
        let worker_ready_reject = self.worker_ready_reject.clone();
        let close_resolve = self.close_promise_resolve.clone();
        let worker_slot = self.worker.clone();

        let on_message = Closure::wrap(Box::new(move |event: web_sys::MessageEvent| {
            let data = event.data();

            // Handle string responses
            if let Some(msg) = data.as_string() {
                match msg.as_str() {
                    "ready" => {
                        web_sys::console::log_1(&"[WebTransport] ✅ Worker reports: READY".into());
                        if let Some(resolve) = worker_ready_resolve.borrow_mut().take() {
                            let _ = resolve.call0(&JsValue::NULL);
                        }
                    }
                    "connected" => {
                        web_sys::console::log_1(&"[WebTransport] ✅ Worker reports: CONNECTED".into());
                        if let Some(ref callback) = state_change_cb {
                            let _ = callback.call1(&JsValue::NULL, &JsValue::from_str("connected"));
                        }
                        // Resolve the connection promise
                        if let Some(resolve) = connection_resolve.borrow_mut().take() {
                            let _ = resolve.call0(&JsValue::NULL);
                        }
                    }
                    "disconnected" => {
                        web_sys::console::log_1(&"[WebTransport] ⚠️ Worker reports: DISCONNECTED".into());
                        // Worker has flushed the JackTrip exit packets — tear it
                        // down immediately so we don't wait for the close()
                        // fallback timer. Safe to call set_onmessage(None) from
                        // inside the onmessage handler itself; the current
                        // invocation keeps the closure alive via its outer
                        // reference.
                        //
                        // Ordering is important: terminate the worker FIRST so
                        // no further pushes to the shared Regulator can occur,
                        // THEN resolve the close() future. The session layer
                        // relies on this ordering to safely reset() the
                        // Regulator after awaiting close().
                        if let Some(worker) = worker_slot.borrow_mut().take() {
                            worker.set_onmessage(None);
                            worker.set_onerror(None);
                            worker.terminate();
                        }
                        if let Some(resolve) = close_resolve.borrow_mut().take() {
                            let _ = resolve.call0(&JsValue::NULL);
                        }
                        if let Some(ref callback) = state_change_cb {
                            let _ = callback.call1(&JsValue::NULL, &JsValue::from_str("disconnected"));
                        }
                    }
                    _ => {
                        web_sys::console::warn_1(
                            &format!("[WebTransport] 📋 Other worker message: {}", msg).into(),
                        );
                    }
                }
                return;
            }

            // Handle object responses (stats, errors)
            if let Ok(msg_type) = Reflect::get(&data, &"type".into()) {
                if let Some(type_str) = msg_type.as_string() {
                    match type_str.as_str() {
                        "error" => {
                            let error = Reflect::get(&data, &"error".into())
                                .ok()
                                .and_then(|v| v.as_string())
                                .unwrap_or_else(|| "Unknown error".to_string());
                            web_sys::console::error_1(
                                &format!("[WebTransport] ❌ Worker error: {}", error).into(),
                            );
                            if let Some(ref callback) = state_change_cb {
                                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str("failed"));
                            }
                            // Reject startup promises if still pending
                            if let Some(reject) = worker_ready_reject.borrow_mut().take() {
                                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&error));
                            }
                            // Reject the connection promise if pending
                            if let Some(reject) = connection_reject.borrow_mut().take() {
                                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&error));
                            }
                        }
                        "stats" => {
                            web_sys::console::debug_1(&"[WebTransport] 📊 Stats update received".into());
                            // Stats updates - could be forwarded to UI
                        }
                        _ => {
                            web_sys::console::warn_1(&format!("[WebTransport] 📋 Unhandled message type: {}", type_str).into());
                        }
                    }
                } else {
                    web_sys::console::warn_1(&"[WebTransport] ⚠️ Message has 'type' but it's not a string".into());
                }
            } else {
                web_sys::console::warn_1(&"[WebTransport] ⚠️ Message is object but has no 'type' field".into());
            }
        }) as Box<dyn FnMut(web_sys::MessageEvent)>);

        worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        self.worker_message_closure = Some(on_message);

        // Set up error handler
        let state_change_cb2 = self.on_state_change.clone();
        let onerror_ready_reject = self.worker_ready_reject.clone();
        let onerror_connection_reject = self.connection_promise_reject.clone();
        let on_error = Closure::wrap(Box::new(move |event: web_sys::ErrorEvent| {
            web_sys::console::error_1(&"[WebTransport] Worker error event triggered".into());

            // Log the full error object for debugging
            web_sys::console::error_1(&event);

            // Build the error message defensively (any ErrorEvent field may be
            // undefined). Shared with the unit tests so the parsing isn't
            // duplicated.
            let msg = assemble_error_event_message(event.as_ref());
            web_sys::console::error_1(&msg.clone().into());

            if let Some(ref callback) = state_change_cb2 {
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str("failed"));
            }
            // Reject startup promises if still pending so callers don't hang
            if let Some(reject) = onerror_ready_reject.borrow_mut().take() {
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&msg));
            }
            if let Some(reject) = onerror_connection_reject.borrow_mut().take() {
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str(&msg));
            }
        }) as Box<dyn FnMut(web_sys::ErrorEvent)>);

        worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        self.worker_error_closure = Some(on_error);

        *self.worker.borrow_mut() = Some(worker);
        Ok(())
    }

    /// Assemble the `init` message that gets posted to the worker.
    ///
    /// Merges the plain-JSON buffer config (from [`build_worker_init_message`])
    /// with the two browser-only fields that can't be represented in JSON:
    /// `wasmMemory` (for SharedArrayBuffer access) and `wasmUrl` (the absolute
    /// URL the blob: worker imports the WASM glue from). Split out from
    /// [`init_worker`] so the assembled message — including those two
    /// browser-only fields — can be inspected in tests without posting to (or
    /// even creating) a real worker.
    fn build_init_message(&self) -> Result<Object, JsValue> {
        let buffers = self.audio_buffers.ok_or("Audio buffers not configured")?;

        // Build the plain-JSON init message (type + buffer pointers + config)
        // and convert it to a JS object. `serialize_maps_as_objects(true)` is
        // required so the JSON object becomes a plain object (whose properties
        // the worker reads via Reflect), rather than an ES `Map`.
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        let msg: Object = build_worker_init_message(&buffers)
            .serialize(&serializer)
            .map_err(|e| JsValue::from_str(&format!("Failed to serialize init message: {e}")))?
            .unchecked_into();

        // Browser-only fields that can't be represented in plain JSON:

        // Pass WASM memory for SharedArrayBuffer access.
        Reflect::set(&msg, &"wasmMemory".into(), &wasm_bindgen::memory())?;

        // Pass the absolute URL to the WASM module (needed for import from blob: worker context).
        // Construct it relative to the current page location.
        let window = web_sys::window().ok_or("No window")?;
        let location = window.location();
        let origin = location.origin().map_err(|_| "Failed to get origin")?;
        let pathname = location.pathname().map_err(|_| "Failed to get pathname")?;
        let wasm_url = wasm_module_url(&origin, &pathname);
        Reflect::set(&msg, &"wasmUrl".into(), &JsValue::from_str(&wasm_url))?;

        Ok(msg)
    }

    /// Initialize the worker with buffer pointers
    fn init_worker(&self) -> Result<(), JsValue> {
        let worker = self.worker.borrow().clone().ok_or("Worker not created")?;
        let msg = self.build_init_message()?;
        worker.post_message(&msg)?;
        Ok(())
    }

    /// Send connect message to worker
    async fn connect_worker(&self, server_url: &str) -> Result<(), JsValue> {
        let worker = self.worker.borrow().clone().ok_or("Worker not created")?;

        // Create a promise that will be resolved by the message handler
        let (promise, resolve, reject) = crate::audio::make_promise();

        // Store resolve/reject for message handler
        *self.connection_promise_resolve.borrow_mut() = Some(resolve);
        *self.connection_promise_reject.borrow_mut() = Some(reject);

        // Send connect message
        let msg = Object::new();
        Reflect::set(&msg, &"type".into(), &"connect".into())?;
        Reflect::set(&msg, &"serverUrl".into(), &JsValue::from_str(server_url))?;

        worker.post_message(&msg)?;

        // Wait for connection to complete
        match JsFuture::from(promise).await {
            Ok(_) => {
                Ok(())
            }
            Err(e) => {
                web_sys::console::error_1(&format!("[WebTransport] ❌ Connection promise rejected: {:?}", e).into());
                Err(e)
            }
        }
    }

    /// Connect to a WebTransport server
    pub async fn connect_to_server(&mut self, server_url: String) -> Result<(), JsValue> {
        web_sys::console::log_1(
            &format!("[WebTransport] 🚀 Starting connection to {}", server_url).into(),
        );

        self.server_url = Some(server_url.clone());
        self.state = TransportState::Connecting;
        self.notify_state_change();

        // Create worker if needed
        if self.worker.borrow().is_none() {
            self.create_worker()?;
        }

        // Initialize worker with buffer pointers
        self.init_worker()?;

        // Wait for worker to signal it is ready (WASM loaded and initialized)
        let (ready_promise, ready_resolve, ready_reject) = crate::audio::make_promise();
        *self.worker_ready_resolve.borrow_mut() = Some(ready_resolve);
        *self.worker_ready_reject.borrow_mut() = Some(ready_reject);
        if let Err(e) = JsFuture::from(ready_promise).await {
            self.state = TransportState::Failed;
            self.notify_state_change();
            return Err(e);
        }

        // Connect via worker
        match self.connect_worker(&server_url).await {
            Ok(()) => {
                web_sys::console::log_1(&"[WebTransport] ✅ Worker connected successfully!".into());
            }
            Err(e) => {
                web_sys::console::error_1(&format!("[WebTransport] ❌ Worker connection failed: {:?}", e).into());
                self.state = TransportState::Failed;
                self.notify_state_change();
                return Err(e);
            }
        }

        self.state = TransportState::Connected;
        self.notify_state_change();

        Ok(())
    }

    fn notify_state_change(&self) {
        notify_transport_state(self.state, &self.on_state_change);
    }
}

impl Transport for WebTransportImpl {
    fn transport_type(&self) -> TransportType {
        TransportType::WebTransport
    }

    fn state(&self) -> TransportState {
        self.state
    }

    fn set_audio_buffers(&mut self, config: AudioBufferConfig) {
        self.audio_buffers = Some(config);
        super::transport::log_audio_buffers_set("WebTransport", config.channels, config.buffer_size);
    }

    fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.on_state_change = Some(callback);
    }

    fn connect(
        &mut self,
        server: &str,
        port: u16,
        client_name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), JsValue>> + '_>> {
        // Build WebTransport URL (HTTPS + /webtransport path)
        let url = build_connection_url(server, port, client_name);

        Box::pin(async move { self.connect_to_server(url).await })
    }

    fn tick(&mut self) {
        // WebTransport doesn't need tick() - the worker handles everything!
        // This is a key advantage over WebRTC.
    }

    fn is_connected(&self) -> bool {
        matches!(self.state, TransportState::Connected)
    }

    fn close(&mut self) -> Pin<Box<dyn Future<Output = ()> + '_>> {
        // Snapshot current state and eagerly fire off the synchronous shutdown
        // work, then return a future that resolves once the worker is
        // guaranteed not to issue further writes to the shared Regulator.
        //
        // Ordering for the happy path:
        //   1. Post `{type:"disconnect"}` to the worker.
        //   2. Worker's `send_loop` flushes JackTrip exit packets, then posts
        //      `"disconnected"` back to the main thread.
        //   3. `onmessage` handler (see `create_worker`) calls
        //      `worker.terminate()` — after this returns, the worker thread
        //      is dead and cannot push to the Regulator.
        //   4. `onmessage` handler resolves the promise returned below.
        //
        // Fallback (worker hung for >2s): the timer below terminates the
        // worker itself and resolves the promise. Either way, the future
        // only resolves *after* `worker.terminate()` has run, so awaiting it
        // establishes a happens-before relationship between any in-flight
        // `regulator.push()` inside the worker and subsequent operations on
        // the main thread (e.g. `Regulator::reset()`).

        let worker_opt = self.worker.borrow().clone();

        // If there is no worker, we are already closed: return an immediately
        // resolved future and skip the promise dance.
        let Some(worker) = worker_opt else {
            self.state = TransportState::Closed;
            self.notify_state_change();
            return Box::pin(async move {});
        };

        // Build the close promise and stash its resolver for `onmessage` and
        // the fallback timer to invoke.
        let (promise, resolve) = {
            let mut resolve_fn: Option<js_sys::Function> = None;
            let promise = js_sys::Promise::new(&mut |resolve, _reject| {
                resolve_fn = Some(resolve);
            });
            (promise, resolve_fn.expect("Promise executor must provide resolve"))
        };
        *self.close_promise_resolve.borrow_mut() = Some(resolve);

        // (1) Tell the worker to stop. It will send the JackTrip exit
        // packet(s) and post "disconnected" back before going idle.
        let msg = Object::new();
        let _ = Reflect::set(&msg, &"type".into(), &"disconnect".into());
        let _ = worker.post_message(&msg);

        // (Fallback) Schedule a 2s force-terminate in case the worker hangs
        // (e.g. `writer.write_with_chunk` stalls after a network loss) and
        // never gets to post "disconnected".
        //
        // Transfer ownership of the worker-level closures into the fallback
        // callback so they stay alive long enough to receive the worker's
        // "disconnected" message. They're dropped when the fallback fires
        // (or when the closure itself is garbage-collected after forget() —
        // the timer holds the only reference).
        let worker_slot = self.worker.clone();
        let close_resolve = self.close_promise_resolve.clone();
        let msg_closure = self.worker_message_closure.take();
        let err_closure = self.worker_error_closure.take();
        let cb = Closure::wrap(Box::new(move || {
            if let Some(worker) = worker_slot.borrow_mut().take() {
                web_sys::console::warn_1(
                    &"[WebTransport] Worker did not report disconnect within 2s — force-terminating".into(),
                );
                worker.set_onmessage(None);
                worker.set_onerror(None);
                worker.terminate();
            }
            if let Some(resolve) = close_resolve.borrow_mut().take() {
                let _ = resolve.call0(&JsValue::NULL);
            }
            let _ = &msg_closure;
            let _ = &err_closure;
        }) as Box<dyn FnMut()>);
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                2000,
            );
        } else {
            // No window (running in a worker scope) — terminate now and
            // resolve immediately.
            if let Some(worker) = self.worker.borrow_mut().take() {
                worker.terminate();
            }
            if let Some(resolve) = self.close_promise_resolve.borrow_mut().take() {
                let _ = resolve.call0(&JsValue::NULL);
            }
        }
        cb.forget();

        self.state = TransportState::Closed;
        self.notify_state_change();

        Box::pin(async move {
            // Ignore any rejection — the resolver above only ever resolves.
            let _ = JsFuture::from(promise).await;
        })
    }
}

impl Default for WebTransportImpl {
    fn default() -> Self {
        Self::new().expect("Failed to create WebTransport")
    }
}

impl Drop for WebTransportImpl {
    fn drop(&mut self) {
        // Drop can't await. Fire off the synchronous shutdown work (posting
        // the `{type:"disconnect"}` message and scheduling the 2s fallback
        // terminator) and discard the returned future. The JS-side timer and
        // onmessage handler set up by `close()` complete the teardown even
        // after this future is dropped.
        let _ = Transport::close(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::ring_buffer::RingBuffer;
    use crate::audio::regulator::Regulator;

    #[test]
    fn webtransport_unavailable_on_native() {
        // WebTransport is a browser API; on the native (cargo test) target the
        // detection must report unavailable.
        assert!(!is_webtransport_available());
    }

    // ── Browser feature detection (web_sys) ──────────────────────────────────
    //
    // The real-browser counterpart to `webtransport_unavailable_on_native`,
    // run in headless Chrome via `npm run test:wasm`. The per-binary
    // `run_in_browser` opt-in lives once in `crate::test_support`.

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;
    #[cfg(target_arch = "wasm32")]
    use crate::test_support::{dispatch_message_event, recording_state_callback};

    /// Chrome (the headless test browser, v97+) exposes the `WebTransport`
    /// global, so detection must report availability — the inverse of the
    /// native result above.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn webtransport_available_in_browser() {
        assert!(
            is_webtransport_available(),
            "WebTransport global must be present in the headless Chrome harness"
        );
    }

    // ── Worker creation / lifecycle (web_sys) ────────────────────────────────
    //
    // These exercise the browser-only Worker setup that the pure-logic tests
    // above can't reach. `create_worker` and `init_worker` are private, but
    // these inline tests live in a child module of `webtransport`, so they call
    // them directly without widening production visibility. The live
    // `connect()`/`connect_to_server()` path needs a real HTTP/3 server and is
    // intentionally not covered here.

    /// Terminate and detach a freshly-created test worker so the async
    /// module-load failure inherent to the test harness (the worker imports the
    /// bindgen module from a `blob:` context) can't fire callbacks after the
    /// test, and so `Drop`'s `close()` short-circuits instead of arming a 2s
    /// fallback timer.
    #[cfg(target_arch = "wasm32")]
    fn teardown_worker(transport: &WebTransportImpl) {
        if let Some(worker) = transport.worker.borrow_mut().take() {
            worker.set_onmessage(None);
            worker.set_onerror(None);
            worker.terminate();
        }
    }

    /// `create_worker()` must build a `web_sys::Worker` via the `dependent_module!`
    /// Blob-URL flow and register both the `onmessage` and `onerror` closures on
    /// it (retaining them on the struct so they outlive the call).
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn create_worker_builds_worker_and_registers_handlers() {
        let mut transport = WebTransportImpl::new()
            .expect("WebTransportImpl construction should succeed in the browser");

        transport
            .create_worker()
            .expect("create_worker should build the Worker via the Blob-URL flow");

        {
            let worker_ref = transport.worker.borrow();
            let worker = worker_ref
                .as_ref()
                .expect("worker must be stored after create_worker");
            assert!(
                worker.onmessage().is_some(),
                "onmessage handler must be registered on the worker"
            );
            assert!(
                worker.onerror().is_some(),
                "onerror handler must be registered on the worker"
            );
        }
        assert!(
            transport.worker_message_closure.is_some(),
            "message closure must be retained to stay alive"
        );
        assert!(
            transport.worker_error_closure.is_some(),
            "error closure must be retained to stay alive"
        );

        teardown_worker(&transport);
    }

    /// The init message that `init_worker()` posts must carry the two
    /// browser-only fields layered on top of the plain-JSON buffer config:
    /// `wasmMemory` (the module's `WebAssembly.Memory`) and `wasmUrl` (resolving
    /// to the bindgen glue). Asserting the assembled message directly via
    /// [`WebTransportImpl::build_init_message`] catches a typo in either
    /// `Reflect::set` key — which a no-panic-only smoke test would miss — and
    /// avoids posting to (or creating) a real worker, so no buffer pointers are
    /// ever dereferenced. The plain-JSON fields themselves are covered natively
    /// by `worker_init_message_has_expected_fields`.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn init_message_includes_browser_only_fields() {
        let mut transport = WebTransportImpl::new()
            .expect("WebTransportImpl construction should succeed in the browser");

        // Null buffer pointers: they are only serialized as numbers into the
        // message and never dereferenced (no worker is created or posted to).
        transport.audio_buffers = Some(AudioBufferConfig {
            local_to_network_ptr: std::ptr::null_mut::<RingBuffer>(),
            network_to_local_ptr: std::ptr::null_mut::<Regulator>(),
            buffer_size: 128,
            channels: 2,
        });

        let msg = transport
            .build_init_message()
            .expect("init message assembly should succeed in the browser");

        // Carried through from the plain-JSON payload.
        assert_eq!(
            Reflect::get(&msg, &"type".into()).unwrap().as_string().as_deref(),
            Some("init")
        );

        // Browser-only field 1: wasmMemory must be the module's WebAssembly.Memory.
        let wasm_memory = Reflect::get(&msg, &"wasmMemory".into()).unwrap();
        assert!(
            wasm_memory.is_instance_of::<js_sys::WebAssembly::Memory>(),
            "wasmMemory must be a WebAssembly.Memory instance"
        );

        // Browser-only field 2: wasmUrl must resolve to the bindgen glue module.
        let wasm_url = Reflect::get(&msg, &"wasmUrl".into())
            .unwrap()
            .as_string()
            .unwrap_or_default();
        assert!(
            wasm_url.ends_with("pkg/webtrip.js"),
            "wasmUrl must point at the bindgen glue, got: {wasm_url}"
        );
    }

    // ── State surface + teardown (web_sys) ───────────────────────────────────
    //
    // `WebTransportImpl::new()` only succeeds where the `WebTransport` global
    // exists, so these run in headless Chrome. They mirror the transport
    // state-surface assertions (initial state, `state()`, `is_connected()`
    // with no worker, `set_audio_buffers` storage) and the server-free `close()`
    // teardown — none of which need a live HTTP/3 server. The live
    // `connect()`/`connect_to_server()`/worker loops remain out of scope.

    /// A freshly constructed transport starts `Disconnected`, reports
    /// `is_connected() == false`, and holds no server URL, audio buffers, or
    /// worker until a connection is attempted.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn new_starts_disconnected_with_no_worker() {
        let transport = WebTransportImpl::new()
            .expect("WebTransportImpl construction should succeed in the browser");

        assert_eq!(transport.state(), TransportState::Disconnected);
        assert!(!transport.is_connected(), "a fresh transport must not be connected");
        assert!(transport.server_url.is_none(), "no server URL before connect");
        assert!(transport.audio_buffers.is_none(), "no audio buffers before set_audio_buffers");
        assert!(transport.worker.borrow().is_none(), "no worker until connect");
    }

    /// `set_audio_buffers()` stores the supplied configuration (the worker, not
    /// the main-thread shim, sizes the actual packet buffers from it). Null
    /// buffer pointers are safe: they are only stored, never dereferenced (no
    /// worker is created).
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn set_audio_buffers_stores_config() {
        let mut transport = WebTransportImpl::new()
            .expect("WebTransportImpl construction should succeed in the browser");

        transport.set_audio_buffers(AudioBufferConfig {
            local_to_network_ptr: std::ptr::null_mut::<RingBuffer>(),
            network_to_local_ptr: std::ptr::null_mut::<Regulator>(),
            buffer_size: 256,
            channels: 1,
        });

        let cfg = transport.audio_buffers.expect("config must be stored");
        assert_eq!(cfg.buffer_size, 256);
        assert_eq!(cfg.channels, 1);
    }

    /// `close()` on a never-connected instance (no worker ever created) takes
    /// the early-return path: it marks the transport `Closed` and returns an
    /// already-resolved future, with nothing to terminate.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn close_without_worker_resolves_immediately_and_marks_closed() {
        let mut transport = WebTransportImpl::new()
            .expect("WebTransportImpl construction should succeed in the browser");

        Transport::close(&mut transport).await;

        assert_eq!(transport.state(), TransportState::Closed);
        assert!(!transport.is_connected());
        assert!(transport.worker.borrow().is_none());
    }

    /// `close()` with a worker present but never connected exercises the full
    /// teardown path: it posts `{type:"disconnect"}` and arms the 2s fallback
    /// timer. With no live worker ever posting `"disconnected"`, the returned
    /// future resolves only once that timer fires and force-terminates the
    /// worker — proving teardown completes cleanly without a live server.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn close_with_worker_tears_down_via_fallback_timer() {
        let mut transport = WebTransportImpl::new()
            .expect("WebTransportImpl construction should succeed in the browser");

        transport
            .create_worker()
            .expect("create_worker should build the Worker via the Blob-URL flow");
        assert!(transport.worker.borrow().is_some(), "worker must exist before close");

        Transport::close(&mut transport).await;

        assert_eq!(transport.state(), TransportState::Closed);
        assert!(
            transport.worker.borrow().is_none(),
            "the fallback timer must terminate and clear the worker"
        );
    }

    // ── Connection-failure & graceful-teardown (main-thread handlers) ────────
    //
    // These cover the worker→main message/error handling the happy-path tests
    // can't reach: the `{type:"error"}` onmessage branch, the `onerror`
    // `ErrorEvent` handler, and the `"disconnected"` teardown branch. The
    // handlers capture clones of the struct's shared promise-resolver / state
    // callback `Rc`s at `create_worker()` time, so each test sets the state
    // callback first, creates the worker, stashes synthetic pending promise
    // resolvers, then drives the handler with a synthetic event. The worker
    // itself never connects (no HTTP/3 server is needed) and is torn down
    // before awaiting so the harness's async module-load failure can't fire a
    // late callback into the (already detached) handlers.

    /// Build a synthetic `{type:"error", error}` worker message payload.
    #[cfg(target_arch = "wasm32")]
    fn error_message_data(error: &str) -> JsValue {
        let obj = Object::new();
        Reflect::set(&obj, &"type".into(), &"error".into()).unwrap();
        Reflect::set(&obj, &"error".into(), &JsValue::from_str(error)).unwrap();
        obj.into()
    }

    /// A worker `{type:"error"}` message must reject BOTH still-pending startup
    /// promises (the `worker_ready` gate and the `connection` promise) and fire
    /// the state callback with `"failed"`, so a caller awaiting either promise
    /// never hangs.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn worker_error_message_rejects_pending_promises_and_fails() {
        let mut transport = WebTransportImpl::new().expect("construction in browser");

        let (log, cb) = recording_state_callback();
        transport.set_on_state_change(cb.as_ref().unchecked_ref::<js_sys::Function>().clone());

        transport.create_worker().expect("create_worker should build the worker");

        // Stash pending startup promises exactly as `connect_to_server` would.
        let (ready_promise, _ready_resolve, ready_reject) = crate::audio::make_promise();
        *transport.worker_ready_reject.borrow_mut() = Some(ready_reject);
        let (conn_promise, _conn_resolve, conn_reject) = crate::audio::make_promise();
        *transport.connection_promise_reject.borrow_mut() = Some(conn_reject);

        // Deliver the synthetic worker error to the real (registered) handler.
        let worker = transport.worker.borrow().clone().expect("worker present");
        dispatch_message_event(worker.as_ref(), &error_message_data("boom"));

        // Detach + terminate BEFORE awaiting so the harness's async module-load
        // failure can't fire a second "failed" into the state callback.
        teardown_worker(&transport);

        let ready_err = JsFuture::from(ready_promise)
            .await
            .expect_err("worker_ready promise must reject");
        assert_eq!(ready_err.as_string().as_deref(), Some("boom"));

        let conn_err = JsFuture::from(conn_promise)
            .await
            .expect_err("connection promise must reject");
        assert_eq!(conn_err.as_string().as_deref(), Some("boom"));

        assert_eq!(
            log.borrow().as_slice(),
            ["failed"],
            "a worker error must emit exactly one \"failed\" state change"
        );
        drop(cb);
    }

    /// A worker `ErrorEvent` must reject the pending startup promises and emit
    /// `"failed"`, with the rejection message assembled from the event's
    /// `message` field.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn worker_error_event_rejects_pending_promises_and_fails() {
        let mut transport = WebTransportImpl::new().expect("construction in browser");

        let (log, cb) = recording_state_callback();
        transport.set_on_state_change(cb.as_ref().unchecked_ref::<js_sys::Function>().clone());

        transport.create_worker().expect("create_worker should build the worker");

        let (ready_promise, _ready_resolve, ready_reject) = crate::audio::make_promise();
        *transport.worker_ready_reject.borrow_mut() = Some(ready_reject);
        let (conn_promise, _conn_resolve, conn_reject) = crate::audio::make_promise();
        *transport.connection_promise_reject.borrow_mut() = Some(conn_reject);

        // Fire a real ErrorEvent carrying a message at the registered onerror.
        let init = web_sys::ErrorEventInit::new();
        init.set_message("kaboom");
        let event = web_sys::ErrorEvent::new_with_event_init_dict("error", &init)
            .expect("ErrorEvent construction should succeed");
        let worker = transport.worker.borrow().clone().expect("worker present");
        worker
            .dispatch_event(&event)
            .expect("dispatching the error event should succeed");

        teardown_worker(&transport);

        let ready_err = JsFuture::from(ready_promise)
            .await
            .expect_err("worker_ready promise must reject on ErrorEvent");
        assert!(
            ready_err.as_string().unwrap_or_default().contains("kaboom"),
            "rejection should carry the assembled error message, got: {ready_err:?}"
        );
        let conn_err = JsFuture::from(conn_promise)
            .await
            .expect_err("connection promise must reject on ErrorEvent");
        assert!(conn_err.as_string().unwrap_or_default().contains("kaboom"));

        assert_eq!(log.borrow().as_slice(), ["failed"]);
        drop(cb);
    }

    /// `assemble_error_event_message` must read `filename`/`lineno`/`message`
    /// defensively: all three present and valid produce the full string.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn assemble_error_event_message_with_all_fields() {
        let obj = Object::new();
        Reflect::set(&obj, &"filename".into(), &"worker.js".into()).unwrap();
        Reflect::set(&obj, &"lineno".into(), &JsValue::from_f64(42.0)).unwrap();
        Reflect::set(&obj, &"message".into(), &"boom".into()).unwrap();

        let msg = assemble_error_event_message(obj.as_ref());
        assert_eq!(msg, "[WebTransport] Worker error at worker.js line 42 - boom");
    }

    /// `assemble_error_event_message` must NOT panic when fields are entirely
    /// absent (undefined) — it falls back to just the base prefix. This is the
    /// real-world defensive case the worker `onerror` guards against.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn assemble_error_event_message_tolerates_missing_fields() {
        // An empty object: every field read returns `undefined`.
        let empty = Object::new();
        assert_eq!(
            assemble_error_event_message(empty.as_ref()),
            "[WebTransport] Worker error"
        );

        // Mixed: a valid message but a zero line number and an empty filename,
        // both of which must be skipped (only meaningful parts are appended).
        let obj = Object::new();
        Reflect::set(&obj, &"filename".into(), &"".into()).unwrap();
        Reflect::set(&obj, &"lineno".into(), &JsValue::from_f64(0.0)).unwrap();
        Reflect::set(&obj, &"message".into(), &"only message".into()).unwrap();
        assert_eq!(
            assemble_error_event_message(obj.as_ref()),
            "[WebTransport] Worker error - only message"
        );
    }

    /// The graceful `"disconnected"` worker message must terminate the worker
    /// (clearing the slot), resolve the pending `close()` promise, and emit a
    /// `"disconnected"` (NOT `"failed"`) state change.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn disconnected_message_terminates_worker_and_resolves_close() {
        let mut transport = WebTransportImpl::new().expect("construction in browser");

        let (log, cb) = recording_state_callback();
        transport.set_on_state_change(cb.as_ref().unchecked_ref::<js_sys::Function>().clone());

        transport.create_worker().expect("create_worker should build the worker");

        // Stash a pending close() resolver as Transport::close would.
        let (close_promise, close_resolve, _close_reject) = crate::audio::make_promise();
        *transport.close_promise_resolve.borrow_mut() = Some(close_resolve);

        let worker = transport.worker.borrow().clone().expect("worker present");
        dispatch_message_event(worker.as_ref(), &JsValue::from_str("disconnected"));

        // The handler resolves the close promise; awaiting must complete.
        JsFuture::from(close_promise)
            .await
            .expect("close promise must resolve on \"disconnected\"");

        assert!(
            transport.worker.borrow().is_none(),
            "the \"disconnected\" branch must terminate and clear the worker"
        );
        assert_eq!(
            log.borrow().as_slice(),
            ["disconnected"],
            "graceful teardown must emit \"disconnected\", never \"failed\""
        );
        drop(cb);
    }

    // --- encode_uri_component ---

    #[test]
    fn encode_uri_component_leaves_unreserved_chars() {
        // The full JS encodeURIComponent unreserved set must pass through verbatim.
        let unreserved = "ABCabc012-_.!~*'()";
        assert_eq!(encode_uri_component(unreserved), unreserved);
    }

    #[test]
    fn encode_uri_component_escapes_reserved_chars() {
        assert_eq!(encode_uri_component("a b"), "a%20b");
        assert_eq!(encode_uri_component("a/b?c=d&e"), "a%2Fb%3Fc%3Dd%26e");
        // Multi-byte UTF-8 is encoded byte-by-byte (é = 0xC3 0xA9).
        assert_eq!(encode_uri_component("é"), "%C3%A9");
    }

    // --- build_connection_url ---

    #[test]
    fn connection_url_without_name() {
        assert_eq!(
            build_connection_url("hub.example.com", 4464, ""),
            "https://hub.example.com:4464/webtransport"
        );
    }

    #[test]
    fn connection_url_with_plain_name() {
        assert_eq!(
            build_connection_url("hub.example.com", 4464, "alice"),
            "https://hub.example.com:4464/webtransport?name=alice"
        );
    }

    #[test]
    fn connection_url_percent_encodes_name() {
        // Spaces and reserved characters in the client name must be encoded so
        // the query string stays well-formed.
        assert_eq!(
            build_connection_url("hub.example.com", 4464, "Alice & Bob"),
            "https://hub.example.com:4464/webtransport?name=Alice%20%26%20Bob"
        );
    }

    // --- wasm_module_url ---

    #[test]
    fn wasm_module_url_with_directory_path() {
        assert_eq!(
            wasm_module_url("https://example.com", "/app/index.html"),
            "https://example.com/app/pkg/webtrip.js"
        );
    }

    #[test]
    fn wasm_module_url_at_root() {
        assert_eq!(
            wasm_module_url("https://example.com", "/"),
            "https://example.com/pkg/webtrip.js"
        );
    }

    #[test]
    fn wasm_module_url_without_slash_uses_root() {
        // Defensive: a pathname with no slash falls back to "/".
        assert_eq!(
            wasm_module_url("https://example.com", "index.html"),
            "https://example.com/pkg/webtrip.js"
        );
    }

    // --- build_worker_init_message ---

    #[test]
    fn worker_init_message_has_expected_fields() {
        let config = AudioBufferConfig {
            local_to_network_ptr: 0x1000 as *mut RingBuffer,
            network_to_local_ptr: 0x2000 as *mut Regulator,
            buffer_size: 128,
            channels: 2,
        };

        let msg = build_worker_init_message(&config);

        assert_eq!(msg["type"], "init");
        assert_eq!(msg["ringBufferPtr"], 0x1000 as f64);
        assert_eq!(msg["regulatorPtr"], 0x2000 as f64);
        assert_eq!(msg["bufferSize"], 128.0);
        assert_eq!(msg["channels"], 2.0);
    }
}
