// Linked so the API's `#[wasm_bindgen]` exports reach the generated package
pub use webtrip_api::{api, models};

// Browser hosts
pub mod audio;
pub mod network;

// Other web modules
mod connection;
mod console_log;
mod dependent_module;
mod js_util;
pub mod session;
mod tick_clock;

// Shared scaffolding for browser (`wasm-bindgen-test`) tests. Holds the single
// `wasm_bindgen_test_configure!(run_in_browser)` for the lib unit-test binary
// plus reusable assertions; see `src/test_support.rs` for the convention.
#[cfg(all(test, target_arch = "wasm32"))]
mod test_support;

// Re-export public types for convenience
pub use audio::{DeviceInfo, ProcessorHandle, AudioEngine};
pub use webtrip_core::audio::{AudioProcessor, AudioParams};
pub use webtrip_core::stream::{RingBuffer, Regulator, RegulatorStats};
pub use webtrip_core::network::{
    // Protocol types
    AudioFormat, AudioPacket, PacketHeader, StreamStats,
    // Transport types
    Transport, TransportType, TransportState,
};
pub use network::{
    TransportConfig, WebRtcTransport, WebTransportImpl,
    // Signaling types
    HubSignaling, HubConnectionState, SignalingMessage,
};

// Re-export session types
pub use session::{WebTripSession, SessionState, SessionStats};

use wasm_bindgen::prelude::*;

/// Wall-clock milliseconds, the one clock every host call into the portable
/// core is stamped with (`Regulator::push_at`/`pop_at` must share an epoch
/// across the main thread, the worker and the worklet, so all use `Date.now()`).
pub(crate) fn now_ms() -> f64 {
    js_sys::Date::now()
}

/// Initialize the WASM module (call once at startup)
#[wasm_bindgen]
pub fn init() {
    console_error_panic_hook::set_once();
    console_log::install();
}

/// Get the WASM memory for use with Atomics.waitAsync
///
/// This is needed by the event-driven network loop to access
/// the ring buffer flag via SharedArrayBuffer.
#[wasm_bindgen(js_name = getWasmMemory)]
pub fn get_wasm_memory() -> JsValue {
    wasm_bindgen::memory().into()
}

/// Serialize this module's accumulated LLVM coverage counters as `.profraw`
/// bytes, for the integration harness to write to disk and feed to
/// `llvm-profdata`/`llvm-cov`.
///
/// The counters live in the module's linear memory, which every thread shares
/// (the WebTransport worker is handed `wasm_bindgen::memory()` at init), so a
/// single call from the main thread captures the worker's execution too.
///
/// Only compiled under the `coverage` feature, which pulls in `minicov` to
/// supply the profiler runtime (`-Zno-profiler-runtime` omits LLVM's own).
#[cfg(feature = "coverage")]
#[wasm_bindgen(js_name = __coverageDump)]
pub fn coverage_dump() -> Vec<u8> {
    let mut data = Vec::new();
    // SAFETY: called once, from the main thread, after driving has quiesced.
    unsafe { minicov::capture_coverage(&mut data).expect("capture coverage") };
    data
}
