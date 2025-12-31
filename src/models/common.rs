//! Common enums and types shared across models

use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};
use tsify_next::Tsify;
use wasm_bindgen::prelude::*;

/// Audio packet frames per period.
/// Lower values = lower latency but may cause audio glitches.
#[derive(Tsify, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[repr(u16)]
pub enum Period {
    P16 = 16,
    P32 = 32,
    P64 = 64,
    P128 = 128,
    P256 = 256,
    P512 = 512,
    P1024 = 1024,
    P2048 = 2048,
}

impl Default for Period {
    fn default() -> Self {
        Self::P128
    }
}

/// Network jitter buffer size.
/// Larger values reduce jitter from unstable connections but add latency.
#[derive(Tsify, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[repr(u8)]
pub enum QueueBuffer {
    Q0 = 0,
    Q2 = 2,
    Q3 = 3,
    Q4 = 4,
    Q5 = 5,
    Q6 = 6,
    Q7 = 7,
    Q8 = 8,
    Q10 = 10,
    Q12 = 12,
    Q14 = 14,
    Q16 = 16,
    Q18 = 18,
    Q20 = 20,
    Q22 = 22,
    Q24 = 24,
    Q26 = 26,
    Q28 = 28,
    Q30 = 30,
    Q32 = 32,
}

impl Default for QueueBuffer {
    fn default() -> Self {
        Self::Q4
    }
}

/// Network jitter buffer strategy
#[derive(Tsify, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[repr(u8)]
pub enum BufferStrategy {
    /// Standard strategy
    Standard = 1,
    /// Auto-adjust strategy
    AutoAdjust = 2,
    /// Broadcast strategy
    Broadcast = 3,
}

impl Default for BufferStrategy {
    fn default() -> Self {
        Self::Standard
    }
}

/// Audio connection quality level
#[derive(Tsify, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[repr(u8)]
pub enum Quality {
    /// Low quality (Jamulus)
    Low = 0,
    /// High quality (Jamulus)
    High = 1,
    /// Lossless (JackTrip)
    Lossless = 2,
}

impl Default for Quality {
    fn default() -> Self {
        Self::Lossless
    }
}

/// Number of audio channels
#[derive(Tsify, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[repr(u8)]
pub enum Channels {
    Mono = 1,
    Stereo = 2,
}

impl Default for Channels {
    fn default() -> Self {
        Self::Stereo
    }
}

/// Broadcast visibility level
#[derive(Tsify, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[repr(u8)]
pub enum BroadcastVisibility {
    /// Not broadcasting
    Off = 0,
    /// Private broadcast (unlisted)
    Private = 1,
    /// Public broadcast
    Public = 2,
}

impl Default for BroadcastVisibility {
    fn default() -> Self {
        Self::Off
    }
}

/// Content visibility level
#[derive(Tsify, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[repr(u8)]
pub enum Visibility {
    /// Private/unlisted
    Private = 0,
    /// Public
    Public = 1,
}

impl Default for Visibility {
    fn default() -> Self {
        Self::Private
    }
}

/// Generic status for resources
#[derive(Tsify, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[serde(rename_all = "PascalCase")]
pub enum ResourceStatus {
    Starting,
    Ready,
    Disabled,
    Deleting,
}

impl Default for ResourceStatus {
    fn default() -> Self {
        Self::Starting
    }
}

/// Recording processing status
#[derive(Tsify, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[repr(u8)]
pub enum RecordingStatus {
    /// Recording in progress
    Recording = 0,
    /// Processing/encoding
    Processing = 1,
    /// Ready for playback
    Ready = 2,
    /// Failed/error
    Failed = 3,
}

impl Default for RecordingStatus {
    fn default() -> Self {
        Self::Recording
    }
}

/// Studio type (audio engine)
#[derive(Tsify, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[tsify(into_wasm_abi, from_wasm_abi)]
pub enum StudioType {
    /// JackTrip audio engine only
    JackTrip,
    /// JackTrip with Jamulus bridge
    #[serde(rename = "JackTrip+Jamulus")]
    JackTripJamulus,
}

impl Default for StudioType {
    fn default() -> Self {
        Self::JackTrip
    }
}

/// Audio sample rate in Hz
#[derive(Tsify, Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[tsify(into_wasm_abi, from_wasm_abi)]
#[repr(u32)]
pub enum SampleRate {
    /// 44.1 kHz (CD quality)
    Rate44100 = 44100,
    /// 48 kHz (professional audio)
    Rate48000 = 48000,
    /// 88.2 kHz (high resolution)
    Rate88200 = 88200,
    /// 96 kHz (high resolution)
    Rate96000 = 96000,
}

impl Default for SampleRate {
    fn default() -> Self {
        Self::Rate48000
    }
}

