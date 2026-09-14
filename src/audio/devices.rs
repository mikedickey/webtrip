use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, MediaDeviceInfo, MediaDeviceKind, MediaDevices, MediaStream,
    MediaStreamConstraints, MediaStreamTrack,
};

use crate::audio::engine::{route_output_sink, AudioConstraints};
use crate::audio::protocol::MAX_CHANNELS;

/// Device information
#[wasm_bindgen]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    device_id: String,
    label: String,
}

#[wasm_bindgen]
impl DeviceInfo {
    #[wasm_bindgen(getter, js_name = deviceId)]
    pub fn device_id(&self) -> String {
        self.device_id.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn label(&self) -> String {
        self.label.clone()
    }
}

/// Plain (non-JS) audio device kind used by the pure categorization core.
///
/// Mirrors the subset of `web_sys::MediaDeviceKind` that matters for routing:
/// audio inputs and outputs are kept; anything else (e.g. video inputs) is
/// discarded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioDeviceKind {
    Input,
    Output,
    Other,
}

/// Plain device descriptor consumed by the pure categorization core.
///
/// This decouples categorization from `web_sys::MediaDeviceInfo` so the logic
/// can be unit-tested natively without a browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawDevice {
    pub device_id: String,
    pub label: String,
    pub kind: AudioDeviceKind,
}

/// Pure core: split a list of devices into (inputs, outputs), preserving order
/// and mapping each device's id/label. Non-audio devices are dropped.
fn categorize_devices_core(devices: &[RawDevice]) -> (Vec<DeviceInfo>, Vec<DeviceInfo>) {
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();

    for device in devices {
        let info = DeviceInfo {
            device_id: device.device_id.clone(),
            label: device.label.clone(),
        };
        match device.kind {
            AudioDeviceKind::Input => inputs.push(info),
            AudioDeviceKind::Output => outputs.push(info),
            AudioDeviceKind::Other => {}
        }
    }

    (inputs, outputs)
}

/// Get the MediaDevices API from the browser
pub fn get_media_devices() -> Result<MediaDevices, JsValue> {
    let window = web_sys::window()
        .ok_or_else(|| JsValue::from_str("Window not available"))?;
    let navigator = window.navigator();
    navigator
        .media_devices()
        .map_err(|_| JsValue::from_str("MediaDevices API not available"))
}

/// Stop all tracks in a media stream
pub fn stop_media_stream(stream: &MediaStream) {
    let tracks = stream.get_tracks();
    for i in 0..tracks.length() {
        if let Some(track) = tracks.get(i).dyn_into::<web_sys::MediaStreamTrack>().ok() {
            track.stop();
        }
    }
}

/// Open an audio-only capture stream. `audio` is the `getUserMedia` audio
/// constraint: `true` for the browser defaults, or a constraints object
/// (see [`AudioConstraints::to_js`]).
pub(crate) async fn get_user_media_audio(
    media_devices: &MediaDevices,
    audio: &JsValue,
) -> Result<MediaStream, JsValue> {
    let constraints = MediaStreamConstraints::new();
    constraints.set_audio(audio);
    constraints.set_video(&JsValue::from(false));

    let stream_promise = media_devices.get_user_media_with_constraints(&constraints)?;
    Ok(JsFuture::from(stream_promise).await?.unchecked_into())
}

/// Request audio permission by getting and immediately stopping a stream
async fn request_audio_permission(media_devices: &MediaDevices) -> Result<(), JsValue> {
    let stream = get_user_media_audio(media_devices, &JsValue::from(true)).await?;
    stop_media_stream(&stream);
    Ok(())
}

/// Clamp a device-reported channel count to the supported range
/// `[1, MAX_CHANNELS]`.
pub(crate) fn clamp_channel_count(channels: u32) -> u32 {
    channels.clamp(1, MAX_CHANNELS as u32)
}

/// Resolve an input device's channel count from what its capture track
/// reports: the capability maximum when known, otherwise the granted
/// settings value, otherwise 1 — clamped to `[1, MAX_CHANNELS]`.
pub(crate) fn resolve_input_channels(settings: Option<u32>, capability_max: Option<u32>) -> u32 {
    clamp_channel_count(capability_max.or(settings).unwrap_or(1))
}

