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
    let options = AudioWorkletNodeOptions::new();
    options.set_number_of_inputs(1);
    options.set_number_of_outputs(1);
    options.set_channel_count(1);
    options.set_channel_count_mode(web_sys::ChannelCountMode::Explicit);

    let output_channels = js_sys::Array::new();
    output_channels.push(&JsValue::from(1));
    options.set_output_channel_count(&output_channels);

    options.set_processor_options(Some(&js_sys::Array::of3(
        &wasm_bindgen::module(),
        &wasm_bindgen::memory(),
        &ProcessorHandle::new(process).into_raw_ptr().into(),
    )));

    AudioWorkletNode::new_with_options(ctx, "WasmProcessor", &options)
}

