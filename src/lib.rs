mod audio_processor;
mod audio_params;
mod dependent_module;
mod audio_devices;
mod audio_worklet;
mod audio_engine;

pub use audio_processor::AudioProcessor;
pub use audio_params::AudioParams;
pub use audio_devices::DeviceInfo;
pub use audio_worklet::ProcessorHandle;
pub use audio_engine::AudioEngine;

use wasm_bindgen::prelude::*;

/// Initialize the WASM module (call once at startup)
#[wasm_bindgen]
pub fn init() {
    console_error_panic_hook::set_once();
}
