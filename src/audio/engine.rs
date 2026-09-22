use crate::audio::devices::{
    clamp_channel_count, first_audio_track, get_media_devices, get_user_media_audio,
    resolve_input_channels, settings_channels, stop_media_stream,
};
use crate::audio::params::AudioParams;
use crate::audio::processor::AudioProcessor;
use crate::audio::protocol::MAX_CHANNELS;
use crate::audio::worklet::{create_worklet_node_with_flag, register_audio_worklet};
use crate::audio::regulator::Regulator;
use crate::audio::ring_buffer::RingBuffer;
use crate::audio::shared_ptr::SharedPtr;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioContextOptions, AudioWorkletNode, ChannelInterpretation, MediaStream,
    MediaStreamAudioSourceNode,
};

/// Resolved audio capture constraints, independent of any JS representation.
///
/// This is the pure core of the constraints builder: it normalizes the inputs
/// (an empty/absent device id means "use the default device", so it collapses
/// to `None`) and carries the processing toggles verbatim. Converting to the
/// `getUserMedia` JS object is handled separately by [`AudioConstraints::to_js`]
/// so the resolution logic can be unit-tested natively.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AudioConstraints {
    /// Effective device id constraint. `None` (including when the caller passes
    /// an empty string) selects the browser's default input device.
    device_id: Option<String>,
    auto_gain_control: bool,
    echo_cancellation: bool,
    noise_suppression: bool,
}

