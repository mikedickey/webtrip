use crate::audio::devices::{get_media_devices, stop_media_stream};
use crate::audio::params::AudioParams;
use crate::audio::processor::AudioProcessor;
use crate::audio::worklet::{create_worklet_node_with_flag, register_audio_worklet};
use crate::audio::regulator::Regulator;
use crate::audio::ring_buffer::RingBuffer;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioContextOptions, AudioWorkletNode, MediaStream,
    MediaStreamAudioSourceNode, MediaStreamConstraints,
};

/// Resolved audio capture constraints, independent of any JS representation.
///
/// This is the pure core of the constraints builder: it normalizes the inputs
/// (an empty/absent device id means "use the default device", so it collapses
/// to `None`) and carries the processing toggles verbatim. Converting to the
/// `getUserMedia` JS object is handled separately by [`AudioConstraints::to_js`]
/// so the resolution logic can be unit-tested natively.
#[derive(Clone, Debug, PartialEq, Eq)]
struct AudioConstraints {
    /// Effective device id constraint. `None` (including when the caller passes
    /// an empty string) selects the browser's default input device.
    device_id: Option<String>,
    auto_gain_control: bool,
    echo_cancellation: bool,
    noise_suppression: bool,
}

impl AudioConstraints {
    /// Resolve raw caller inputs into normalized constraints.
    fn resolve(
        device_id: Option<String>,
        auto_gain_control: bool,
        echo_cancellation: bool,
        noise_suppression: bool,
    ) -> Self {
        Self {
            device_id: device_id.filter(|s| !s.is_empty()),
            auto_gain_control,
            echo_cancellation,
            noise_suppression,
        }
    }

