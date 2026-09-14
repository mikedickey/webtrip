//! Transport abstraction layer for audio packet transmission
//!
//! This module defines the trait that all transport implementations must implement,
//! allowing for runtime selection of different transport mechanisms (WebRTC, WebTransport, etc.)

use std::pin::Pin;
use std::future::Future;
use wasm_bindgen::prelude::*;
use web_sys;

use crate::audio::protocol::{AudioPacket, PacketHeader, ProtocolError};
use crate::audio::regulator::{PushOutcome, Regulator};
use crate::audio::shared_ptr::SharedPtr;

/// In the high-error/reject-rate regime, emit at most one warning per this
/// many occurrences, so a degraded link or a persistently mismatched peer
/// can't flood the console from a realtime receive loop. Only the browser
/// build logs, so the constant is wasm-only (dead code in the native test
/// build otherwise). Shared by [`deliver_received_packet`]'s rejection
/// warnings and the WebTransport worker's deserialize-error accounting.
#[cfg(target_arch = "wasm32")]
pub(crate) const HIGH_RATE_WARN_INTERVAL: u64 = 50;

/// Deserialize one received wire packet and deliver it to the regulator.
///
/// The single receive-side entry point shared by both transports: WebRTC's
/// main-thread tick loop and the WebTransport worker's datagram receive loop
/// both reuse this instead of duplicating deserialize-then-push. Deserializes
/// `data` via [`AudioPacket::deserialize_into`] into `samples`, then pushes it
/// with the header's declared incoming channel count. On any non-`Stored`
/// [`PushOutcome`] (bad channel count, a channel count that changed
/// mid-stream, or a size mismatch — see `Regulator::push`), emits a throttled
/// `console::warn` so a persistently silent stream is diagnosable instead of
/// failing with no stat and no log.
pub(crate) fn deliver_received_packet(
    regulator: &mut Regulator,
    data: &[u8],
    samples: &mut Vec<f32>,
) -> Result<PushOutcome, ProtocolError> {
    let header: PacketHeader = AudioPacket::deserialize_into(data, samples)?;
    let outcome = regulator.push(header.sequence_number, header.num_outgoing_channels as usize, samples);

    #[cfg(target_arch = "wasm32")]
    if outcome != PushOutcome::Stored {
        let rejected = regulator.stats().packets_rejected;
        if rejected % HIGH_RATE_WARN_INTERVAL == 0 {
            web_sys::console::warn_1(&format!(
                "⚠️ Regulator rejected a received packet: {:?} ({} rejected so far)",
                outcome, rejected
            ).into());
        }
    }

    Ok(outcome)
}

/// Transport type selection
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportType {
    /// WebRTC Data Channels (current, universal support)
    WebRTC,
    /// WebTransport (future, Chrome/Edge only)
    WebTransport,
}

impl TransportType {
    /// Get a human-readable name for the transport
    pub fn name(&self) -> String {
        match self {
            TransportType::WebRTC => "WebRTC Data Channels".to_string(),
            TransportType::WebTransport => "WebTransport (QUIC)".to_string(),
        }
    }

    /// Get a short identifier for the transport
    pub fn id(&self) -> String {
        match self {
            TransportType::WebRTC => "webrtc".to_string(),
            TransportType::WebTransport => "webtransport".to_string(),
        }
    }

    /// Parse transport type from string ID
    pub fn from_id(id: &str) -> Option<TransportType> {
        match id {
            "webrtc" => Some(TransportType::WebRTC),
            "webtransport" => Some(TransportType::WebTransport),
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

/// Audio buffer configuration passed to transports that need internal tick loops.
///
/// The buffers live in shared WASM memory owned by [`WebTripSession`]; the
/// transport reaches them through [`SharedPtr`], which carries the `Send`/`Sync`
/// assertion so this struct needs no hand-rolled `unsafe impl`.
#[derive(Debug, Clone, Copy)]
pub struct AudioBufferConfig {
    /// Ring buffer (local-to-network); reached via its `&self` API.
    pub local_to_network: SharedPtr<crate::audio::ring_buffer::RingBuffer>,
    /// Jitter buffer (network-to-local); still `&mut`-accessed via
    /// [`SharedPtr::as_mut`].
    pub network_to_local: SharedPtr<crate::audio::regulator::Regulator>,
    /// Buffer size in samples per channel
    pub buffer_size: usize,
    /// Channels this client sends: the ring buffer's interleaved width and
    /// every outbound packet's payload width (`num_outgoing_channels`, byte 15)
    pub send_channels: u8,
    /// Channels this client asks the peer to send back
    /// (`num_incoming_channels`, byte 14, on every outbound packet)
    pub receive_channels: u8,
}

/// Log that audio buffers have been configured on a transport
pub(crate) fn log_audio_buffers_set(transport_name: &str, config: &AudioBufferConfig) {
    web_sys::console::debug_1(
        &format!(
            "✅ {}: Audio buffers configured (send {}ch, receive {}ch, {} samples)",
            transport_name, config.send_channels, config.receive_channels, config.buffer_size
        )
        .into(),
    );
}

/// Map a [`TransportState`] to its canonical string representation.
///
/// This is the single source of truth for state names passed to JavaScript
/// callbacks. Extracted as a pure function so it can be tested without a
/// browser.
pub(crate) fn transport_state_str(state: TransportState) -> &'static str {
    match state {
        TransportState::Disconnected => "disconnected",
        TransportState::Connecting   => "connecting",
        TransportState::Connected    => "connected",
        TransportState::Failed       => "failed",
        TransportState::Closed       => "closed",
    }
}

/// Notify a JS state-change callback with the string representation of `state`.
/// Shared by all three transport implementations.
pub(crate) fn notify_transport_state(state: TransportState, callback: &Option<js_sys::Function>) {
    if let Some(ref cb) = callback {
        let _ = cb.call1(&JsValue::NULL, &JsValue::from_str(transport_state_str(state)));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_type_id_covers_all_variants() {
        // Pins a cross-language contract: the demo UI
        // (website/src/pages/demo/Demo.tsx) hardcodes these id strings.
        assert_eq!(TransportType::WebRTC.id(), "webrtc");
        assert_eq!(TransportType::WebTransport.id(), "webtransport");
    }

    #[test]
    fn transport_type_from_id_roundtrip() {
        for t in [TransportType::WebRTC, TransportType::WebTransport] {
            assert_eq!(
                TransportType::from_id(&t.id()),
                Some(t),
                "from_id(id()) should be the identity for {:?}",
                t,
            );
        }
    }

    #[test]
    fn transport_type_from_id_unknown_returns_none() {
        assert_eq!(TransportType::from_id("unknown"), None);
        assert_eq!(TransportType::from_id(""), None);
        // IDs are case-sensitive.
        assert_eq!(TransportType::from_id("WebRTC"), None);
        assert_eq!(TransportType::from_id("MOCK"), None);
    }

    #[test]
    fn transport_state_str_covers_all_variants() {
        assert_eq!(transport_state_str(TransportState::Disconnected), "disconnected");
        assert_eq!(transport_state_str(TransportState::Connecting),   "connecting");
        assert_eq!(transport_state_str(TransportState::Connected),    "connected");
        assert_eq!(transport_state_str(TransportState::Failed),       "failed");
        assert_eq!(transport_state_str(TransportState::Closed),       "closed");
    }
}
