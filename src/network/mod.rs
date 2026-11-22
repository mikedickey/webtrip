//! Browser network hosts: WebRTC, WebTransport (+ its worker) and hub
//! signaling. The portable protocol and `Transport` contract live in
//! `webtrip_core::network`.
//!
//! Dependency rule: `network` never imports `audio` (and vice versa);
//! `session.rs` composes them.

pub mod webrtc;
pub mod webtransport;
pub mod webtransport_worker;
pub mod signaling;

pub use webrtc::{TransportConfig, WebRtcTransport};
pub use webtransport::WebTransportImpl;
pub use signaling::{HubSignaling, HubConnectionState, SignalingMessage};
