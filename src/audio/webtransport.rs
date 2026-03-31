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
use super::transport::{AudioBufferConfig, Transport, TransportState, TransportType};
use js_sys::{Object, Reflect};
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
    worker: Option<web_sys::Worker>,
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
            worker: None,
            worker_message_closure: None,
            worker_error_closure: None,
            audio_buffers: None,
            on_state_change: None,
            connection_promise_resolve: Rc::new(RefCell::new(None)),
            connection_promise_reject: Rc::new(RefCell::new(None)),
            worker_ready_resolve: Rc::new(RefCell::new(None)),
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
        let on_error = Closure::wrap(Box::new(move |event: web_sys::ErrorEvent| {
            web_sys::console::error_1(&"[WebTransport] Worker error event triggered".into());
            
            // Log the full error object for debugging
            web_sys::console::error_1(&event);
            
            // Build error message safely - ErrorEvent properties may be undefined
            let mut msg_parts = vec!["[WebTransport] Worker error".to_string()];
            
            // Try to get filename (may be undefined)
            if let Ok(filename_val) = js_sys::Reflect::get(&event, &"filename".into()) {
                if !filename_val.is_undefined() && !filename_val.is_null() {
                    if let Some(filename) = filename_val.as_string() {
                        if !filename.is_empty() {
                            msg_parts.push(format!("at {}", filename));
                        }
                    }
                }
            }
            
            // Try to get line number
            if let Ok(lineno_val) = js_sys::Reflect::get(&event, &"lineno".into()) {
                if !lineno_val.is_undefined() && !lineno_val.is_null() {
                    if let Some(lineno) = lineno_val.as_f64() {
                        if lineno > 0.0 {
                            msg_parts.push(format!("line {}", lineno as u32));
                        }
                    }
                }
            }
            
            // Try to get message
            if let Ok(message_val) = js_sys::Reflect::get(&event, &"message".into()) {
                if !message_val.is_undefined() && !message_val.is_null() {
                    if let Some(message) = message_val.as_string() {
                        if !message.is_empty() {
                            msg_parts.push(format!("- {}", message));
                        }
                    }
                }
            }
            
            let msg = msg_parts.join(" ");
            web_sys::console::error_1(&msg.into());
            
            if let Some(ref callback) = state_change_cb2 {
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str("failed"));
            }
        }) as Box<dyn FnMut(web_sys::ErrorEvent)>);

        worker.set_onerror(Some(on_error.as_ref().unchecked_ref()));
        self.worker_error_closure = Some(on_error);

        self.worker = Some(worker);
        Ok(())
    }

    /// Initialize the worker with buffer pointers
    fn init_worker(&self) -> Result<(), JsValue> {
        let worker = self.worker.as_ref().ok_or("Worker not created")?;
        let buffers = self.audio_buffers.ok_or("Audio buffers not configured")?;


        // Send init message with buffer pointers
        let msg = Object::new();
        Reflect::set(&msg, &"type".into(), &"init".into())?;
        Reflect::set(
            &msg,
            &"ringBufferPtr".into(),
            &JsValue::from_f64(buffers.local_to_network_ptr as usize as f64),
        )?;
        Reflect::set(
            &msg,
            &"regulatorPtr".into(),
            &JsValue::from_f64(buffers.network_to_local_ptr as usize as f64),
        )?;
        Reflect::set(
            &msg,
            &"bufferSize".into(),
            &JsValue::from_f64(buffers.buffer_size as f64),
        )?;
        Reflect::set(
            &msg,
            &"channels".into(),
            &JsValue::from_f64(buffers.channels as f64),
        )?;

        // Also pass WASM memory for SharedArrayBuffer access
        Reflect::set(&msg, &"wasmMemory".into(), &wasm_bindgen::memory())?;

        // Pass the absolute URL to the WASM module (needed for import from blob: worker context)
        // Construct the URL relative to the current page location
        let window = web_sys::window().ok_or("No window")?;
        let location = window.location();
        let origin = location.origin().map_err(|_| "Failed to get origin")?;
        let pathname = location.pathname().map_err(|_| "Failed to get pathname")?;
        
        // Get the base path (directory of the current page)
        let base_path = if let Some(last_slash) = pathname.rfind('/') {
            &pathname[..=last_slash]
        } else {
            "/"
        };
        
        // Construct the absolute URL to the WASM module
        let wasm_url = format!("{}{}pkg/webtrip.js", origin, base_path);
        Reflect::set(&msg, &"wasmUrl".into(), &JsValue::from_str(&wasm_url))?;

        worker.post_message(&msg)?;
        Ok(())
    }

    /// Send connect message to worker
    async fn connect_worker(&self, server_url: &str) -> Result<(), JsValue> {
        let worker = self.worker.as_ref().ok_or("Worker not created")?;

        // Create a promise that will be resolved by the message handler
        let (promise, resolve, reject) = {
            let mut resolve_fn = None;
            let mut reject_fn = None;
            let promise = js_sys::Promise::new(&mut |resolve, reject| {
                resolve_fn = Some(resolve);
                reject_fn = Some(reject);
            });
            (promise, resolve_fn.unwrap(), reject_fn.unwrap())
        };

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
        if self.worker.is_none() {
            self.create_worker()?;
        }

        // Initialize worker with buffer pointers
        self.init_worker()?;

        // Wait for worker to signal it is ready (WASM loaded and initialized)
        let (ready_promise, ready_resolve, _ready_reject) = {
            let mut resolve_fn = None;
            let mut reject_fn = None;
            let promise = js_sys::Promise::new(&mut |resolve, reject| {
                resolve_fn = Some(resolve);
                reject_fn = Some(reject);
            });
            (promise, resolve_fn.unwrap(), reject_fn.unwrap())
        };
        *self.worker_ready_resolve.borrow_mut() = Some(ready_resolve);
        JsFuture::from(ready_promise).await?;

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
        if let Some(ref callback) = self.on_state_change {
            let state_str = match self.state {
                TransportState::Disconnected => "disconnected",
                TransportState::Connecting => "connecting",
                TransportState::Connected => "connected",
                TransportState::Failed => "failed",
                TransportState::Closed => "closed",
            };
            let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(state_str));
        }
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
        web_sys::console::debug_1(
            &format!(
                "[WebTransport] Audio buffers configured ({}ch, {} samples)",
                config.channels, config.buffer_size
            )
            .into(),
        );
    }

    fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.on_state_change = Some(callback);
    }

    fn connect(
        &mut self,
        server: &str,
        port: u16,
        use_tls: bool,
        client_name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), JsValue>> + '_>> {
        // Build WebTransport URL
        // WebTransport requires HTTPS and the /webtransport path
        let protocol = if use_tls { "https" } else { "https" }; // WebTransport always uses HTTPS

        // Only include name parameter if client_name is not empty
        let url = if client_name.is_empty() {
            format!("{}://{}:{}/webtransport", protocol, server, port)
        } else {
            let encoded_name = js_sys::encode_uri_component(client_name);
            format!(
                "{}://{}:{}/webtransport?name={}",
                protocol, server, port, encoded_name
            )
        };

        Box::pin(async move { self.connect_to_server(url).await })
    }

    fn tick(&mut self) {
        // WebTransport doesn't need tick() - the worker handles everything!
        // This is a key advantage over WebRTC.
    }

    fn is_connected(&self) -> bool {
        matches!(self.state, TransportState::Connected)
    }

    fn close(&mut self) {
        // Send disconnect message to worker
        if let Some(ref worker) = self.worker {
            let msg = Object::new();
            let _ = Reflect::set(&msg, &"type".into(), &"disconnect".into());
            let _ = worker.post_message(&msg);

            // Terminate worker
            worker.terminate();
        }

        // Clean up
        self.worker = None;
        
        // Remove event handlers before dropping closures
        self.worker_message_closure = None;
        self.worker_error_closure = None;

        self.state = TransportState::Closed;
        self.notify_state_change();
    }
}

impl Default for WebTransportImpl {
    fn default() -> Self {
        Self::new().expect("Failed to create WebTransport")
    }
}

impl Drop for WebTransportImpl {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webtransport_available_check() {
        // In non-WASM test environment, this should return false
        // (no window object)
        // This test mainly ensures the function doesn't panic
        let _ = is_webtransport_available();
    }
}