/// The first audio track of `stream`, if any.
pub(crate) fn first_audio_track(stream: &MediaStream) -> Option<MediaStreamTrack> {
    stream.get_audio_tracks().get(0).dyn_into::<MediaStreamTrack>().ok()
}

/// The channel count the browser granted `track`
/// (`getSettings().channelCount`), if reported.
pub(crate) fn settings_channels(track: &MediaStreamTrack) -> Option<u32> {
    track.get_settings().get_channel_count().map(|c| c.max(0) as u32)
}

/// The maximum channel count `track`'s device supports
/// (`getCapabilities().channelCount.max`), if reported.
fn capability_max_channels(track: &MediaStreamTrack) -> Result<Option<u32>, JsValue> {
    // Some browsers (notably Safari) don't implement `getCapabilities()` on
    // `MediaStreamTrack` at all — calling the unguarded web-sys binding would
    // throw a JS TypeError, so probe for the method first, the same way
    // `route_output_sink` probes for `setSinkId`.
    if !js_sys::Reflect::has(track, &JsValue::from_str("getCapabilities"))? {
        return Ok(None);
    }
    Ok(track
        .get_capabilities()
        .get_channel_count()
        .and_then(|range| range.get_max()))
}

/// Query how many channels an input device has, without connecting.
///
/// Opens the device (the default input when `deviceId` is absent or empty)
/// with processing off and the native-width channel request, reads the
/// capture track's channel capability, and stops the stream. Resolves to a
/// count in `[1, MAX_CHANNELS]`.
///
/// A module-level function rather than a session method, so it can run while
/// a `connectToStudio` call holds the session borrowed.
#[wasm_bindgen(js_name = getInputDeviceChannels)]
pub async fn get_input_device_channels(device_id: Option<String>) -> Result<u32, JsValue> {
    let media_devices = get_media_devices()?;
    let constraints = AudioConstraints::resolve(device_id, false, false, false).to_js()?;
    let stream = get_user_media_audio(&media_devices, &constraints).await?;

    let channels = match first_audio_track(&stream) {
        Some(track) => capability_max_channels(&track)
            .map(|max| resolve_input_channels(settings_channels(&track), max)),
        None => Ok(1),
    };

    stop_media_stream(&stream);
    channels
}

/// Query how many channels an output device has, without connecting.
///
/// Creates a throwaway `AudioContext`, routes it to `deviceId` when one is
/// given (a no-op without `setSinkId`, so Safari and Firefox report the
/// default output — the device they play to anyway), reads the
/// destination's `maxChannelCount`, and closes the context. Resolves to a
/// count in `[1, MAX_CHANNELS]`.
///
/// Module-level for the same reason as [`get_input_device_channels`].
#[wasm_bindgen(js_name = getOutputDeviceChannels)]
pub async fn get_output_device_channels(device_id: Option<String>) -> Result<u32, JsValue> {
    let ctx = AudioContext::new()?;

    let channels = match device_id.filter(|id| !id.is_empty()) {
        Some(id) => route_output_sink(ctx.as_ref(), Some(id)).await,
        None => Ok(()),
    }
    .map(|()| clamp_channel_count(ctx.destination().max_channel_count()));

    if let Ok(close) = ctx.close() {
        let _ = JsFuture::from(close).await;
    }
    channels
}

/// Enumerate all media devices
async fn enumerate_devices(media_devices: &MediaDevices) -> Result<js_sys::Array, JsValue> {
    let devices_promise = media_devices.enumerate_devices()?;
    JsFuture::from(devices_promise)
        .await?
        .dyn_into::<js_sys::Array>()
}

/// Map a `web_sys::MediaDeviceKind` to the plain `AudioDeviceKind` used by the
/// pure categorization core.
fn map_media_device_kind(kind: MediaDeviceKind) -> AudioDeviceKind {
    match kind {
        MediaDeviceKind::Audioinput => AudioDeviceKind::Input,
        MediaDeviceKind::Audiooutput => AudioDeviceKind::Output,
        _ => AudioDeviceKind::Other,
    }
}