    /// Build the `getUserMedia` audio-constraints JS object from the resolved
    /// constraints. Browser glue only — not exercised by native tests.
    fn to_js(&self) -> Result<JsValue, JsValue> {
        let constraints = js_sys::Object::new();

        // Set device ID if a specific device was requested
        if let Some(id) = &self.device_id {
            let exact_constraint = js_sys::Object::new();
            js_sys::Reflect::set(&exact_constraint, &"exact".into(), &JsValue::from_str(id))?;
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
    network_to_local_buffer_ptr: *mut Regulator,
}

#[wasm_bindgen]
impl AudioEngine {
    /// Create a new audio engine (without network support)
    #[wasm_bindgen(js_name = create)]
    pub async fn create(params_ptr: *const AudioParams) -> Result<AudioEngine, JsValue> {
        Self::create_with_network(params_ptr, std::ptr::null_mut(), std::ptr::null_mut()).await
    }

    /// Create a new audio engine with network audio support
    /// - local_to_network_buffer_ptr: ring buffer for sending local audio to network
    /// - network_to_local_buffer_ptr: jitter buffer for receiving audio from network
    #[wasm_bindgen(js_name = createWithNetwork)]
    pub async fn create_with_network(
        params_ptr: *const AudioParams,
        local_to_network_buffer_ptr: *mut RingBuffer,
        network_to_local_buffer_ptr: *mut Regulator,
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
    pub fn set_network_to_local_buffer(&mut self, ptr: *mut Regulator) {
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
        let audio_constraints = AudioConstraints::resolve(
            device_id,
            auto_gain_control,
            echo_cancellation,
            noise_suppression,
        )
        .to_js()?;

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

        // Get ring buffer flag pointer for event-driven wake-up
        let ring_buffer_flag_ptr = if !local_to_network_ptr.is_null() {
            unsafe {
                let ring_buffer = &*local_to_network_ptr;
                Some(ring_buffer.get_has_data_flag_ptr())
            }
        } else {
            None
        };

        // Create worklet node for processing (with flag pointer for Atomics.notify)
        let worklet_node = create_worklet_node_with_flag(&self.ctx, process, ring_buffer_flag_ptr)?;

        // Connect: source -> worklet -> destination
        source_node.connect_with_audio_node(&worklet_node)?;
        worklet_node.connect_with_audio_node(&self.ctx.destination())?;

        self.source_node = Some(source_node);
        self.worklet_node = Some(worklet_node);
        self.current_stream = Some(stream);

        // Resume the audio context.
        // On iOS Safari, AudioContext.resume() returns a promise that *never* resolves when
        // called outside an active user-gesture context (which expires ~5 s after a tap).
        // Awaiting it would hang the entire connect flow. Instead we fire it as a background
        // task; the context will resume either immediately (iOS 16+ where getUserMedia acts as
        // an implicit unlock) or on the next user interaction (older iOS via resumeCtx()).
        let resume_promise = self.ctx.resume()?;
        wasm_bindgen_futures::spawn_local(async move {
            let _ = JsFuture::from(resume_promise).await;
        });

        Ok(())
    }

    /// Check whether the AudioContext is still suspended (e.g. waiting for a user gesture on iOS).
    #[wasm_bindgen(js_name = isSuspended)]
    pub fn is_suspended(&self) -> bool {
        // Read the `state` property via JS reflection to avoid web_sys enum binding issues.
        let ctx_js: &JsValue = self.ctx.as_ref();
        js_sys::Reflect::get(ctx_js, &JsValue::from_str("state"))
            .ok()
            .and_then(|v| v.as_string())
            .map(|s| s == "suspended")
            .unwrap_or(false)
    }

    /// Explicitly resume the AudioContext.
    ///
    /// Must be called from within a synchronous user-gesture handler on iOS Safari so that the
    /// browser grants the audio-output activation.  Exposed so the TypeScript layer can wire a
    /// "Tap to enable audio" button after the connection is established.
    #[wasm_bindgen(js_name = resumeCtx)]
    pub async fn resume_ctx(&self) -> Result<(), JsValue> {
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

// ==============================================================================
// Tests
// ==============================================================================
//
// These run on the native target via `npm run test`. They cover the pure
// constraints core (`AudioConstraints::resolve`), which produces a plain data
// structure and needs no browser / `web_sys` runtime. The JS-object conversion
// (`AudioConstraints::to_js`) and the rest of the engine (`AudioContext`,
// `getUserMedia`, worklet wiring) are browser glue, left to the WASM tests.
#[cfg(test)]
mod tests {
    use super::*;

    /// Every combination of the three processing toggles, as
    /// (auto_gain_control, echo_cancellation, noise_suppression).
    const BOOL_PERMUTATIONS: [(bool, bool, bool); 8] = [
        (false, false, false),
        (false, false, true),
        (false, true, false),
        (false, true, true),
        (true, false, false),
        (true, false, true),
        (true, true, false),
        (true, true, true),
    ];

    #[test]
    fn test_resolve_carries_all_toggle_permutations() {
        // The device id is fixed here; the focus is that each boolean toggle is
        // carried through verbatim for every permutation.
        for (agc, ec, ns) in BOOL_PERMUTATIONS {
            let resolved =
                AudioConstraints::resolve(Some("dev-1".to_string()), agc, ec, ns);
            assert_eq!(
                resolved,
                AudioConstraints {
                    device_id: Some("dev-1".to_string()),
                    auto_gain_control: agc,
                    echo_cancellation: ec,
                    noise_suppression: ns,
                },
                "toggles must pass through for agc={agc} ec={ec} ns={ns}"
            );
        }
    }

    #[test]
    fn test_resolve_device_id_present() {
        let resolved =
            AudioConstraints::resolve(Some("mic-42".to_string()), false, false, false);
        assert_eq!(resolved.device_id.as_deref(), Some("mic-42"));
    }

    #[test]
    fn test_resolve_device_id_absent() {
        // `None` means "use the browser default device".
        let resolved = AudioConstraints::resolve(None, false, false, false);
        assert_eq!(resolved.device_id, None);
    }

    #[test]
    fn test_resolve_empty_device_id_normalizes_to_none() {
        // An empty string is treated the same as no device id: default device.
        let resolved =
            AudioConstraints::resolve(Some(String::new()), true, true, true);
        assert_eq!(resolved.device_id, None);
        // Toggles are unaffected by device-id normalization.
        assert!(resolved.auto_gain_control);
        assert!(resolved.echo_cancellation);
        assert!(resolved.noise_suppression);
    }

    #[test]
    fn test_resolve_full_matrix_device_id_x_toggles() {
        // device-id present/absent (incl. empty) × every toggle permutation.
        let device_id_cases: [(Option<String>, Option<&str>); 3] = [
            (Some("dev-1".to_string()), Some("dev-1")),
            (None, None),
            (Some(String::new()), None),
        ];

        for (raw_id, expected_id) in device_id_cases {
            for (agc, ec, ns) in BOOL_PERMUTATIONS {
                let resolved =
                    AudioConstraints::resolve(raw_id.clone(), agc, ec, ns);
                assert_eq!(resolved.device_id.as_deref(), expected_id);
                assert_eq!(resolved.auto_gain_control, agc);
                assert_eq!(resolved.echo_cancellation, ec);
                assert_eq!(resolved.noise_suppression, ns);
            }
        }
    }
}
