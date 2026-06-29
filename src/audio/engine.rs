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
        route_output_sink(self.ctx.as_ref(), device_id).await
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
async fn route_output_sink(ctx_obj: &JsValue, device_id: Option<String>) -> Result<(), JsValue> {
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
    #[cfg(target_arch = "wasm32")]
    async fn close_engine(engine: AudioEngine) {
        if let Ok(promise) = engine.ctx.close() {
            let _ = JsFuture::from(promise).await;
        }
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

    /// End-to-end: `set_output_device` must resolve `Ok` for the default-sink
    /// request against a real `AudioContext`.
    ///
    /// `None` and an empty string both normalize to the default device (sink id
    /// `""`). Depending on the browser the real context either lacks `setSinkId`
    /// (graceful warn + `Ok(())`) or exposes it (resolves via `setSinkId("")`);
    /// either way the entry point must not throw. Both branches are pinned down
    /// deterministically — independent of this browser's capabilities — by the
    /// `route_output_sink_*` tests below.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn set_output_device_default_resolves_ok() {
        let params = AudioParams::default();
        let engine = create_engine(&params).await;

        engine
            .set_output_device(None)
            .await
            .expect("set_output_device(None) must resolve Ok (graceful no-op or setSinkId default)");
        engine
            .set_output_device(Some(String::new()))
            .await
            .expect("set_output_device(\"\") must resolve Ok for the default sink");

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
            .start_capture(None, false, false, false)
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

        engine.stop_capture();

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
    /// buffer's has-data flag. We assert capture still comes up (worklet port
    /// present), proving the setters fed live pointers into the capture path.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn buffer_setters_feed_capture_path() {
        // All three backing allocations must outlive the engine, which holds raw
        // pointers into them — declare them before the engine so they drop last.
        let params = AudioParams::default();
        let mut ring = RingBuffer::new();
        let mut regulator = Regulator::new();
        let mut engine = create_engine(&params).await;

        engine.set_local_to_network_buffer(&mut ring as *mut RingBuffer);
        engine.set_network_to_local_buffer(&mut regulator as *mut Regulator);

        engine
            .start_capture(None, false, false, false)
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

        engine.stop_capture();
        close_engine(engine).await;
    }

    /// `AudioConstraints::to_js` must emit an `exact` device-id constraint when a
    /// specific device is requested, alongside the three processing toggles.
    ///
    /// This is the optional-config branch of the constraints builder (the
    /// natively-tested `resolve` only produces the plain struct); the JS-object
    /// conversion is browser glue, so the device-id path is exercised here.
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

        assert_toggle_fields(&js, true, false, true);
    }

    /// `AudioConstraints::to_js` must omit the device-id constraint entirely when
    /// no device is requested (the default-device path), while still carrying the
    /// processing toggles — the complementary branch to `to_js_emits_exact_device_id`.
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