/// Categorize devices into input and output arrays.
///
/// Browser glue: converts the `MediaDeviceInfo` JS array into plain `RawDevice`
/// values, runs the pure `categorize_devices_core`, then converts the resulting
/// `DeviceInfo` lists back into JS arrays.
fn categorize_devices(devices: &js_sys::Array) -> (js_sys::Array, js_sys::Array) {
    let raw: Vec<RawDevice> = (0..devices.length())
        .filter_map(|i| devices.get(i).dyn_into::<MediaDeviceInfo>().ok())
        .map(|device| RawDevice {
            device_id: device.device_id(),
            label: device.label(),
            kind: map_media_device_kind(device.kind()),
        })
        .collect();

    let (inputs, outputs) = categorize_devices_core(&raw);

    let input_devices = js_sys::Array::new();
    for info in inputs {
        input_devices.push(&JsValue::from(info));
    }
    let output_devices = js_sys::Array::new();
    for info in outputs {
        output_devices.push(&JsValue::from(info));
    }

    (input_devices, output_devices)
}

/// Get available audio devices (returns {inputDevices, outputDevices})
#[wasm_bindgen(js_name = getAudioDevices)]
pub async fn get_audio_devices() -> Result<JsValue, JsValue> {
    let media_devices = get_media_devices()?;

    // Request permissions first
    request_audio_permission(&media_devices).await?;

    // Enumerate devices
    let devices = enumerate_devices(&media_devices).await?;

    // Categorize into input and output
    let (input_devices, output_devices) = categorize_devices(&devices);

    // Build result object
    let result = js_sys::Object::new();
    js_sys::Reflect::set(&result, &"inputDevices".into(), &input_devices)?;
    js_sys::Reflect::set(&result, &"outputDevices".into(), &output_devices)?;

    Ok(result.into())
}

// ==============================================================================
// Tests
// ==============================================================================
//
// These run on the native target via `npm run test`. They cover the pure
// categorization core (`categorize_devices_core`), which operates over plain
// `RawDevice` values and therefore needs no browser / `web_sys` runtime. The
// browser glue (`enumerate_devices`, `categorize_devices`,
// `map_media_device_kind`) that adapts `MediaDeviceInfo` to/from these plain
// types is exercised by the browser-bound WASM tests.
#[cfg(test)]
mod tests {
    use super::*;

    fn input(id: &str, label: &str) -> RawDevice {
        RawDevice {
            device_id: id.to_string(),
            label: label.to_string(),
            kind: AudioDeviceKind::Input,
        }
    }

    fn output(id: &str, label: &str) -> RawDevice {
        RawDevice {
            device_id: id.to_string(),
            label: label.to_string(),
            kind: AudioDeviceKind::Output,
        }
    }

    fn other(id: &str, label: &str) -> RawDevice {
        RawDevice {
            device_id: id.to_string(),
            label: label.to_string(),
            kind: AudioDeviceKind::Other,
        }
    }

    /// Assert a `DeviceInfo` list matches the expected (id, label) pairs in order.
    fn assert_devices(actual: &[DeviceInfo], expected: &[(&str, &str)]) {
        assert_eq!(actual.len(), expected.len(), "device count mismatch");
        for (got, (id, label)) in actual.iter().zip(expected.iter()) {
            assert_eq!(got.device_id(), *id, "device_id mismatch");
            assert_eq!(got.label(), *label, "label mismatch");
        }
    }

    #[test]
    fn test_categorize_mixed_partitions_and_maps() {
        // Interleaved inputs/outputs so we also confirm correct partitioning.
        let devices = vec![
            input("in-1", "Mic A"),
            output("out-1", "Speakers"),
            input("in-2", "Mic B"),
            output("out-2", "Headphones"),
        ];
        let (inputs, outputs) = categorize_devices_core(&devices);
        assert_devices(&inputs, &[("in-1", "Mic A"), ("in-2", "Mic B")]);
        assert_devices(&outputs, &[("out-1", "Speakers"), ("out-2", "Headphones")]);
    }

    #[test]
    fn test_categorize_drops_other_kinds() {
        // Non-audio devices (e.g. video inputs) must be discarded entirely.
        let devices = vec![
            other("vid-1", "Webcam"),
            input("in-1", "Mic A"),
            other("vid-2", "Capture Card"),
            output("out-1", "Speakers"),
        ];
        let (inputs, outputs) = categorize_devices_core(&devices);
        assert_devices(&inputs, &[("in-1", "Mic A")]);
        assert_devices(&outputs, &[("out-1", "Speakers")]);
    }

