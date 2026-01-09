use crate::audio::devices::{get_media_devices, stop_media_stream};
use crate::audio::params::AudioParams;
use crate::audio::processor::AudioProcessor;
use crate::audio::worklet::{create_worklet_node, register_audio_worklet};
use crate::audio::jitter_buffer::LockFreeJitterBuffer;
use crate::audio::ring_buffer::RingBuffer;
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
    local_to_network_buffer_ptr: *mut RingBuffer,
    network_to_local_buffer_ptr: *const LockFreeJitterBuffer,
}

#[wasm_bindgen]
impl AudioEngine {
    /// Create a new audio engine (without network support)
    #[wasm_bindgen(js_name = create)]
    pub async fn create(params_ptr: *const AudioParams) -> Result<AudioEngine, JsValue> {
        Self::create_with_network(params_ptr, std::ptr::null_mut(), std::ptr::null()).await
    }

    /// Create a new audio engine with network audio support
    /// - local_to_network_buffer_ptr: ring buffer for sending local audio to network
    /// - network_to_local_buffer_ptr: jitter buffer for receiving audio from network
    #[wasm_bindgen(js_name = createWithNetwork)]
    pub async fn create_with_network(
        params_ptr: *const AudioParams,
        local_to_network_buffer_ptr: *mut RingBuffer,
        network_to_local_buffer_ptr: *const LockFreeJitterBuffer,
    ) -> Result<AudioEngine, JsValue> {
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
            local_to_network_buffer_ptr,
            network_to_local_buffer_ptr,
        })
    }

    /// Get the AudioContext sample rate
    #[wasm_bindgen(js_name = getSampleRate)]
    pub fn get_sample_rate(&self) -> f32 {
        self.ctx.sample_rate()
    }

    /// Set the local-to-network ring buffer pointer
    #[wasm_bindgen(js_name = setLocalToNetworkBuffer)]
    pub fn set_local_to_network_buffer(&mut self, ptr: *mut RingBuffer) {
        self.local_to_network_buffer_ptr = ptr;
    }

    /// Set the network-to-local jitter buffer pointer
    #[wasm_bindgen(js_name = setNetworkToLocalBuffer)]
    pub fn set_network_to_local_buffer(&mut self, ptr: *const LockFreeJitterBuffer) {
        self.network_to_local_buffer_ptr = ptr;
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

        // Create processor with network support
        let params = unsafe { &*self.params_ptr };
        let local_to_network_ptr = self.local_to_network_buffer_ptr;
        let network_to_local_ptr = self.network_to_local_buffer_ptr;
        
        let mut processor = if local_to_network_ptr.is_null() && network_to_local_ptr.is_null() {
            AudioProcessor::new(params)
        } else {
            AudioProcessor::with_network(params, local_to_network_ptr, network_to_local_ptr)
        };

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

    /// Check if audio is currently being captured
    #[wasm_bindgen(js_name = isCapturing)]
    pub fn is_capturing(&self) -> bool {
        self.worklet_node.is_some()
    }

    /// Get the worklet node's message port for event-driven audio processing
    /// 
    /// The worklet posts 'audio-ready' messages after each process() call,
    /// allowing the network loop to wake immediately when audio data is available
    /// instead of polling at a fixed interval.
    #[wasm_bindgen(js_name = getWorkletPort)]
    pub fn get_worklet_port(&self) -> Option<web_sys::MessagePort> {
        self.worklet_node.as_ref().and_then(|node| node.port().ok())
    }

    /// Set the output audio device (sink) for playback
    /// 
    /// Uses the AudioContext.setSinkId() API to route audio to a specific device.
    /// Pass an empty string to use the default device.
    /// 
    /// # Arguments
    /// * `device_id` - The device ID from the output device selector, or empty string for default
    #[wasm_bindgen(js_name = setOutputDevice)]
    pub async fn set_output_device(&self, device_id: Option<String>) -> Result<(), JsValue> {
        let ctx_obj: &JsValue = self.ctx.as_ref();
        
        // Check if setSinkId is available
        let has_set_sink_id = js_sys::Reflect::has(ctx_obj, &JsValue::from_str("setSinkId"))?;
        
        if !has_set_sink_id {
            web_sys::console::warn_1(&"setSinkId not supported in this browser, using default output device".into());
            return Ok(());
        }

        // Call setSinkId with the device ID or empty string for default
        let sink_id = device_id.unwrap_or_default();
        let set_sink_id_fn = js_sys::Reflect::get(ctx_obj, &JsValue::from_str("setSinkId"))?
            .dyn_into::<js_sys::Function>()?;
        
        let promise = set_sink_id_fn.call1(ctx_obj, &JsValue::from_str(&sink_id))?;
        JsFuture::from(js_sys::Promise::from(promise)).await?;
        
        Ok(())
    }
}
