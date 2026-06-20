//! Transport abstraction layer for audio packet transmission
//!
//! This module defines the trait that all transport implementations must implement,
//! allowing for runtime selection of different transport mechanisms (WebRTC, WebTransport, etc.)

use std::pin::Pin;
use std::future::Future;
use wasm_bindgen::prelude::*;
use web_sys;

/// Transport type selection
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportType {
    /// WebRTC Data Channels (current, universal support)
    WebRTC,
    /// WebTransport (future, Chrome/Edge only)
    WebTransport,
    /// Mock transport for testing
    Mock,
}

impl TransportType {
    /// Get a human-readable name for the transport
    pub fn name(&self) -> String {
        match self {
            TransportType::WebRTC => "WebRTC Data Channels".to_string(),
            TransportType::WebTransport => "WebTransport (QUIC)".to_string(),
            TransportType::Mock => "Mock (Testing)".to_string(),
        }
    }

    /// Get a short identifier for the transport
    pub fn id(&self) -> String {
        match self {
            TransportType::WebRTC => "webrtc".to_string(),
            TransportType::WebTransport => "webtransport".to_string(),
            TransportType::Mock => "mock".to_string(),
        }
    }

    /// Parse transport type from string ID
    pub fn from_id(id: &str) -> Option<TransportType> {
        match id {
            "webrtc" => Some(TransportType::WebRTC),
            "webtransport" => Some(TransportType::WebTransport),
            "mock" => Some(TransportType::Mock),
            _ => None,
        }
    }
}

/// Connection state shared across all transports
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportState {
    Disconnected,
    Connecting,
    Connected,
    Failed,
    Closed,
}

/// Audio buffer configuration passed to transports that need internal tick loops
#[derive(Debug, Clone, Copy)]
pub struct AudioBufferConfig {
    /// Pointer to ring buffer (local-to-network)
    pub local_to_network_ptr: *mut crate::audio::ring_buffer::RingBuffer,
    /// Pointer to jitter buffer (network-to-local) - mutable for Regulator
    pub network_to_local_ptr: *mut crate::audio::regulator::Regulator,
    /// Buffer size in samples per channel
    pub buffer_size: usize,
    /// Number of audio channels
    pub channels: u8,
}

// Safety: These pointers are only used by the transport layer which is single-threaded in WASM
unsafe impl Send for AudioBufferConfig {}

/// Log that audio buffers have been configured on a transport
pub(crate) fn log_audio_buffers_set(transport_name: &str, channels: u8, buffer_size: usize) {
    web_sys::console::debug_1(
        &format!(
            "✅ {}: Audio buffers configured ({}ch, {} samples)",
            transport_name, channels, buffer_size
        )
        .into(),
    );
}

/// Transport trait that all implementations must implement
/// 
/// This is a minimal interface focused on audio packet transmission.
/// Implementation-specific details (like WebRTC signaling and internal tick loops) 
/// are handled internally by each transport.
/// 
/// ## Buffer Management
/// 
/// Transports run their own internal loops to manage audio packet flow:
/// - `set_audio_buffers()` provides access to ring buffer (send) and jitter buffer (receive)
/// - `start_streaming()` begins the internal send/receive loop
/// - The transport directly reads from ring buffer and writes to jitter buffer
/// 
/// This design keeps transport logic isolated and allows WebRTC to stay on the main
/// thread while other transports (like WebTransport) can run in worker threads.
pub trait Transport {
    /// Get current transport type
    fn transport_type(&self) -> TransportType;

    /// Get current connection state
    fn state(&self) -> TransportState;

    /// Set audio buffer pointers (for transports that need internal tick loops)
    /// 
    /// This is called by the session before starting the connection to give
    /// the transport access to audio buffers. WebRTC transport uses this to
    /// run its internal tick loop. Other transports can ignore this.
    fn set_audio_buffers(&mut self, _config: AudioBufferConfig) {
        // Default: no-op (not all transports need buffers)
    }

    /// Set callback for transport state changes
    /// 
    /// This allows the session layer to be notified when the transport's
    /// connection state changes (e.g., connected, failed, disconnected).
    /// The callback receives a state string: "connected", "failed", "disconnected"
    fn set_on_state_change(&mut self, _callback: js_sys::Function) {
        // Default: no-op (not all transports need state callbacks)
    }

    /// Connect to a hub server
    /// 
    /// This async method establishes a connection to the hub server and returns
    /// when the connection is fully established and ready to send/receive packets.
    /// 
    /// Transports manage their own internal send/receive loops after connection.
    /// 
    /// # Arguments
    /// * `server` - Server hostname or IP
    /// * `port` - Server port
    /// * `client_name` - Client identifier
    fn connect(
        &mut self,
        server: &str,
        port: u16,
        client_name: &str,
    ) -> Pin<Box<dyn Future<Output = Result<(), JsValue>> + '_>>;

    /// Process one audio callback tick
    /// 
    /// Called by the session layer each time the audio worklet's process() callback runs.
    /// Transports should:
    /// - Read from ring buffer and send packets
    /// - Receive packets and write to jitter buffer
    fn tick(&mut self) {
        // Default: no-op (for transports that don't need tick processing)
    }

    /// Check if connected and ready to send
    fn is_connected(&self) -> bool {
        matches!(self.state(), TransportState::Connected)
    }

    /// Close the connection.
    ///
    /// This method eagerly performs any synchronous teardown (e.g. posting a
    /// shutdown message to a worker, scheduling a fallback timer) and returns
    /// a future that resolves once the transport is fully torn down. Callers
    /// that need to guarantee no further writes to the audio buffers (e.g.
    /// before resetting a `Regulator`) must `await` the returned future. For
    /// best-effort cleanup (e.g. in `Drop`) the future can be discarded; the
    /// synchronous teardown has already been initiated.
    fn close(&mut self) -> Pin<Box<dyn Future<Output = ()> + '_>>;
}

