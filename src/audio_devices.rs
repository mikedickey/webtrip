use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{MediaDeviceInfo, MediaDeviceKind, MediaDevices, MediaStream, MediaStreamConstraints};

/// Device information
#[wasm_bindgen]
#[derive(Clone, Debug)]
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

/// Categorize devices into input and output arrays
fn categorize_devices(devices: &js_sys::Array) -> (js_sys::Array, js_sys::Array) {
    let input_devices = js_sys::Array::new();
    let output_devices = js_sys::Array::new();

    for i in 0..devices.length() {
        if let Some(device) = devices.get(i).dyn_into::<MediaDeviceInfo>().ok() {
            let info = DeviceInfo {
                device_id: device.device_id(),
                label: device.label(),
            };

            match device.kind() {
                MediaDeviceKind::Audioinput => {
                    input_devices.push(&JsValue::from(info));
                }
                MediaDeviceKind::Audiooutput => {
                    output_devices.push(&JsValue::from(info));
                }
                _ => {}
            }
        }
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

