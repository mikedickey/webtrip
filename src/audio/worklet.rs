use crate::audio::protocol::MAX_CHANNELS;
use crate::dependent_module;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{AudioContext, AudioWorkletNode, AudioWorkletNodeOptions, ChannelInterpretation};

/// Frames per AudioWorklet render quantum. Fixed by the Web Audio spec, and
/// the per-channel capacity of [`ProcessorHandle`]'s scratch buffers.
pub const RENDER_QUANTUM_FRAMES: usize = 128;

/// Type alias for the audio processing callback.
///
/// `input` is one render quantum of mono capture (real multichannel capture
/// is a later phase). `output` is planar across `out_channels` output
/// channels: `output[ch * frames + frame]`, `frames == input.len()`.
pub type ProcessorCallback = Box<dyn FnMut(&[f32], &mut [f32], usize) -> bool>;

/// Handle for a WASM audio processor that can be passed to the AudioWorklet.
///
/// Owns preallocated planar scratch buffers in WASM linear memory
/// (`RENDER_QUANTUM_FRAMES * MAX_CHANNELS` each) so `worklet.js` can build
/// stable `Float32Array` views over them once and `.set()` each channel in
/// place every callback, instead of wasm-bindgen copying a `&[f32]` argument
/// on every call.
#[wasm_bindgen]
pub struct ProcessorHandle {
    callback: ProcessorCallback,
    /// Planar input scratch: channel `ch`'s samples live at
    /// `[ch * RENDER_QUANTUM_FRAMES .. + frames]`. Only channel 0 is read
    /// today (capture stays mono until real multichannel capture lands).
    input_scratch: Vec<f32>,
    /// Planar output scratch, same per-channel layout as `input_scratch`.
    output_scratch: Vec<f32>,
}

#[wasm_bindgen]
impl ProcessorHandle {
    /// Pointer to the planar input scratch buffer.
    pub fn input_ptr(&self) -> usize {
        self.input_scratch.as_ptr() as usize
    }

    /// Pointer to the planar output scratch buffer.
    pub fn output_ptr(&self) -> usize {
        self.output_scratch.as_ptr() as usize
    }

