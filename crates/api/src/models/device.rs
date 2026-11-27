//! Device models

use super::{AudioProperties, Channels, ModifiedAtTime, Quality, ServerConfig};
use serde::{Deserialize, Serialize};
#[cfg(feature = "wasm")]
use tsify_next::Tsify;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// A JackTrip hardware device.
///
/// The [`config`](Self::config), [`alsa`](Self::alsa), [`audio`](Self::audio)
/// and [`modified`](Self::modified) groups are flattened, so all of their keys
/// sit at the top level of the JSON object.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// Device ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Device name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Device MAC address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,

    /// Owner's user ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,

    /// ID of the studio the device is connected to
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,

    /// Device software version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// ALSA device name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alsa_name: Option<String>,

    /// ALSA device type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<String>,

    /// API key prefix
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_prefix: Option<String>,

    /// API key hash
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_hash: Option<String>,

    /// Port, quality, channels and effects
    #[serde(flatten)]
    pub config: DeviceConfig,

    /// Sound-card volumes and mutes
    #[serde(flatten)]
    pub alsa: AlsaConfig,

    /// Period, queue buffer and buffer strategy
    #[serde(flatten)]
    pub audio: AudioProperties,

    /// Registration and last-update timestamps
    #[serde(flatten)]
    pub modified: ModifiedAtTime,
}

/// Device settings (spec name `DeviceConfig`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct DeviceConfig {
    /// Local UDP port the device binds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_port: Option<i32>,

    /// Connection quality
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<Quality>,

    /// Number of input channels
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_channels: Option<Channels>,

    /// Number of output channels
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_channels: Option<Channels>,

    /// Reverb level
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverb: Option<i32>,

    /// Apply a limiter to input and output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limiter: Option<bool>,

    /// Apply a compressor to the output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compressor: Option<bool>,

    /// Allow multiple USB audio interfaces (JackTrip Analog Bridge only)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_usb: Option<bool>,
}

/// Device sound-card settings (spec name `ALSAConfig`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct AlsaConfig {
    /// Capture (input) volume
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_volume: Option<i32>,

    /// Mute the capture input
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_mute: Option<bool>,

    /// Boost capture gain (deprecated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_boost: Option<bool>,

    /// Playback (output) volume
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_volume: Option<i32>,

    /// Mute the playback output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_mute: Option<bool>,

    /// Boost playback gain (deprecated)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_boost: Option<bool>,

    /// Local monitor volume
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_volume: Option<i32>,

    /// Mute the local monitor
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_mute: Option<bool>,
}

/// The configuration a device applies, returned by
/// [`DevicesApi::send_heartbeat`](crate::api::devices::DevicesApi::send_heartbeat):
/// its own settings plus those of the studio it is connected to. Every group
/// is flattened into one JSON object.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct DeviceAgentConfig {
    /// Device settings
    #[serde(flatten)]
    pub device: DeviceConfig,

    /// The connected studio's settings (name, host, port, sample rate, …)
    #[serde(flatten)]
    pub server: ServerConfig,

    /// Sound-card settings
    #[serde(flatten)]
    pub alsa: AlsaConfig,

    /// Period, queue buffer and buffer strategy
    #[serde(flatten)]
    pub audio: AudioProperties,

    /// Token securing the connection between device and studio
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
}

// Hand-written so the auth token never reaches logs.
impl std::fmt::Debug for DeviceAgentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceAgentConfig")
            .field("device", &self.device)
            .field("server", &self.server)
            .field("alsa", &self.alsa)
            .field("audio", &self.audio)
            .field("auth_token", &self.auth_token.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Period, SampleRate};

    #[test]
    fn device_wire_format() {
        // Regression: the connected studio was sent as `studioId`; the API
        // uses `serverId`.
        let json = serde_json::json!({
            "id": "dev-1", "mac": "AA:BB", "name": "Living Room JT", "ownerId": "user-1",
            "serverId": "studio-1", "version": "2.4.0",
            "quality": 2, "inputChannels": 2, "outputChannels": 1, "enableUsb": true,
            "captureVolume": 80, "playbackMute": false,
            "period": 128, "queueBuffer": 4,
            "createdAt": "2026-06-14T00:00:00Z"
        });
        let d: Device = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(d.server_id.as_deref(), Some("studio-1"));
        assert_eq!(d.config.quality, Some(Quality::Lossless));
        assert_eq!(d.config.output_channels, Some(Channels::Mono));
        assert_eq!(d.alsa.capture_volume, Some(80));
        assert_eq!(d.audio.period, Some(Period::P128));
        assert_eq!(serde_json::to_value(&d).unwrap(), json);
    }

    #[test]
    fn device_agent_config_wire_format() {
        // Regression: this was modeled as nested `{device, server,
        // credentials}`; the API returns one flat object.
        let json = serde_json::json!({
            "devicePort": 4464, "serverHost": "h.example.com", "serverPort": 4464,
            "sampleRate": 48000, "captureVolume": 70, "period": 64, "authToken": "s3cr3t"
        });
        let c: DeviceAgentConfig = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(c.server.server_host.as_deref(), Some("h.example.com"));
        assert_eq!(c.server.sample_rate, Some(SampleRate::Rate48000));
        assert_eq!(c.auth_token.as_deref(), Some("s3cr3t"));
        assert_eq!(serde_json::to_value(&c).unwrap(), json);
        assert!(!format!("{c:?}").contains("s3cr3t"));
    }
}
