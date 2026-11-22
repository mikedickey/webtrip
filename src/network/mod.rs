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
pub use signaling::HubSignaling;
pub use crate::webtrip_core::network::signaling_message::{HubConnectionState, SignalingMessage};

use std::rc::Rc;

use wasm_bindgen::{JsCast, JsValue};

use crate::webtrip_core::network::transport::{TransportError, TransportState};

/// A transport's state callback as its browser handlers hold it: cloned into
/// several JS event closures, all confined to the agent that registered them
/// (the `Rc` is justified by that confinement, not by this file's name). The
/// portable trait hands over a `Box`, converted once at `set_on_state_change`.
pub(crate) type SharedStateCallback = Rc<dyn Fn(TransportState)>;

/// Report `state` to the installed callback, if any.
pub(crate) fn notify_transport_state(state: TransportState, callback: &Option<SharedStateCallback>) {
    if let Some(callback) = callback {
        callback(state);
    }
}

/// Flatten a JS rejection into a portable [`TransportError`]: an `Error`'s
/// message, a thrown string as is, anything else via `Debug`.
pub(crate) fn transport_error_from_js(error: &JsValue) -> TransportError {
    let message = match error.dyn_ref::<js_sys::Error>() {
        Some(error) => String::from(error.message()),
        None => error.as_string().unwrap_or_else(|| format!("{error:?}")),
    };
    TransportError::new(message)
}
