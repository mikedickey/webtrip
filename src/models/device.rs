//! Device models

use super::{BufferStrategy, Channels, Period, Quality, QueueBuffer};
use serde::{Deserialize, Serialize};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// A JackTrip hardware device
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    /// Device ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Device MAC address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,

    /// Device name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Owner's user ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,

    /// Connected studio ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub studio_id: Option<String>,

    /// Device firmware version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// ALSA device name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alsa_name: Option<String>,

    /// ALSA device overlay/type
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<String>,

    /// API key prefix
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_prefix: Option<String>,

    /// API key hash
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_hash: Option<String>,

    /// Device bind port
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_port: Option<i32>,

    /// Audio quality setting
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quality: Option<Quality>,

    /// Input channels configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_channels: Option<Channels>,

    /// Output channels configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_channels: Option<Channels>,

    /// Audio frame period
    #[serde(skip_serializing_if = "Option::is_none")]
    pub period: Option<Period>,

    /// Jitter buffer size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_buffer: Option<QueueBuffer>,

    /// Buffer strategy
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_strategy: Option<BufferStrategy>,

    /// Capture/input volume (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_volume: Option<u32>,

    /// Mute capture input
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_mute: Option<bool>,

    /// Playback/output volume (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_volume: Option<u32>,

    /// Mute playback output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback_mute: Option<bool>,

    /// Local monitor volume (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_volume: Option<u32>,

    /// Mute local monitor
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_mute: Option<bool>,

    /// Reverb level (0-100)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverb: Option<u32>,

    /// Enable limiter on input/output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limiter: Option<bool>,

    /// Enable compressor on output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compressor: Option<bool>,

    /// Enable USB audio interfaces (JackTrip Analog Bridge)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable_usb: Option<bool>,

    /// Creation timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,

    /// Last update timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Device configuration (returned from agent config endpoints)
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAgentConfig {
    /// Device configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<Device>,

    /// Server/studio configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<super::Studio>,

    /// Agent credentials
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials: Option<AgentCredentials>,
}

/// Agent credentials for device authentication
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct AgentCredentials {
    /// API key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// API secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_secret: Option<String>,
}

/// Device heartbeat data
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct DeviceHeartbeat {
    /// Device MAC address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,

    /// Device version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Device type/overlay
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub device_type: Option<String>,

    /// API key prefix
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_prefix: Option<String>,

    /// API key secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_secret: Option<String>,

    /// Packets received count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkts_recv: Option<i32>,

    /// Packets sent count
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkts_sent: Option<i32>,

    /// Minimum round-trip time (ms)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_rtt: Option<i32>,

    /// Maximum round-trip time (ms)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rtt: Option<i32>,

    /// Average round-trip time (ms)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_rtt: Option<i32>,

    /// Standard deviation of round-trip time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stddev_rtt: Option<i32>,

    /// Latest round-trip time (ms)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_rtt: Option<i32>,

    /// Stats collection timestamp (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats_updated_at: Option<String>,
}

/// ALSA audio device configuration
#[derive(Tsify, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "camelCase")]
pub struct AlsaConfig {
    /// ALSA device name
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,

    /// Sample rate (Hz)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<i32>,

    /// Buffer size (frames)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_size: Option<i32>,

    /// Number of periods
    #[serde(skip_serializing_if = "Option::is_none")]
    pub periods: Option<i32>,
}