    /// Render one callback's worth of audio through the wrapped processor.
    ///
    /// `frames` is clamped to [`RENDER_QUANTUM_FRAMES`] and `out_channels` to
    /// [`MAX_CHANNELS`] — both scratch buffers' actual capacity — because this
    /// is the ABI boundary crossed from JS: `debug_assert!` alone does not
    /// run in an optimized (release) Wasm build, so an out-of-range value
    /// from a caller would otherwise slice `input_scratch`/`output_scratch`
    /// out of bounds and panic regardless of build profile (see PR #84
    /// review). The output planes this writes are laid out contiguously at
    /// stride `frames`, which only matches the fixed
    /// `ch * RENDER_QUANTUM_FRAMES` offsets the JS side builds its views at
    /// when `frames == RENDER_QUANTUM_FRAMES` — true for every live
    /// AudioWorklet callback.
    pub fn render(&mut self, in_channels: usize, out_channels: usize, frames: usize) -> bool {
        // `in_channels` is accepted for forward compatibility with real
        // multichannel capture; only the first input plane is read today.
        let _ = in_channels;
        let out_channels = out_channels.max(1).min(MAX_CHANNELS as usize);
        let frames = frames.min(RENDER_QUANTUM_FRAMES);
        (self.callback)(
            &self.input_scratch[..frames],
            &mut self.output_scratch[..out_channels * frames],
            out_channels,
        )
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
        let capacity = RENDER_QUANTUM_FRAMES * MAX_CHANNELS as usize;
        Self {
            callback,
            input_scratch: vec![0.0; capacity],
            output_scratch: vec![0.0; capacity],
        }
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
    create_worklet_node_with_flag(ctx, process, 1, 1, None)
}

/// Create an AudioWorkletNode with the given input/output channel counts and
/// an optional ring buffer flag pointer for event-driven wake-up using
/// `Atomics.notify()`.
///
/// `channelCount`/`outputChannelCount` are set `Explicit`/`Discrete` so
/// channels are never speaker-folded or up-mixed by the graph itself — the
/// mapping between capture/peer channels and output channels is `webtrip`'s
/// own (`crate::audio::processor::map_to_output`), not the browser's.
pub fn create_worklet_node_with_flag(
    ctx: &AudioContext,
    process: ProcessorCallback,
    input_channels: usize,
    output_channels: usize,
    ring_buffer_flag_ptr: Option<usize>,
) -> Result<AudioWorkletNode, JsValue> {
    let options = AudioWorkletNodeOptions::new();
    options.set_number_of_inputs(1);
    options.set_number_of_outputs(1);
    options.set_channel_count(input_channels as u32);
    options.set_channel_count_mode(web_sys::ChannelCountMode::Explicit);
    options.set_channel_interpretation(ChannelInterpretation::Discrete);

    let output_channel_count = js_sys::Array::new();
    output_channel_count.push(&JsValue::from(output_channels as u32));
    options.set_output_channel_count(&output_channel_count);

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
// `ProcessorHandle::render` is the worklet ABI boundary: JS writes capture
// samples into the input scratch buffer via `input_ptr()`, calls `render`,
// then reads the output scratch buffer via `output_ptr()`. That pointer
// hand-off only means anything against a real WASM linear memory /
// `wasm-bindgen` object lifetime, so it is covered here in the browser via
// `npm run test:wasm` rather than a native test.
#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
    use super::*;

    // Browser-test opt-in (`wasm_bindgen_test_configure!(run_in_browser)`) lives
    // once in `crate::test_support`.
    use wasm_bindgen_test::wasm_bindgen_test;

    /// Drives `render` through the real scratch buffers: writes a
    /// per-channel-distinguishable mono input plane, renders at a 2-channel
    /// output width, and asserts both output planes via `output_ptr()` — the
    /// full ABI round trip a real worklet callback performs.
    #[wasm_bindgen_test]
    fn render_writes_planar_output_through_scratch_buffers() {
        let frames = 4usize;
        let out_channels = 2usize;

        let mut handle = ProcessorHandle::new(Box::new(move |input, output, out_ch| {
            // Mirror `map_to_output`'s src_channels==1 rule: copy the mono
            // input to every output channel plane, scaled by channel index so
            // the two planes are distinguishable in the assertion below.
            let frames = input.len();
            for ch in 0..out_ch {
                for f in 0..frames {
                    output[ch * frames + f] = input[f] * (ch as f32 + 1.0);
                }
            }
            true
        }));

        let input_ptr = handle.input_ptr();
        // SAFETY: `input_ptr` points at `handle`'s own live `input_scratch`
        // `Vec<f32>`, which JS would instead reach through a `Float32Array`
        // view over wasm memory at this same address.
        let input_scratch = unsafe { std::slice::from_raw_parts_mut(input_ptr as *mut f32, frames) };
        for (i, s) in input_scratch.iter_mut().enumerate() {
            *s = (i + 1) as f32;
        }

        let result = handle.render(1, out_channels, frames);
        assert!(result, "render must return the callback's result");

        let output_ptr = handle.output_ptr();
        // SAFETY: same reasoning as the input scratch read above.
        let output_scratch =
            unsafe { std::slice::from_raw_parts(output_ptr as *const f32, out_channels * frames) };

        for ch in 0..out_channels {
            let plane = &output_scratch[ch * frames..ch * frames + frames];
            for (f, &sample) in plane.iter().enumerate() {
                let expected = (f + 1) as f32 * (ch as f32 + 1.0);
                assert_eq!(
                    sample, expected,
                    "channel {ch} frame {f}: expected {expected}, got {sample}"
                );
            }
        }
    }

    /// Regression test for PR #84 review: a destination reporting more than
    /// `MAX_CHANNELS` (a multichannel audio interface) must not make `render`
    /// slice `output_scratch` — capacity `RENDER_QUANTUM_FRAMES * MAX_CHANNELS`
    /// — out of bounds. At a full render quantum, `MAX_CHANNELS + 1` output
    /// channels is exactly the shape that panicked before `render` clamped
    /// `out_channels`.
    #[wasm_bindgen_test]
    fn render_clamps_out_channels_above_max_channels() {
        let frames = RENDER_QUANTUM_FRAMES;
        let requested_out_channels = MAX_CHANNELS as usize + 1;
        let seen_out_channels = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let seen = seen_out_channels.clone();

        let mut handle = ProcessorHandle::new(Box::new(move |_input, output, out_ch| {
            seen.set(out_ch);
            output.fill(1.0);
            true
        }));

        let result = handle.render(1, requested_out_channels, frames);
        assert!(
            result,
            "render must not panic when out_channels exceeds MAX_CHANNELS"
        );
        assert_eq!(
            seen_out_channels.get(),
            MAX_CHANNELS as usize,
            "render must clamp out_channels to MAX_CHANNELS before invoking the callback"
        );
    }

    /// Regression test for PR #84 review: the *lower* boundary of the
    /// `out_channels` clamp. `render_clamps_out_channels_above_max_channels`
    /// only covers the upper bound; a caller passing `0` (or a regression
    /// that dropped the `.max(1)`) must still see the callback invoked with
    /// exactly one output channel rather than an empty output slice.
    #[wasm_bindgen_test]
    fn render_clamps_out_channels_below_one() {
        let frames = RENDER_QUANTUM_FRAMES;
        let seen_out_channels = std::rc::Rc::new(std::cell::Cell::new(usize::MAX));
        let seen = seen_out_channels.clone();

        let mut handle = ProcessorHandle::new(Box::new(move |_input, output, out_ch| {
            seen.set(out_ch);
            output.fill(1.0);
            true
        }));

        let result = handle.render(1, 0, frames);
        assert!(result, "render must not panic when out_channels is 0");
        assert_eq!(
            seen_out_channels.get(),
            1,
            "render must clamp out_channels up to 1 before invoking the callback"
        );
    }

    /// Regression test for PR #84 review: `render`'s `frames` argument used
    /// to be checked only by a `debug_assert!`, which does not run in an
    /// optimized (release) Wasm build — the build every `npm run build:wasm`
    /// invocation produces. A `frames` above `RENDER_QUANTUM_FRAMES` must be
    /// clamped rather than left to slice `input_scratch`/`output_scratch` out
    /// of bounds and panic.
    #[wasm_bindgen_test]
    fn render_clamps_frames_above_render_quantum() {
        let requested_frames = RENDER_QUANTUM_FRAMES + 1;
        let seen_frames = std::rc::Rc::new(std::cell::Cell::new(0usize));
        let seen = seen_frames.clone();

        let mut handle = ProcessorHandle::new(Box::new(move |input, output, _out_ch| {
            seen.set(input.len());
            output.fill(1.0);
            true
        }));

        let result = handle.render(1, 1, requested_frames);
        assert!(
            result,
            "render must not panic when frames exceeds RENDER_QUANTUM_FRAMES"
        );
        assert_eq!(
            seen_frames.get(),
            RENDER_QUANTUM_FRAMES,
            "render must clamp frames to RENDER_QUANTUM_FRAMES before invoking the callback"
        );
    }
}