    #[test]
    fn test_categorize_preserves_order() {
        // Order within each category must follow input order, even when inputs
        // and outputs are interleaved with each other.
        let devices = vec![
            input("in-3", "Third"),
            input("in-1", "First"),
            output("out-2", "Out Second"),
            input("in-2", "Second"),
            output("out-1", "Out First"),
        ];
        let (inputs, outputs) = categorize_devices_core(&devices);
        assert_devices(
            &inputs,
            &[("in-3", "Third"), ("in-1", "First"), ("in-2", "Second")],
        );
        assert_devices(&outputs, &[("out-2", "Out Second"), ("out-1", "Out First")]);
    }

    #[test]
    fn test_resolve_input_channels_boundaries() {
        // (settings, capability_max) -> expected
        let cases: &[(Option<u32>, Option<u32>, u32)] = &[
            // Nothing reported: assume mono.
            (None, None, 1),
            // Capability only.
            (None, Some(4), 4),
            // Settings fallback when capabilities are unavailable.
            (Some(2), None, 2),
            // The capability wins over a narrower granted setting.
            (Some(1), Some(2), 2),
            // Clamped to [1, MAX_CHANNELS] on both sides.
            (Some(0), None, 1),
            (None, Some(0), 1),
            (None, Some(9), 8),
            (Some(9), None, 8),
        ];
        for &(settings, capability_max, expected) in cases {
            assert_eq!(
                resolve_input_channels(settings, capability_max),
                expected,
                "settings={settings:?} capability_max={capability_max:?}"
            );
        }
    }

    // ── Browser tests (web_sys / MediaDevices) ───────────────────────────────
    //
    // Real-browser coverage of the device-enumeration / permission glue around
    // the natively-tested `categorize_devices_core` (above): `get_media_devices`,
    // `enumerate_devices`, `request_audio_permission`/`getUserMedia`,
    // `stop_media_stream`, and the `get_audio_devices` orchestrator. Run in
    // headless Chrome via `npm run test:wasm`. The per-binary browser opt-in
    // (`wasm_bindgen_test_configure!(run_in_browser)`) lives once in
    // `crate::test_support`; here we only import the attribute.
    //
    // Headless Chrome returns an empty device list and rejects `getUserMedia`
    // unless launched with `--use-fake-device-for-media-stream` (synthetic mic)
    // and `--use-fake-ui-for-media-stream` (auto-granted permission, no user
    // gesture). Those flags are set in `webdriver.json`; see `docs/WASM_TESTING.md`.

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;

    /// Read the `deviceId`/`label` getters off a `DeviceInfo` exported to JS.
    ///
    /// `get_audio_devices` returns the categorized devices as JS arrays of the
    /// `#[wasm_bindgen]` `DeviceInfo` struct, so a browser test inspects them
    /// through the same JS surface the TypeScript UI sees.
    #[cfg(target_arch = "wasm32")]
    fn read_device_info(value: &JsValue) -> (String, String) {
        let device_id = js_sys::Reflect::get(value, &"deviceId".into())
            .expect("DeviceInfo must expose a deviceId getter")
            .as_string()
            .expect("deviceId must be a string");
        let label = js_sys::Reflect::get(value, &"label".into())
            .expect("DeviceInfo must expose a label getter")
            .as_string()
            .expect("label must be a string");
        (device_id, label)
    }

    /// Read a `MediaStreamTrack`'s `readyState` (`"live"`/`"ended"`) via JS
    /// reflection. The `MediaStreamTrackState` enum binding isn't in our enabled
    /// `web-sys` feature set, so — like `engine.rs` reads `AudioContext.state` —
    /// we go through the string property rather than widen the feature gates.
    #[cfg(target_arch = "wasm32")]
    fn track_ready_state(track: &web_sys::MediaStreamTrack) -> String {
        js_sys::Reflect::get(track.as_ref(), &"readyState".into())
            .expect("MediaStreamTrack must expose readyState")
            .as_string()
            .expect("readyState must be a string")
    }

    /// `stop_media_stream` must stop every track on a live stream. We acquire a
    /// synthetic mic stream directly (so we can inspect tracks *after* stopping,
    /// unlike `request_audio_permission` which stops internally), assert its
    /// tracks start out `"live"`, stop them, then assert they end up `"ended"`.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn stop_media_stream_ends_tracks() {
        let media_devices =
            get_media_devices().expect("navigator.mediaDevices must be available");

