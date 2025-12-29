use crate::audio_devices::{get_media_devices, stop_media_stream};
use crate::audio_params::AudioParams;
use crate::audio_processor::AudioProcessor;
use crate::audio_worklet::{create_worklet_node, register_audio_worklet};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioContextOptions, AudioWorkletNode, MediaStream,
    MediaStreamAudioSourceNode, MediaStreamConstraints,
};

/// Helper for building audio constraints with device ID and processing options
struct AudioConstraintsBuilder {
    device_id: Option<String>,
    auto_gain_control: bool,
    echo_cancellation: bool,
    noise_suppression: bool,
}

impl AudioConstraintsBuilder {
    fn new(
        device_id: Option<String>,
        auto_gain_control: bool,
        echo_cancellation: bool,
        noise_suppression: bool,
    ) -> Self {
        Self {
            device_id,
            auto_gain_control,
            echo_cancellation,
            noise_suppression,
        }
    }

    fn build(self) -> Result<JsValue, JsValue> {
        let constraints = js_sys::Object::new();

        // Set device ID if specified
        if let Some(id) = self.device_id.filter(|s| !s.is_empty()) {
            let exact_constraint = js_sys::Object::new();
            js_sys::Reflect::set(&exact_constraint, &"exact".into(), &JsValue::from_str(&id))?;
            js_sys::Reflect::set(&constraints, &"deviceId".into(), &exact_constraint)?;
        }

        // Set audio processing options
        js_sys::Reflect::set(
            &constraints,
            &"autoGainControl".into(),
            &JsValue::from_bool(self.auto_gain_control),
        )?;
        js_sys::Reflect::set(
            &constraints,
            &"echoCancellation".into(),
            &JsValue::from_bool(self.echo_cancellation),
        )?;
        js_sys::Reflect::set(
            &constraints,
            &"noiseSuppression".into(),
            &JsValue::from_bool(self.noise_suppression),
        )?;

        Ok(constraints.into())
    }
}

/// Audio engine with capture and playback capabilities
#[wasm_bindgen]
pub struct AudioEngine {
    ctx: AudioContext,
    worklet_node: Option<AudioWorkletNode>,
    source_node: Option<MediaStreamAudioSourceNode>,
    current_stream: Option<MediaStream>,
    params_ptr: *const AudioParams,
}

#[wasm_bindgen]
impl AudioEngine {
    /// Create a new audio engine
    #[wasm_bindgen(js_name = create)]
    pub async fn create(params_ptr: *const AudioParams) -> Result<AudioEngine, JsValue> {
        // Configure AudioContext with minimal latency
        let options = AudioContextOptions::new();
        options.set_latency_hint(&JsValue::from(0));

        let ctx = AudioContext::new_with_context_options(&options)?;
        register_audio_worklet(&ctx).await?;

        Ok(Self {
            ctx,
            worklet_node: None,
            source_node: None,
            current_stream: None,
            params_ptr,
        })
    }

    /// Get the AudioContext sample rate
    #[wasm_bindgen(js_name = getSampleRate)]
    pub fn get_sample_rate(&self) -> f32 {
        self.ctx.sample_rate()
    }

    /// Start audio capture from the specified input device
    #[wasm_bindgen(js_name = startCapture)]
    pub async fn start_capture(
        &mut self,
        device_id: Option<String>,
        auto_gain_control: bool,
        echo_cancellation: bool,
        noise_suppression: bool,
    ) -> Result<(), JsValue> {
        // Stop any existing capture
        self.stop_capture();

        // Get user media with specified device and constraints
        let media_devices = get_media_devices()?;
        let constraints = MediaStreamConstraints::new();

        // Build audio constraints
        let audio_constraints = AudioConstraintsBuilder::new(
            device_id,
            auto_gain_control,
            echo_cancellation,
            noise_suppression,
        )
        .build()?;

        constraints.set_audio(&audio_constraints);
        constraints.set_video(&JsValue::from(false));

        let stream_promise = media_devices.get_user_media_with_constraints(&constraints)?;
        let stream: MediaStream = JsFuture::from(stream_promise).await?.unchecked_into();

        // Create source node from the stream
        let source_node = self.ctx.create_media_stream_source(&stream)?;

        // Create processor once and reuse it for all audio callbacks
        let params = unsafe { &*self.params_ptr };
        let mut processor = AudioProcessor::new(params);

        let process = Box::new(move |input: &[f32], output: &mut [f32]| {
            processor.process(input, output)
        });

        // Create worklet node for processing
        let worklet_node = create_worklet_node(&self.ctx, process)?;

        // Connect: source -> worklet -> destination
        source_node.connect_with_audio_node(&worklet_node)?;
        worklet_node.connect_with_audio_node(&self.ctx.destination())?;

        self.source_node = Some(source_node);
        self.worklet_node = Some(worklet_node);
        self.current_stream = Some(stream);

        // Resume the audio context
        JsFuture::from(self.ctx.resume()?).await?;

        Ok(())
    }

    /// Stop audio capture
    #[wasm_bindgen(js_name = stopCapture)]
    pub fn stop_capture(&mut self) {
        // Signal the worklet to stop processing
        if let Some(ref node) = self.worklet_node {
            if let Ok(port) = node.port() {
                let _ = port.post_message(&JsValue::from_str("stop"));
            }
        }

        // Stop all tracks in the stream
        if let Some(ref stream) = self.current_stream {
            stop_media_stream(stream);
        }

        // Disconnect nodes
        if let Some(ref node) = self.source_node {
            let _ = node.disconnect();
        }
        if let Some(ref node) = self.worklet_node {
            let _ = node.disconnect();
        }

        self.source_node = None;
        self.worklet_node = None;
        self.current_stream = None;
    }
}