impl AudioConstraints {
    /// Resolve raw caller inputs into normalized constraints.
    pub(crate) fn resolve(
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
    ///
    /// Always asks for `channelCount: { ideal: MAX_CHANNELS }` so the browser
    /// opens the device at its native width instead of downmixing: "capture
    /// N input channels" then reliably means the device's first N.
    pub(crate) fn to_js(&self) -> Result<JsValue, JsValue> {
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

        let channel_constraint = js_sys::Object::new();
        js_sys::Reflect::set(
            &channel_constraint,
            &"ideal".into(),
            &JsValue::from_f64(MAX_CHANNELS as f64),
        )?;
        js_sys::Reflect::set(&constraints, &"channelCount".into(), &channel_constraint)?;

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
    /// The output channel count the current `worklet_node` was built with
    /// (`outputChannelCount` is construction-time only, so a change means
    /// rebuilding the node — see `build_and_connect_worklet_node` and its
    /// call from `set_output_device`).
    worklet_output_channels: usize,
    /// The input channel count the current `worklet_node` was built with:
    /// the requested input channels, clamped to what the browser granted.
    /// Preserved across `set_output_device` rebuilds, which don't change the
    /// capture stream.
    worklet_input_channels: usize,
    /// The output channel count requested by the last `start_capture`,
    /// re-clamped against the new sink's `maxChannelCount` whenever
    /// `set_output_device` rebuilds the node.
    requested_output_channels: u32,
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
            worklet_output_channels: 0,
            worklet_input_channels: 0,
            requested_output_channels: 2,
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
    ///
    /// Captures the device's first `input_channels` channels (fewer when the
    /// browser grants fewer) and plays to the output device's first
    /// `output_channels` channels (fewer when the destination supports fewer).
    /// A given `output_device_id` is routed before the worklet is connected,
    /// so playback never starts on the default output.
    #[wasm_bindgen(js_name = startCapture)]
    pub async fn start_capture(
        &mut self,
        device_id: Option<String>,
        output_device_id: Option<String>,
        auto_gain_control: bool,
        echo_cancellation: bool,
        noise_suppression: bool,
        input_channels: u32,
        output_channels: u32,
    ) -> Result<(), JsValue> {
        // Stop any existing capture
        self.stop_capture().await;

        // Get user media with specified device and constraints
        let media_devices = get_media_devices()?;
        let audio_constraints = AudioConstraints::resolve(
            device_id,
            auto_gain_control,
            echo_cancellation,
            noise_suppression,
        )
        .to_js()?;
        let stream = get_user_media_audio(&media_devices, &audio_constraints).await?;

        // The constraints open the device at its native width; the worklet
        // node's Explicit/Discrete `channelCount` then keeps only its first
        // `input_channels` (dropping the rest). When the browser granted
        // fewer than requested (e.g. Chrome forcing mono under echo
        // cancellation) the narrower width is used and the send path's
        // conform fills the wire width.
        let granted = first_audio_track(&stream)
            .map_or(1, |track| resolve_input_channels(settings_channels(&track), None));
        let worklet_input_channels = clamp_channel_count(input_channels.min(granted));

        // Create source node from the stream
        let source_node = self.ctx.create_media_stream_source(&stream)?;
        self.source_node = Some(source_node);
        self.current_stream = Some(stream);

        // Route to the selected sink before anything is connected to the
        // destination: remote audio may already be queued, and
        // `maxChannelCount` below must be the selected device's, not the
        // default output's.
        if output_device_id.is_some() {
            route_output_sink(self.ctx.as_ref(), output_device_id).await?;
        }

        // Configure the destination (explicit/discrete, so the browser never
        // speaker-folds or up-mixes on our behalf — mapping is
        // `map_to_output`'s job) at the requested width, clamped to the device.
        self.requested_output_channels = output_channels;
        let output_channels = self.configure_destination(output_channels)?;
        self.build_and_connect_worklet_node(worklet_input_channels as usize, output_channels as usize)?;

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

    /// Configure the destination explicit / discrete at `requested` channels,
    /// so the browser never speaker-folds or up-mixes on our behalf. Returns
    /// the configured count.
    ///
    /// Clamped to the destination's `maxChannelCount` (a wider `channelCount`
    /// throws) and to [`MAX_CHANNELS`]: the worklet ABI's scratch buffers
    /// (`ProcessorHandle`) and `worklet.js`'s channel views are both fixed at
    /// that width, so a wider interface is addressed at its first
    /// `MAX_CHANNELS` outputs.
    fn configure_destination(&self, requested: u32) -> Result<u32, JsValue> {
        let destination = self.ctx.destination();
        let channels = clamp_channel_count(requested.min(destination.max_channel_count()));
        destination.set_channel_count(channels);
        destination.set_channel_count_mode(web_sys::ChannelCountMode::Explicit);
        destination.set_channel_interpretation(ChannelInterpretation::Discrete);
        Ok(channels)
    }

    /// Build a fresh worklet node at `input_channels` / `output_channels`
    /// wide and connect it: source → worklet → destination, disconnecting
    /// any previous worklet node. `input_channels` is the caller's
    /// responsibility to determine — in practice the requested input width
    /// clamped to what the browser granted (`start_capture`), preserved as
    /// `self.worklet_input_channels` across rebuilds that aren't themselves
    /// changing the capture device.
    ///
    /// Used both by `start_capture` (first build) and `set_output_device`
    /// (rebuild when `setSinkId` changes the destination's max channel
    /// count) — `outputChannelCount`/`channelCount` are construction-time
    /// only on `AudioWorkletNode`, so a width change means a new node, not a
    /// reconfigure.
    fn build_and_connect_worklet_node(&mut self, input_channels: usize, output_channels: usize) -> Result<(), JsValue> {
        // Create processor with network support. `AudioEngine` stores raw
        // pointers (it is a `#[wasm_bindgen]` boundary type); wrap them in
        // `SharedPtr` here so the processor and the flag read below go through
        // the one audited deref.
        let params = unsafe { &*self.params_ptr };
        let local_to_network = SharedPtr::new(self.local_to_network_buffer_ptr);
        let network_to_local = SharedPtr::new(self.network_to_local_buffer_ptr);

        let mut processor = if local_to_network.is_null() && network_to_local.is_null() {
            AudioProcessor::new(params)
        } else {
            AudioProcessor::with_network(params, local_to_network, network_to_local)
        };

        let process = Box::new(move |input: &[f32], in_channels: usize, output: &mut [f32], out_channels: usize| {
            processor.process(input, in_channels, output, out_channels)
        });

        // Get ring buffer flag pointer for event-driven wake-up
        let ring_buffer_flag_ptr = local_to_network
            .as_ref()
            .map(|ring_buffer| ring_buffer.get_has_data_flag_ptr());

        // Create worklet node for processing (with flag pointer for Atomics.notify)
        let worklet_node = create_worklet_node_with_flag(
            &self.ctx,
            process,
            input_channels,
            output_channels,
            ring_buffer_flag_ptr,
        )?;

        // Connect: source -> worklet -> destination
        if let Some(ref source) = self.source_node {
            source.connect_with_audio_node(&worklet_node)?;
        }
        worklet_node.connect_with_audio_node(&self.ctx.destination())?;

        if let Some(ref old_node) = self.worklet_node {
            self.teardown_worklet_node(old_node);
        }

        self.worklet_node = Some(worklet_node);
        self.worklet_input_channels = input_channels;
        self.worklet_output_channels = output_channels;

        Ok(())
    }

    /// Fully detach `node` from the audio graph: post `"stop"` on its port
    /// (see `worklet.js`) so its processor's `process()` returns `false` once
    /// the message is delivered, and disconnect it in both directions.
    ///
    /// Both steps matter. Per the Web Audio spec, an `AudioWorkletNode` whose
    /// `process()` last returned `true` keeps being invoked on the render
    /// thread even with no output connections — `node.disconnect()` alone
    /// only drops outgoing connections, so a node built by
    /// `build_and_connect_worklet_node` and later replaced (e.g. by
    /// `set_output_device`'s rebuild) would otherwise linger as an "active
    /// processing" node: still popping the shared [`Regulator`] and writing
    /// the send ring buffer every callback alongside its replacement.
    ///
    /// [`Regulator`]: crate::audio::regulator::Regulator
    fn teardown_worklet_node(&self, node: &AudioWorkletNode) {
        if let Ok(port) = node.port() {
            let _ = port.post_message(&JsValue::from_str("stop"));
        }
        if let Some(ref source) = self.source_node {
            let _ = source.disconnect_with_audio_node(node);
        }
        let _ = node.disconnect();
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
    /// Fully stop capture: suspend the AudioContext (so the worklet render
    /// thread cannot run another `process()`), tear down the worklet node,
    /// and stop media tracks.
    ///
    /// Suspending before teardown is load-bearing for [`Regulator::reset`]:
    /// posting `"stop"` alone is asynchronous, and a concurrent `pop` racing
    /// `reset` is a data race (WEB-53 Bug 7). `AudioContext.suspend()` waits
    /// until the render quantum has stopped.
    #[wasm_bindgen(js_name = stopCapture)]
    pub async fn stop_capture(&mut self) {
        // Quiesce the render thread before dropping worklet / shared buffers.
        if let Ok(suspend) = self.ctx.suspend() {
            let _ = JsFuture::from(suspend).await;
        }
        self.stop_capture_now();
    }

    /// Tear down the worklet and media tracks without waiting for the render
    /// thread. Used from [`Drop`] paths that cannot await; prefer
    /// [`stop_capture`](Self::stop_capture) whenever async is available.
    pub(crate) fn stop_capture_now(&mut self) {
        if let Some(ref node) = self.worklet_node {
            self.teardown_worklet_node(node);
        }

        // Stop all tracks in the stream
        if let Some(ref stream) = self.current_stream {
            stop_media_stream(stream);
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

    /// The number of channels the current playback output is configured
    /// for (the requested output channels clamped to the destination's
    /// `maxChannelCount` and `MAX_CHANNELS` — see `configure_destination`).
    /// `None` until capture has started AND the worklet node has actually
    /// been built; always `>= 1` once known.
    #[wasm_bindgen(js_name = outputChannels)]
    pub fn output_channels(&self) -> Option<u32> {
        (self.worklet_output_channels > 0).then_some(self.worklet_output_channels as u32)
    }

    /// Set the output audio device (sink) for playback
    ///
    /// Uses the AudioContext.setSinkId() API to route audio to a specific device.
    /// Pass an empty string to use the default device.
    ///
    /// `setSinkId` can change `maxChannelCount` (a different physical output
    /// device may support a different channel count). Once routed, this
    /// re-clamps the requested output channels against it and rebuilds the
    /// worklet node at the new width if it changed.
    ///
    /// # Arguments
    /// * `device_id` - The device ID from the output device selector, or empty string for default
    #[wasm_bindgen(js_name = setOutputDevice)]
    pub async fn set_output_device(&mut self, device_id: Option<String>) -> Result<(), JsValue> {
        route_output_sink(self.ctx.as_ref(), device_id).await?;

        if self.worklet_node.is_some() {
            let output_channels = self.configure_destination(self.requested_output_channels)?;
            if output_channels as usize != self.worklet_output_channels {
                self.build_and_connect_worklet_node(self.worklet_input_channels, output_channels as usize)?;
            }
        }

        Ok(())
    }
}

/// Route playback to the requested output sink on `ctx_obj` via `setSinkId`.
///
/// Split out of [`AudioEngine::set_output_device`] so the two branches can be
/// exercised against a synthetic context regardless of whether the *real*
/// `AudioContext` of the running browser happens to expose `setSinkId` (modern
/// Chrome does; older browsers do not):
///
/// - When `setSinkId` is absent it is a graceful no-op — warn and return
///   `Ok(())` rather than erroring (the app falls back to the default sink).
/// - Otherwise it calls `setSinkId(device_id | "")`, where an absent/empty id
///   selects the default device, and awaits the returned promise.
pub(crate) async fn route_output_sink(ctx_obj: &JsValue, device_id: Option<String>) -> Result<(), JsValue> {
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
    fn test_resolve_full_matrix_device_id_x_toggles() {
        // device-id present/absent (incl. empty) × every toggle permutation.
        let device_id_cases: [(Option<String>, Option<&str>); 3] = [
            (Some("dev-1".to_string()), Some("dev-1")),
            (None, None),
            (Some(String::new()), None),
        ];

        for (raw_id, expected_id) in device_id_cases {
            for (agc, ec, ns) in BOOL_PERMUTATIONS {
                let resolved = AudioConstraints::resolve(raw_id.clone(), agc, ec, ns);
                assert_eq!(resolved.device_id.as_deref(), expected_id);
                assert_eq!(resolved.auto_gain_control, agc);
                assert_eq!(resolved.echo_cancellation, ec);
                assert_eq!(resolved.noise_suppression, ns);
            }
        }
    }

    // ── Browser tests (web_sys / Web Audio) ──────────────────────────────────
    //
    // Real-browser coverage of the AudioContext bootstrap, run in headless
    // Chrome via `npm run test:wasm`. The per-binary browser opt-in
    // (`wasm_bindgen_test_configure!(run_in_browser)`) lives once in
    // `crate::test_support`; here we only import the attribute. No user gesture
    // or fake media device is needed: constructing an AudioContext (it starts
    // suspended) and registering the worklet module both work headless.

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;

    /// Build an `AudioEngine` in the browser harness.
    ///
    /// `create` (no network) leaves both buffer pointers null and registers the
    /// worklet module against a fresh `AudioContext`. Shared so every engine
    /// browser test bootstraps the same way instead of re-rolling the
    /// `AudioEngine::create(...).await.expect(...)` dance.
    #[cfg(target_arch = "wasm32")]
    async fn create_engine(params: &AudioParams) -> AudioEngine {
        AudioEngine::create(params as *const AudioParams)
            .await
            .expect("AudioEngine::create should succeed in the browser")
    }

    /// Close an engine's `AudioContext`, releasing its native audio resources.
    ///
    /// All engine browser tests share a single Chrome page, so an unclosed
    /// `AudioContext` (especially one with a live worklet / capture stream)
    /// leaks real audio-thread resources for the rest of the suite. That is
    /// tolerable for the plain `test:wasm` run but, under the much heavier
    /// coverage-instrumented build, the accumulation exhausts the renderer
    /// mid-suite (the driver gets SIGKILLed). Each test that builds an engine
    /// hands it here when done so the context is torn down promptly.
    ///
    /// Both a synchronous `close()` failure and a rejected close promise are
    /// surfaced as hard test failures so a teardown leak can't pass unnoticed.
    #[cfg(target_arch = "wasm32")]
    async fn close_engine(engine: AudioEngine) {
        let promise = engine
            .ctx
            .close()
            .expect("AudioContext.close() should be issuable");
        JsFuture::from(promise)
            .await
            .expect("AudioContext should close without error");
    }

    /// Assert a freshly read JS object carries the three processing toggles
    /// verbatim as booleans. Shared between the device-present / device-absent
    /// `to_js` tests so the toggle-field reads aren't duplicated.
    #[cfg(target_arch = "wasm32")]
    fn assert_toggle_fields(js: &JsValue, agc: bool, ec: bool, ns: bool) {
        for (field, expected) in [
            ("autoGainControl", agc),
            ("echoCancellation", ec),
            ("noiseSuppression", ns),
        ] {
            let value = js_sys::Reflect::get(js, &field.into())
                .unwrap_or_else(|_| panic!("constraints object must expose {field}"))
                .as_bool();
            assert_eq!(
                value,
                Some(expected),
                "{field} must be carried through as {expected}, got {value:?}"
            );
        }
    }

    /// `AudioEngine::create` must build a real `AudioContext` that reports a
    /// plausible, positive sample rate — the bootstrap on the critical path of
    /// every session. `create` also registers the worklet module, so this
    /// additionally smoke-tests that path end to end.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn engine_create_reports_plausible_sample_rate() {
        // `create` stores the params pointer but `get_sample_rate` never
        // dereferences it, so a stack `AudioParams` kept alive for the duration
        // of the test is sufficient.
        let params = AudioParams::default();
        let engine = create_engine(&params).await;

        let sample_rate = engine.get_sample_rate();
        assert!(
            sample_rate > 0.0,
            "AudioContext sample rate must be positive, got {sample_rate}"
        );
        // Bound it well outside any real device rate to catch a bogus
        // (e.g. uninitialized / mis-decoded) value while staying rate-agnostic.
        assert!(
            (8_000.0..=768_000.0).contains(&sample_rate),
            "sample rate {sample_rate} is outside any plausible audio range"
        );

        close_engine(engine).await;
    }

    /// Graceful no-op branch: when the context object has no `setSinkId`,
    /// `route_output_sink` must warn and return `Ok(())` without throwing —
    /// the requested device id is simply ignored (default sink is kept). Driven
    /// against a plain object so the absent-`setSinkId` path is reached even on
    /// browsers (like current Chrome) whose real `AudioContext` does expose it.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn route_output_sink_absent_setsinkid_is_graceful_noop() {
        let ctx_like: JsValue = js_sys::Object::new().into();
        assert!(
            !js_sys::Reflect::has(&ctx_like, &"setSinkId".into()).unwrap(),
            "a plain object must not expose setSinkId (precondition for the no-op branch)"
        );

        route_output_sink(&ctx_like, Some("ignored-device".to_string()))
            .await
            .expect("a context without setSinkId must yield a graceful Ok no-op");
    }

    /// Call path: when the context object exposes `setSinkId`,
    /// `route_output_sink` must invoke it with the device id, and with `""`
    /// when no device is requested (the default-sink case). A stub records the
    /// argument it was called with so we can assert the mapping. Driven against
    /// a synthetic object so this branch is covered regardless of whether the
    /// real `AudioContext` happens to implement `setSinkId`.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn route_output_sink_calls_setsinkid_with_resolved_id() {
        use std::cell::RefCell;
        use std::rc::Rc;
        use wasm_bindgen::closure::Closure;

        let recorded: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        let recorded_cb = recorded.clone();
        // The stub mirrors `AudioContext.setSinkId`: it records its single string
        // argument and resolves, so the awaited promise completes `Ok`.
        let stub = Closure::wrap(Box::new(move |id: JsValue| -> js_sys::Promise {
            *recorded_cb.borrow_mut() = id.as_string();
            js_sys::Promise::resolve(&JsValue::UNDEFINED)
        }) as Box<dyn FnMut(JsValue) -> js_sys::Promise>);

        let ctx_like = js_sys::Object::new();
        js_sys::Reflect::set(&ctx_like, &"setSinkId".into(), stub.as_ref().unchecked_ref())
            .expect("attaching the setSinkId stub should succeed");
        let ctx_like: JsValue = ctx_like.into();

        route_output_sink(&ctx_like, Some("speaker-3".to_string()))
            .await
            .expect("setSinkId stub should resolve Ok");
        assert_eq!(
            recorded.borrow().as_deref(),
            Some("speaker-3"),
            "a specific device id must be forwarded verbatim to setSinkId"
        );

        route_output_sink(&ctx_like, None)
            .await
            .expect("default-sink request should resolve Ok");
        assert_eq!(
            recorded.borrow().as_deref(),
            Some(""),
            "an absent device id must select the default sink via setSinkId(\"\")"
        );

        drop(stub);
    }

    /// `get_worklet_port` / `is_capturing` must track the worklet node's
    /// presence across the capture lifecycle.
    ///
    /// Before capture there is no worklet node: `is_capturing()` is `false` and
    /// `get_worklet_port()` is `None`. After `start_capture` (over the no-network
    /// `create` path — both buffer pointers null, so the processor takes its
    /// non-networked branch and the worklet gets no ring-buffer flag) a worklet
    /// node exists, so `is_capturing()` flips to `true` and `get_worklet_port()`
    /// returns `Some(MessagePort)`. `stop_capture` must tear that back down to
    /// the absent state. Capture works headless thanks to the fake-device flags
    /// in `webdriver.json`.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn worklet_port_tracks_capture_lifecycle() {
        // `start_capture` dereferences the params pointer, so `params` must
        // outlive the engine — declare it first so it drops last.
        let params = AudioParams::default();
        let mut engine = create_engine(&params).await;

        assert!(
            !engine.is_capturing(),
            "a fresh engine must not report capturing"
        );
        assert!(
            engine.get_worklet_port().is_none(),
            "get_worklet_port must be None before capture starts"
        );

        engine
            .start_capture(None, None, false, false, false, 2, 2)
            .await
            .expect("start_capture should resolve with the fake-device flags");

        assert!(
            engine.is_capturing(),
            "is_capturing must report true once a worklet node exists"
        );
        assert!(
            engine.get_worklet_port().is_some(),
            "get_worklet_port must return Some(MessagePort) while capturing"
        );

        engine.stop_capture().await;

        assert!(
            !engine.is_capturing(),
            "is_capturing must report false after stop_capture"
        );
        assert!(
            engine.get_worklet_port().is_none(),
            "get_worklet_port must be None after stop_capture"
        );

        close_engine(engine).await;
    }

    /// Wiring buffer pointers via the setters must route a later `start_capture`
    /// through the *networked* processor branch.
    ///
    /// An engine built with `create` starts with null buffer pointers;
    /// `set_local_to_network_buffer` / `set_network_to_local_buffer` install
    /// real buffers after the fact, so `start_capture` takes the
    /// `AudioProcessor::with_network` branch and hands the worklet the ring
    /// buffer's has-data flag.
    ///
    /// To prove the networked branch was actually taken (and not just that
    /// capture came up — `is_capturing` / `get_worklet_port` are identical on
    /// both branches), we assert a **network-only side effect**: with streaming
    /// enabled, the running worklet's networked send path writes the captured
    /// fake-device tone into the very ring buffer wired via the setter, so
    /// `samples_written` climbs above zero. The non-network processor
    /// (`AudioProcessor::new`) holds no ring buffer and could never produce this
    /// write, so a non-zero count is unique to the networked path fed by the
    /// setters.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn buffer_setters_feed_capture_path() {
        use crate::test_support::wait_until;

        // All three backing allocations must outlive the engine, which holds raw
        // pointers into them — declare them before the engine so they drop last.
        let params = AudioParams::default();
        let mut ring = RingBuffer::new();
        let mut regulator = Regulator::new();
        let mut engine = create_engine(&params).await;

        engine.set_local_to_network_buffer(&mut ring as *mut RingBuffer);
        engine.set_network_to_local_buffer(&mut regulator as *mut Regulator);

        // The networked send path is a no-op unless the ring buffer is
        // streaming, so enable it before the worklet starts producing.
        ring.set_streaming(true);

        engine
            .start_capture(None, None, false, false, false, 2, 2)
            .await
            .expect("start_capture over the networked branch should resolve");

        assert!(
            engine.is_capturing(),
            "capture must come up after wiring buffers via the setters"
        );
        assert!(
            engine.get_worklet_port().is_some(),
            "networked capture must still expose a worklet message port"
        );

        // Resume the graph so the worklet actually runs process() callbacks
        // (start_capture only fires resume in the background). The fake input
        // device emits a synthetic tone the networked processor then forwards.
        engine
            .resume_ctx()
            .await
            .expect("resuming the AudioContext should succeed");

        let wrote_to_ring = wait_until(
            || ring.samples_written() > 0,
            /* timeout_ms */ 3000,
            /* interval_ms */ 10,
        )
        .await;
        assert!(
            wrote_to_ring,
            "the networked worklet must write captured audio into the ring buffer \
             wired via set_local_to_network_buffer (samples_written stayed 0)"
        );

        engine.stop_capture().await;
        close_engine(engine).await;
    }

    /// `AudioConstraints::to_js` must emit an `exact` device-id constraint when a
    /// specific device is requested, alongside the three processing toggles, and
    /// always an `ideal: MAX_CHANNELS` channelCount constraint (so the device
    /// opens at its native width rather than downmixed).
    ///
    /// This is the optional-config branch of the constraints builder (the
    /// natively-tested `resolve` only produces the plain struct); the JS-object
    /// conversion is browser glue, so the device-id / channel-count paths are
    /// exercised here.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn to_js_emits_exact_device_id() {
        let js = AudioConstraints::resolve(Some("mic-7".to_string()), true, false, true)
            .to_js()
            .expect("to_js must build the getUserMedia constraints object");

        let device_id = js_sys::Reflect::get(&js, &"deviceId".into())
            .expect("constraints must expose a deviceId field for a specific device");
        let exact = js_sys::Reflect::get(&device_id, &"exact".into())
            .expect("deviceId must carry an exact constraint")
            .as_string();
        assert_eq!(
            exact.as_deref(),
            Some("mic-7"),
            "deviceId.exact must equal the requested device id"
        );

        let channel_count = js_sys::Reflect::get(&js, &"channelCount".into())
            .expect("constraints must expose a channelCount field");
        let ideal = js_sys::Reflect::get(&channel_count, &"ideal".into())
            .expect("channelCount must carry an ideal constraint")
            .as_f64();
        assert_eq!(
            ideal,
            Some(MAX_CHANNELS as f64),
            "channelCount.ideal must ask for MAX_CHANNELS"
        );

        assert_toggle_fields(&js, true, false, true);
    }

    /// `AudioConstraints::to_js` must omit the device-id constraint entirely
    /// for the default-device path, while still carrying the processing
    /// toggles — the complementary branch to `to_js_emits_exact_device_id`.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn to_js_omits_device_id_for_default() {
        let js = AudioConstraints::resolve(None, false, true, false)
            .to_js()
            .expect("to_js must build the getUserMedia constraints object");

        assert!(
            !js_sys::Reflect::has(&js, &"deviceId".into())
                .expect("Reflect::has must succeed on the constraints object"),
            "the default-device path must not set a deviceId constraint"
        );

        assert_toggle_fields(&js, false, true, false);
    }
}
