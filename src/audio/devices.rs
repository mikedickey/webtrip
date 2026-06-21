use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{MediaDeviceInfo, MediaDeviceKind, MediaDevices, MediaStream, MediaStreamConstraints};

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

/// Request audio permission by getting and immediately stopping a stream
async fn request_audio_permission(media_devices: &MediaDevices) -> Result<(), JsValue> {
    let constraints = MediaStreamConstraints::new();
    constraints.set_audio(&JsValue::from(true));
    constraints.set_video(&JsValue::from(false));

    let stream_promise = media_devices.get_user_media_with_constraints(&constraints)?;
    let stream: MediaStream = JsFuture::from(stream_promise).await?.unchecked_into();

    stop_media_stream(&stream);
    Ok(())
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
    fn test_categorize_empty() {
        let (inputs, outputs) = categorize_devices_core(&[]);
        assert!(inputs.is_empty());
        assert!(outputs.is_empty());
    }

    #[test]
    fn test_categorize_inputs_only() {
        let devices = vec![input("in-1", "Mic A"), input("in-2", "Mic B")];
        let (inputs, outputs) = categorize_devices_core(&devices);
        assert_devices(&inputs, &[("in-1", "Mic A"), ("in-2", "Mic B")]);
        assert!(outputs.is_empty());
    }

    #[test]
    fn test_categorize_outputs_only() {
        let devices = vec![output("out-1", "Speakers"), output("out-2", "Headphones")];
        let (inputs, outputs) = categorize_devices_core(&devices);
        assert!(inputs.is_empty());
        assert_devices(&outputs, &[("out-1", "Speakers"), ("out-2", "Headphones")]);
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
}

