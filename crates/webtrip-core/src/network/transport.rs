//! Transport abstraction layer for audio packet transmission
//!
//! This module defines the trait that all transport implementations must implement,
//! allowing for runtime selection of different transport mechanisms (WebRTC, WebTransport, etc.)

use std::pin::Pin;
use std::future::Future;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

use crate::network::protocol::{AudioPacket, PacketHeader, ProtocolError};
use crate::stream::regulator::{PushOutcome, Regulator};
use crate::stream::shared_ptr::SharedPtr;

/// In the high-error/reject-rate regime, emit at most one warning per this
/// many occurrences, so a degraded link or a persistently mismatched peer
/// can't flood the log from a realtime receive loop. Shared by
/// [`deliver_received_packet`]'s rejection warnings and the WebTransport
/// worker's deserialize-error accounting.
#[doc(hidden)]
pub const HIGH_RATE_WARN_INTERVAL: u64 = 50;

/// Deserialize one received wire packet and deliver it to the regulator.
///
/// The single receive-side entry point shared by both transports: WebRTC's
/// main-thread tick loop and the WebTransport worker's datagram receive loop
/// both reuse this instead of duplicating deserialize-then-push. Deserializes
/// `data` via [`AudioPacket::deserialize_into`] into `samples`, then pushes it
/// with the header's declared incoming channel count at `now_ms` (the caller's
/// clock; core reads none). On any non-`Stored`
/// [`PushOutcome`] (bad channel count, a channel count that changed
/// mid-stream, or a size mismatch — see `Regulator::push_at`), emits a
/// throttled `log::warn!` so a persistently silent stream is diagnosable instead of
/// failing with no stat and no log.
#[doc(hidden)]
pub fn deliver_received_packet(
    regulator: &Regulator,
    data: &[u8],
    samples: &mut Vec<f32>,
    now_ms: f64,
) -> Result<PushOutcome, ProtocolError> {
    let header: PacketHeader = AudioPacket::deserialize_into(data, samples)?;
    let outcome = regulator.push_at(
        header.sequence_number,
        header.num_outgoing_channels as usize,
        samples,
        now_ms,
    );

    if outcome != PushOutcome::Stored {
        let rejected = regulator.stats().packets_rejected;
        if rejected % HIGH_RATE_WARN_INTERVAL == 0 {
            log::warn!(
                "⚠️ Regulator rejected a received packet: {:?} ({} rejected so far)",
                outcome, rejected
            );
        }
    }

    Ok(outcome)
}

/// Transport type selection
#[cfg_attr(feature = "wasm", wasm_bindgen)]
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
#[cfg_attr(feature = "wasm", wasm_bindgen)]
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
    pub local_to_network: SharedPtr<crate::stream::ring_buffer::RingBuffer>,
    /// Jitter buffer (network-to-local); reached via its `&self` SPSC API
    /// through [`SharedPtr::as_ref`].
    pub network_to_local: SharedPtr<crate::stream::regulator::Regulator>,
    /// Buffer size in samples per channel
    pub buffer_size: usize,
    /// Channels this client sends: the ring buffer's interleaved width and
    /// every outbound packet's payload width (`num_outgoing_channels`, byte 15)
    pub send_channels: u8,
    /// Channels this client asks the peer to send back
    /// (`num_incoming_channels`, byte 14, on every outbound packet)
    pub receive_channels: u8,
}

// `AudioBufferConfig` is handed to the transport's network loop, which runs on a
// different agent (worker) or OS thread from the audio thread. Nothing in the
// build otherwise *requires* it to be `Send + Sync` — addresses cross agents as
// integers over `postMessage` — so without this assertion `SharedPtr`'s
// `T: Sync` bound would never be checked against the real pointees (WEB-54).
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<AudioBufferConfig>();
};

impl AudioBufferConfig {
    /// Interleaved samples in one outbound packet: `buffer_size` frames at
    /// `send_channels` wide.
    pub fn samples_per_packet(&self) -> usize {
        self.buffer_size * self.send_channels as usize
    }
}

