// Audio subsystem (contains all audio-related modules)
pub mod audio;

// Clean API client module
// Provides a friendly, ergonomic interface to the JackTrip Virtual Studio API
pub mod api;

// Clean, typed models for the JackTrip API
// All models are exposed to JavaScript via wasm-bindgen and generate TypeScript types
pub mod models;

// Other core modules
mod dependent_module;
pub mod session;

// Shared scaffolding for browser (`wasm-bindgen-test`) tests. Holds the single
// `wasm_bindgen_test_configure!(run_in_browser)` for the lib unit-test binary
// plus reusable assertions; see `src/test_support.rs` for the convention.
#[cfg(all(test, target_arch = "wasm32"))]
mod test_support;

// Re-export all audio types for convenience
pub use audio::{
    // Core audio types
    AudioProcessor, AudioParams, DeviceInfo, ProcessorHandle, AudioEngine,
    // Buffer types
    RingBuffer, Regulator, RegulatorStats,
    // Protocol types
    AudioFormat, AudioPacket, PacketHeader, StreamStats,
    // Transport types
    Transport, TransportType, TransportState,
    TransportConfig,
    WebRtcTransport,
    MockTransport, WebTransportImpl,
    // Signaling types
    HubSignaling, HubConnectionState, SignalingMessage,
};

// Re-export session types
pub use session::{WebTripSession, SessionState, SessionStats};

use wasm_bindgen::prelude::*;

/// Initialize the WASM module (call once at startup)
#[wasm_bindgen]
pub fn init() {
    console_error_panic_hook::set_once();
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
