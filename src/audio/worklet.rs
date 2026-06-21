use crate::dependent_module;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioContext, AudioWorkletNode, AudioWorkletNodeOptions};

/// Type alias for the audio processing callback
pub type ProcessorCallback = Box<dyn FnMut(&[f32], &mut [f32]) -> bool>;

/// Handle for a WASM audio processor that can be passed to the AudioWorklet
#[wasm_bindgen]
pub struct ProcessorHandle(ProcessorCallback);

#[wasm_bindgen]
impl ProcessorHandle {
    /// Process audio through the callback
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> bool {
        self.0(input, output)
    }

    /// Convert the processor into a raw pointer for passing to JavaScript
    pub fn into_raw_ptr(self) -> usize {
        Box::into_raw(Box::new(self)) as usize
    }

    /// Reconstruct the processor from a raw pointer (unsafe)
    pub unsafe fn from_raw_ptr(val: usize) -> Self {
        *Box::from_raw(val as *mut _)
    }
}

impl ProcessorHandle {
    /// Create a new processor handle from a callback
    pub fn new(callback: ProcessorCallback) -> Self {
        Self(callback)
    }
}

/// Register the audio worklet module with the AudioContext
pub async fn register_audio_worklet(ctx: &AudioContext) -> Result<(), JsValue> {
    let mod_url = dependent_module!("worklet.js")?;
    JsFuture::from(ctx.audio_worklet()?.add_module(&mod_url)?).await?;
    Ok(())
}

/// Create an AudioWorkletNode running a WASM audio processor
pub fn create_worklet_node(
    ctx: &AudioContext,
    process: ProcessorCallback,
) -> Result<AudioWorkletNode, JsValue> {
    create_worklet_node_with_flag(ctx, process, None)
}

/// Create an AudioWorkletNode with optional ring buffer flag pointer
/// for event-driven wake-up using Atomics.notify()
pub fn create_worklet_node_with_flag(
    ctx: &AudioContext,
    process: ProcessorCallback,
    ring_buffer_flag_ptr: Option<usize>,
) -> Result<AudioWorkletNode, JsValue> {
    let options = AudioWorkletNodeOptions::new();
    options.set_number_of_inputs(1);
    options.set_number_of_outputs(1);
    options.set_channel_count(1);
    options.set_channel_count_mode(web_sys::ChannelCountMode::Explicit);

    let output_channels = js_sys::Array::new();
    output_channels.push(&JsValue::from(1));
    options.set_output_channel_count(&output_channels);

    // Pass module, memory, processor handle, and optionally the ring buffer flag pointer
    let processor_options = js_sys::Array::new();
    processor_options.push(&wasm_bindgen::module());
    processor_options.push(&wasm_bindgen::memory());
    processor_options.push(&ProcessorHandle::new(process).into_raw_ptr().into());
    
    if let Some(flag_ptr) = ring_buffer_flag_ptr {
        processor_options.push(&JsValue::from_f64(flag_ptr as f64));
    }
    
    options.set_processor_options(Some(&processor_options));

    AudioWorkletNode::new_with_options(ctx, "WasmProcessor", &options)
}

// ==============================================================================
// Tests
// ==============================================================================
//
// Worklet wiring is pure browser glue, so coverage lives in `npm run test:wasm`
// (headless Chrome). The whole module is gated on `wasm32` because there is no
// native-testable logic here; on the native target it compiles away cleanly.
// The per-binary `run_in_browser` opt-in lives once in `crate::test_support`.
#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;
    use wasm_bindgen_test::wasm_bindgen_test;

    /// `register_audio_worklet` must resolve without error: `dependent_module!`
    /// assembles `worklet.js` into a `blob:` URL (prepending the bindgen ES
    /// import) and registers it via `AudioContext.audioWorklet.addModule`. This
    /// covers the dependent-module Blob/URL flow that the engine relies on.
    #[wasm_bindgen_test]
    async fn register_audio_worklet_resolves() {
        let ctx = AudioContext::new()
            .expect("AudioContext creation should succeed in the browser");

        register_audio_worklet(&ctx)
            .await
            .expect("worklet module registration via the Blob URL should resolve");
    }
}