/// Log that audio buffers have been configured on a transport
#[doc(hidden)]
pub fn log_audio_buffers_set(transport_name: &str, config: &AudioBufferConfig) {
    log::debug!(
        "✅ {}: Audio buffers configured (send {}ch, receive {}ch, {} samples)",
        transport_name, config.send_channels, config.receive_channels, config.buffer_size
    );
}

/// Map a [`TransportState`] to its canonical string representation.
///
/// This is the single source of truth for state names passed to JavaScript
/// callbacks. Extracted as a pure function so it can be tested without a
/// browser.
#[doc(hidden)]
pub fn transport_state_str(state: TransportState) -> &'static str {
    match state {
        TransportState::Disconnected => "disconnected",
        TransportState::Connecting   => "connecting",
        TransportState::Connected    => "connected",
        TransportState::Failed       => "failed",
        TransportState::Closed       => "closed",
    }
}

/// Whether a transport-level state string reports that the connection failed.
///
/// Only failures cross from the transport into the session: the session
/// drives `connecting`/`connected` itself, so a transport's own (possibly
/// late) reports of those are dropped.
///
/// The two transports deliberately differ on `"disconnected"`:
///
/// * **WebRTC** reports `"disconnected"`/`"closed"` only when the peer
///   connection or data channel drops, which from the session's point of view
///   is a failure.
/// * **WebTransport** posts `"disconnected"` from the worker after a
///   *graceful*, user-initiated close (once the JackTrip exit packets have been
///   flushed); genuine failures arrive separately as `"failed"`. Treating
///   `"disconnected"` as a failure there flips the UI from "Not Connected" to
///   "Connection Error" ~20 ms after the user clicks Disconnect.
pub fn transport_state_is_failure(transport_type: TransportType, transport_state: &str) -> bool {
    match (transport_type, transport_state) {
        (_, "failed") => true,
        (TransportType::WebRTC, "disconnected" | "closed") => true,
        _ => false,
    }
}

/// Callback a host installs to hear about transport state changes.
pub type StateCallback = Box<dyn Fn(TransportState)>;

/// Why a transport could not connect. Portable: carries only a message, so a
/// host maps it to its own error type (the browser host makes a JS `Error`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportError {
    message: String,
}

impl TransportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TransportError {}

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
    /// The callback receives the new [`TransportState`]
    fn set_on_state_change(&mut self, _callback: StateCallback) {
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
    ) -> Pin<Box<dyn Future<Output = Result<(), TransportError>> + '_>>;

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

    // Transport-state → session failure mapping

    /// Full matrix over both transports and every state string either of them
    /// can emit. The rows that matter most:
    ///
    /// * WebTransport `"disconnected"` must not be a failure (regression: a
    ///   graceful user-initiated disconnect flipped the UI to "Connection
    ///   Error" ~20 ms after the exit packets were flushed).
    /// * WebRTC `"disconnected"`/`"closed"` must be a failure (the peer
    ///   connection really did drop).
    /// * `"connected"`/`"connecting"` are never forwarded: the session drives
    ///   those itself, and a late transport report must not regress the UI.
    #[test]
    fn transport_failure_reports_per_transport() {
        use TransportType::{WebRTC, WebTransport};

        let cases: &[(TransportType, &str, bool)] = &[
            (WebRTC, "failed", true),
            (WebTransport, "failed", true),
            // Divergent: a dropped peer connection vs. a graceful worker close.
            (WebRTC, "disconnected", true),
            (WebRTC, "closed", true),
            (WebTransport, "disconnected", false),
            (WebTransport, "closed", false),
            // Progress reports belong to the session, not the transport.
            (WebRTC, "connected", false),
            (WebTransport, "connected", false),
            (WebRTC, "connecting", false),
            (WebTransport, "connecting", false),
            // Anything unrecognized is dropped rather than guessed at.
            (WebRTC, "", false),
            (WebTransport, "", false),
        ];

        for &(transport, state, expected) in cases {
            assert_eq!(
                transport_state_is_failure(transport, state),
                expected,
                "{transport:?} transport state {state:?} mapped incorrectly"
            );
        }
    }
}
