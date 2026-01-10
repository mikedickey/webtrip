//! WebTransport implementation (stub for future implementation)
//!
//! This module provides a WebTransport-based transport for JackTrip audio streaming.
//! WebTransport offers better performance than WebRTC data channels by:
//! - Running in a dedicated Worker thread (no main thread contention)
//! - Using QUIC instead of SCTP (better congestion control)
//! - Native datagram support (no need for RTP-style framing)
//!
//! Current status: STUB - Not yet fully implemented
//! See docs/WEBTRANSPORT_EXPLORATION.md for implementation plan

use super::transport::{Transport, TransportState, TransportType};
use std::future::Future;
use std::pin::Pin;
use wasm_bindgen::prelude::*;

/// WebTransport implementation (stub)
pub struct WebTransportImpl {
    state: TransportState,
    server_url: Option<String>,
    
    // Callbacks
    #[allow(dead_code)]
    on_state_change: Option<js_sys::Function>,
}

impl WebTransportImpl {
    /// Create a new WebTransport implementation
    pub fn new() -> Result<Self, JsValue> {
        // Check if WebTransport is supported
        let window = web_sys::window().ok_or("No window object")?;
        let has_webtransport = js_sys::Reflect::has(&window, &"WebTransport".into())?;
        
        if !has_webtransport {
            return Err("WebTransport not supported in this browser".into());
        }

        Ok(Self {
            state: TransportState::Disconnected,
            server_url: None,
            on_state_change: None,
        })
    }

    /// Set callback for state changes
    pub fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.on_state_change = Some(callback);
    }

    /// Connect to a WebTransport server
    pub async fn connect(&mut self, server_url: String) -> Result<(), JsValue> {
        web_sys::console::log_1(&"[WebTransport] Connecting (stub implementation)...".into());
        
        self.server_url = Some(server_url.clone());
        self.state = TransportState::Connecting;
        self.notify_state_change();

        // TODO: Implement actual WebTransport connection
        // 1. Create Worker
        // 2. Initialize WASM memory in worker
        // 3. Pass buffer pointers to worker
        // 4. Establish WebTransport connection
        // 5. Start send/receive loops in worker
        
        // For now, just fail gracefully
        Err("WebTransport implementation not yet complete. Use WebRTC instead.".into())
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

    fn connect(
        &mut self,
        _server: &str,
        _port: u16,
        _use_tls: bool,
        _client_name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), JsValue>> + '_>> {
        // WebTransport is not yet implemented - return error
        Box::pin(async move {
            Err("WebTransport connect not yet implemented".into())
        })
    }

    fn close(&mut self) {
        self.state = TransportState::Closed;
        self.notify_state_change();
        
        // TODO: Close worker and WebTransport connection
    }
}

impl Default for WebTransportImpl {
    fn default() -> Self {
        Self::new().expect("Failed to create WebTransport")
    }
}
