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
