//! Browser audio host: device enumeration, the audio engine, and the
//! AudioWorklet integration. The portable processor and parameters live in
//! `webtrip_core::audio`.
//!
//! Dependency rule: `audio` never imports `network` (and vice versa);
//! `session.rs` composes them.

pub mod devices;
pub mod worklet;
pub mod engine;
pub mod audio_callback_loop;

/// An `Error` with the given `name`. A connect rejects with the DOM's
/// conventional names for an aborted (`"AbortError"`) or timed-out
/// (`"TimeoutError"`) operation, so a caller can tell a cancellation it caused
/// from a failure (a cross-language contract, relied on by the demo).
pub(crate) fn named_error(name: &str, message: &str) -> wasm_bindgen::JsValue {
    let error = js_sys::Error::new(message);
    error.set_name(name);
    error.into()
}

pub(crate) fn cancelled_error() -> wasm_bindgen::JsValue {
    named_error("AbortError", "Connection cancelled by disconnect()")
}

// Helper function for creating a Promise with extractable resolve/reject
pub(crate) fn make_promise() -> (js_sys::Promise, js_sys::Function, js_sys::Function) {
    let mut resolve_fn = None;
    let mut reject_fn = None;
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        resolve_fn = Some(resolve);
        reject_fn = Some(reject);
    });
    (promise, resolve_fn.unwrap(), reject_fn.unwrap())
}

pub use devices::DeviceInfo;
pub use worklet::ProcessorHandle;
pub use engine::AudioEngine;
pub use audio_callback_loop::{AudioCallbackLoop, has_atomics_wait_async};