        let stream = get_user_media_audio(&media_devices, &JsValue::from(true))
            .await
            .expect("getUserMedia should resolve with fake-device flags");

        let tracks = stream.get_tracks();
        assert!(tracks.length() > 0, "fake mic stream must expose at least one track");
        for i in 0..tracks.length() {
            let track: web_sys::MediaStreamTrack = tracks.get(i).unchecked_into();
            assert_eq!(track_ready_state(&track), "live");
        }

        stop_media_stream(&stream);

        for i in 0..tracks.length() {
            let track: web_sys::MediaStreamTrack = tracks.get(i).unchecked_into();
            assert_eq!(
                track_ready_state(&track),
                "ended",
                "stop_media_stream must end every track"
            );
        }
    }

    /// `get_audio_devices` orchestrates permission → enumeration → categorization
    /// → JS object. With the fake-device flags it must return a
    /// `{ inputDevices, outputDevices }` object whose input list is non-empty,
    /// with each entry carrying a non-empty id and label (labels are only exposed
    /// after permission is granted, which the fake-UI flag does). This exercises
    /// the `MediaDeviceInfo` → `RawDevice` → `DeviceInfo` → JS glue around the
    /// natively-tested core, not the core itself.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn get_audio_devices_returns_populated_lists() {
        let result = get_audio_devices()
            .await
            .expect("get_audio_devices should succeed with fake-device flags");

        let input_devices: js_sys::Array =
            js_sys::Reflect::get(&result, &"inputDevices".into())
                .expect("result must have an inputDevices field")
                .dyn_into()
                .expect("inputDevices must be an array");
        let output_devices: js_sys::Array =
            js_sys::Reflect::get(&result, &"outputDevices".into())
                .expect("result must have an outputDevices field")
                .dyn_into()
                .expect("outputDevices must be an array");

        assert!(
            input_devices.length() > 0,
            "fake-device Chrome must report at least one audio input"
        );

        for value in input_devices.iter() {
            let (device_id, label) = read_device_info(&value);
            assert!(!device_id.is_empty(), "input device id must be non-empty");
            assert!(
                !label.is_empty(),
                "input device label must be populated after permission grant"
            );
        }

        // Output (audiooutput) devices: their ids/labels must be well-formed when
        // present, but headless Chrome does not always expose an audio sink even
        // with the fake-device flags, so an empty output list is tolerated rather
        // than asserted on (see the headless caveat in WEB-39 / docs/WASM_TESTING.md).
        for value in output_devices.iter() {
            let (device_id, label) = read_device_info(&value);
            assert!(!device_id.is_empty(), "output device id must be non-empty");
            assert!(
                !label.is_empty(),
                "output device label must be populated after permission grant"
            );
        }
    }

    /// `get_input_device_channels` must open the device, resolve a count in
    /// `[1, MAX_CHANNELS]`, and surface a `getUserMedia` failure (an unknown
    /// exact device id) as a rejection rather than a made-up count.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn get_input_device_channels_resolves_count_or_rejects() {
        let channels = get_input_device_channels(None)
            .await
            .expect("probing the default fake input should resolve");
        assert!(
            (1..=MAX_CHANNELS as u32).contains(&channels),
            "input probe must resolve within [1, MAX_CHANNELS], got {channels}"
        );

        assert!(
            get_input_device_channels(Some("no-such-input-device".to_string()))
                .await
                .is_err(),
            "an unknown exact device id must reject"
        );
    }

    /// `get_output_device_channels` must resolve the default output's count
    /// in `[1, MAX_CHANNELS]`, and surface a failed `setSinkId` (an unknown
    /// device id) as a rejection.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn get_output_device_channels_resolves_count_or_rejects() {
        let channels = get_output_device_channels(None)
            .await
            .expect("probing the default output should resolve");
        assert!(
            (1..=MAX_CHANNELS as u32).contains(&channels),
            "output probe must resolve within [1, MAX_CHANNELS], got {channels}"
        );

        assert!(
            get_output_device_channels(Some("no-such-output-device".to_string()))
                .await
                .is_err(),
            "an unknown sink id must reject"
        );
    }
}

