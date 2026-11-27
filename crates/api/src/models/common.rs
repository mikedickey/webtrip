//! Common enums and types shared across models.
//!
//! Integer enums are serialized as their integer wire value (shown on each
//! variant); string enums as the string shown.

use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
#[cfg(feature = "wasm")]
use tsify_next::Tsify;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// Audio frames per packet (the JackTrip period). Wire value = frame count.
///
/// Lower values lower latency but are more prone to dropouts; at 48 kHz,
/// 128 frames is about 2.7 ms of audio.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[repr(u16)]
pub enum Period {
    /// 16 frames per packet (`16`)
    P16 = 16,
    /// 32 frames per packet (`32`)
    P32 = 32,
    /// 64 frames per packet (`64`)
    P64 = 64,
    /// 128 frames per packet (`128`)
    #[default]
    P128 = 128,
    /// 256 frames per packet (`256`)
    P256 = 256,
    /// 512 frames per packet (`512`)
    P512 = 512,
    /// 1024 frames per packet (`1024`)
    P1024 = 1024,
    /// 2048 frames per packet (`2048`)
    P2048 = 2048,
}

/// Network jitter-buffer size, in packets. Wire value = packet count.
///
/// A larger queue absorbs more jitter from unstable connections at the cost of
/// added latency.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[repr(u8)]
pub enum QueueBuffer {
    /// `0`: no fixed queue size (the API does not define a packet count for it)
    Q0 = 0,
    /// 2 packets (`2`)
    Q2 = 2,
    /// 3 packets (`3`)
    Q3 = 3,
    /// 4 packets (`4`)
    #[default]
    Q4 = 4,
    /// 5 packets (`5`)
    Q5 = 5,
    /// 6 packets (`6`)
    Q6 = 6,
    /// 7 packets (`7`)
    Q7 = 7,
    /// 8 packets (`8`)
    Q8 = 8,
    /// 10 packets (`10`)
    Q10 = 10,
    /// 12 packets (`12`)
    Q12 = 12,
    /// 14 packets (`14`)
    Q14 = 14,
    /// 16 packets (`16`)
    Q16 = 16,
    /// 18 packets (`18`)
    Q18 = 18,
    /// 20 packets (`20`)
    Q20 = 20,
    /// 22 packets (`22`)
    Q22 = 22,
    /// 24 packets (`24`)
    Q24 = 24,
    /// 26 packets (`26`)
    Q26 = 26,
    /// 28 packets (`28`)
    Q28 = 28,
    /// 30 packets (`30`)
    Q30 = 30,
    /// 32 packets (`32`)
    Q32 = 32,
}

/// Network jitter-buffer strategy, passed through to JackTrip's
/// `--bufstrategy` option.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[repr(u8)]
pub enum BufferStrategy {
    /// `1`: JackTrip buffer strategy 1
    #[default]
    Standard = 1,
    /// `2`: JackTrip buffer strategy 2
    AutoAdjust = 2,
    /// `3`: JackTrip buffer strategy 3
    Broadcast = 3,
}

/// Device connection quality.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[repr(u8)]
pub enum Quality {
    /// `0`: low-quality Jamulus
    Low = 0,
    /// `1`: high-quality Jamulus
    High = 1,
    /// `2`: lossless JackTrip
    #[default]
    Lossless = 2,
}

/// Number of device audio channels.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[repr(u8)]
pub enum Channels {
    /// `1`: one channel
    Mono = 1,
    /// `2`: two channels
    #[default]
    Stereo = 2,
}

/// Studio broadcast visibility, used by [`ServerAgentConfig::broadcast`](super::ServerAgentConfig::broadcast)
/// and [`ActivationRequestOpts::visibility`](super::ActivationRequestOpts::visibility).
///
/// A studio's `broadcast` field only takes `0`–`2`; the recording values are
/// for activation requests.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[repr(u8)]
pub enum BroadcastVisibility {
    /// `0`: not broadcasting
    #[default]
    Offline = 0,
    /// `1`: public broadcast, listed on JackTrip Radio
    Public = 1,
    /// `2`: unlisted broadcast, reachable only by link
    Unlisted = 2,
    /// `3`: private recording, not broadcast
    PrivateRecording = 3,
    /// `4`: stem (per-participant track) recording, not broadcast
    StemRecording = 4,
}

/// Recording visibility.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[repr(u8)]
pub enum Visibility {
    /// `0`: private
    #[default]
    Private = 0,
    /// `1`: public
    Public = 1,
}

/// Studio lifecycle status, serialized as the PascalCase variant name (a
/// string, unlike the integer enums).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "PascalCase")]
pub enum ResourceStatus {
    /// `"Starting"`: being provisioned or booting; not yet joinable
    #[default]
    Starting,
    /// `"Ready"`: running and joinable
    Ready,
    /// `"Disabled"`: stopped; can be started again
    Disabled,
    /// `"Deleting"`: being torn down
    Deleting,
    /// `"Removed"`: deleted
    Removed,
}

/// Recording processing status.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[repr(u8)]
pub enum RecordingStatus {
    /// `0`: recording in progress
    #[default]
    Recording = 0,
    /// `1`: processing/encoding
    Processing = 1,
    /// `2`: ready for playback
    Ready = 2,
    /// `3`: failed
    Failed = 3,
}

/// Studio type (audio engine), serialized as the string shown.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub enum StudioType {
    /// `"JackTrip"`: JackTrip audio engine only
    #[default]
    JackTrip,
    /// `"JackTrip+Jamulus"`: JackTrip with a Jamulus bridge
    #[serde(rename = "JackTrip+Jamulus")]
    JackTripJamulus,
}

/// Audio sample rate in Hz. Wire value = rate.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[repr(u32)]
pub enum SampleRate {
    /// `44100`: 44.1 kHz
    Rate44100 = 44100,
    /// `48000`: 48 kHz
    #[default]
    Rate48000 = 48000,
    /// `88200`: 88.2 kHz
    Rate88200 = 88200,
    /// `96000`: 96 kHz
    Rate96000 = 96000,
}

/// Device ping statistics (spec name `PingStats`), part of a
/// [`DeviceHeartbeat`]. Unlike most models its keys are snake_case on the wire.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct PingStats {
    /// Packets received
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkts_recv: Option<i64>,

    /// Packets sent
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pkts_sent: Option<i64>,

    /// Minimum round-trip time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_rtt: Option<i64>,

    /// Maximum round-trip time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rtt: Option<i64>,

    /// Average round-trip time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_rtt: Option<i64>,

    /// Standard deviation of the round-trip time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stddev_rtt: Option<i64>,

    /// Latest round-trip time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_rtt: Option<i64>,

    /// When these statistics were collected (RFC3339)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats_updated_at: Option<String>,
}

/// What a device reports in a heartbeat (spec name `DeviceHeartbeat`).
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
pub struct DeviceHeartbeat {
    /// Ping statistics (flattened: snake_case keys at the top level)
    #[serde(flatten)]
    pub ping: PingStats,

    /// Device MAC address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,

    /// Device software version
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// ALSA device type (`type` on the wire)
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub device_type: Option<String>,
}

/// A device's API key (spec name `AgentCredentials`), issued when the device
/// is registered.
#[cfg_attr(feature = "wasm", derive(Tsify))]
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "wasm", tsify(into_wasm_abi, from_wasm_abi))]
#[serde(rename_all = "camelCase")]
pub struct AgentCredentials {
    /// API key prefix
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_prefix: Option<String>,

    /// API key secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_secret: Option<String>,
}

// Hand-written so the secret never reaches logs.
impl std::fmt::Debug for AgentCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentCredentials")
            .field("api_prefix", &self.api_prefix)
            .field("api_secret", &self.api_secret.as_ref().map(|_| "<redacted>"))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_enums_serialize_as_their_wire_values() {
        // Pins the serde_repr integers the API uses for these enums.
        let cases: [(String, &str); 9] = [
            (serde_json::to_string(&BroadcastVisibility::Offline).unwrap(), "0"),
            (serde_json::to_string(&BroadcastVisibility::Public).unwrap(), "1"),
            (serde_json::to_string(&BroadcastVisibility::Unlisted).unwrap(), "2"),
            (serde_json::to_string(&BroadcastVisibility::PrivateRecording).unwrap(), "3"),
            (serde_json::to_string(&BroadcastVisibility::StemRecording).unwrap(), "4"),
            (serde_json::to_string(&QueueBuffer::Q0).unwrap(), "0"),
            (serde_json::to_string(&Period::P2048).unwrap(), "2048"),
            (serde_json::to_string(&Quality::Lossless).unwrap(), "2"),
            (serde_json::to_string(&SampleRate::Rate44100).unwrap(), "44100"),
        ];
        for (got, want) in cases {
            assert_eq!(got, want);
        }
        // Regression: Public and Unlisted were swapped (Private=1, Public=2).
        let v: BroadcastVisibility = serde_json::from_str("1").unwrap();
        assert_eq!(v, BroadcastVisibility::Public);
    }

    #[test]
    fn resource_status_wire_format() {
        for (v, s) in [
            (ResourceStatus::Starting, "\"Starting\""),
            (ResourceStatus::Ready, "\"Ready\""),
            (ResourceStatus::Disabled, "\"Disabled\""),
            (ResourceStatus::Deleting, "\"Deleting\""),
            (ResourceStatus::Removed, "\"Removed\""),
        ] {
            assert_eq!(serde_json::to_string(&v).unwrap(), s);
            assert_eq!(serde_json::from_str::<ResourceStatus>(s).unwrap(), v);
        }
    }

    #[test]
    fn studio_type_wire_format() {
        assert_eq!(serde_json::to_string(&StudioType::JackTrip).unwrap(), "\"JackTrip\"");
        assert_eq!(serde_json::to_string(&StudioType::JackTripJamulus).unwrap(), "\"JackTrip+Jamulus\"");
        let v: StudioType = serde_json::from_str("\"JackTrip+Jamulus\"").unwrap();
        assert_eq!(v, StudioType::JackTripJamulus);
    }

    #[test]
    fn device_heartbeat_wire_format() {
        // Regression: ping stats were sent camelCase (`minRtt`); the API's
        // PingStats keys are snake_case and flattened beside mac/version/type.
        let h = DeviceHeartbeat {
            ping: PingStats { pkts_recv: Some(100), min_rtt: Some(10), stats_updated_at: Some("t".into()), ..Default::default() },
            mac: Some("00:11".into()),
            version: Some("2.4.0".into()),
            device_type: Some("usb-x2".into()),
        };
        assert_eq!(
            serde_json::to_value(&h).unwrap(),
            serde_json::json!({
                "pkts_recv": 100, "min_rtt": 10, "stats_updated_at": "t",
                "mac": "00:11", "version": "2.4.0", "type": "usb-x2"
            })
        );
    }

    #[test]
    fn agent_credentials_wire_format_and_debug_redaction() {
        let c = AgentCredentials { api_prefix: Some("pre".into()), api_secret: Some("shh".into()) };
        assert_eq!(serde_json::to_value(&c).unwrap(), serde_json::json!({"apiPrefix": "pre", "apiSecret": "shh"}));
        assert!(!format!("{c:?}").contains("shh"));
    }
}
