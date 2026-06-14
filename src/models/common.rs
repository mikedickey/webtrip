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

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T>(value: &T)
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(value, &back, "round-trip mismatch; json={json}");
    }

    #[test]
    fn period_variants_roundtrip() {
        for v in [
            Period::P16,
            Period::P32,
            Period::P64,
            Period::P128,
            Period::P256,
            Period::P512,
            Period::P1024,
            Period::P2048,
        ] {
            roundtrip(&v);
            let n = serde_json::to_string(&v).unwrap();
            assert_eq!(n.parse::<u16>().unwrap(), v as u16);
        }
    }

    #[test]
    fn queue_buffer_variants_roundtrip() {
        for v in [
            QueueBuffer::Q0,
            QueueBuffer::Q2,
            QueueBuffer::Q3,
            QueueBuffer::Q4,
            QueueBuffer::Q5,
            QueueBuffer::Q6,
            QueueBuffer::Q7,
            QueueBuffer::Q8,
            QueueBuffer::Q10,
            QueueBuffer::Q12,
            QueueBuffer::Q14,
            QueueBuffer::Q16,
            QueueBuffer::Q18,
            QueueBuffer::Q20,
            QueueBuffer::Q22,
            QueueBuffer::Q24,
            QueueBuffer::Q26,
            QueueBuffer::Q28,
            QueueBuffer::Q30,
            QueueBuffer::Q32,
        ] {
            roundtrip(&v);
            assert_eq!(serde_json::to_string(&v).unwrap().parse::<u8>().unwrap(), v as u8);
        }
    }

    #[test]
    fn buffer_strategy_variants_roundtrip() {
        for v in [BufferStrategy::Standard, BufferStrategy::AutoAdjust, BufferStrategy::Broadcast] {
            roundtrip(&v);
        }
        assert_eq!(serde_json::to_string(&BufferStrategy::Standard).unwrap(), "1");
        assert_eq!(serde_json::to_string(&BufferStrategy::AutoAdjust).unwrap(), "2");
        assert_eq!(serde_json::to_string(&BufferStrategy::Broadcast).unwrap(), "3");
    }

    #[test]
    fn quality_variants_roundtrip() {
        for v in [Quality::Low, Quality::High, Quality::Lossless] {
            roundtrip(&v);
        }
        assert_eq!(serde_json::to_string(&Quality::Lossless).unwrap(), "2");
    }

    #[test]
    fn channels_variants_roundtrip() {
        roundtrip(&Channels::Mono);
        roundtrip(&Channels::Stereo);
        assert_eq!(serde_json::to_string(&Channels::Mono).unwrap(), "1");
        assert_eq!(serde_json::to_string(&Channels::Stereo).unwrap(), "2");
    }

    #[test]
    fn broadcast_visibility_variants_roundtrip() {
        for v in [BroadcastVisibility::Off, BroadcastVisibility::Private, BroadcastVisibility::Public] {
            roundtrip(&v);
        }
        assert_eq!(serde_json::to_string(&BroadcastVisibility::Public).unwrap(), "2");
    }

    #[test]
    fn visibility_variants_roundtrip() {
        roundtrip(&Visibility::Private);
        roundtrip(&Visibility::Public);
        assert_eq!(serde_json::to_string(&Visibility::Private).unwrap(), "0");
        assert_eq!(serde_json::to_string(&Visibility::Public).unwrap(), "1");
    }

    #[test]
    fn resource_status_variants_roundtrip_and_wire_format() {
        for v in [
            ResourceStatus::Starting,
            ResourceStatus::Ready,
            ResourceStatus::Disabled,
            ResourceStatus::Deleting,
        ] {
            roundtrip(&v);
        }
        // PascalCase on the wire
        assert_eq!(serde_json::to_string(&ResourceStatus::Starting).unwrap(), "\"Starting\"");
        assert_eq!(serde_json::to_string(&ResourceStatus::Ready).unwrap(), "\"Ready\"");
        assert_eq!(serde_json::to_string(&ResourceStatus::Disabled).unwrap(), "\"Disabled\"");
        assert_eq!(serde_json::to_string(&ResourceStatus::Deleting).unwrap(), "\"Deleting\"");

        let v: ResourceStatus = serde_json::from_str("\"Ready\"").unwrap();
        assert_eq!(v, ResourceStatus::Ready);
    }

    #[test]
    fn recording_status_variants_roundtrip() {
        for v in [
            RecordingStatus::Recording,
            RecordingStatus::Processing,
            RecordingStatus::Ready,
            RecordingStatus::Failed,
        ] {
            roundtrip(&v);
        }
        assert_eq!(serde_json::to_string(&RecordingStatus::Failed).unwrap(), "3");
    }

    #[test]
    fn studio_type_variants_roundtrip_and_wire_format() {
        roundtrip(&StudioType::JackTrip);
        roundtrip(&StudioType::JackTripJamulus);
        // JackTrip is the default Serialize name; JackTripJamulus is renamed
        assert_eq!(serde_json::to_string(&StudioType::JackTrip).unwrap(), "\"JackTrip\"");
        assert_eq!(
            serde_json::to_string(&StudioType::JackTripJamulus).unwrap(),
            "\"JackTrip+Jamulus\""
        );
        let v: StudioType = serde_json::from_str("\"JackTrip+Jamulus\"").unwrap();
        assert_eq!(v, StudioType::JackTripJamulus);
    }

    #[test]
    fn sample_rate_variants_roundtrip() {
        for v in [
            SampleRate::Rate44100,
            SampleRate::Rate48000,
            SampleRate::Rate88200,
            SampleRate::Rate96000,
        ] {
            roundtrip(&v);
            assert_eq!(
                serde_json::to_string(&v).unwrap().parse::<u32>().unwrap(),
                v as u32
            );
        }
    }

    #[test]
    fn defaults_are_sensible() {
        assert_eq!(Period::default(), Period::P128);
        assert_eq!(QueueBuffer::default(), QueueBuffer::Q4);
        assert_eq!(BufferStrategy::default(), BufferStrategy::Standard);
        assert_eq!(Quality::default(), Quality::Lossless);
        assert_eq!(Channels::default(), Channels::Stereo);
        assert_eq!(BroadcastVisibility::default(), BroadcastVisibility::Off);
        assert_eq!(Visibility::default(), Visibility::Private);
        assert_eq!(ResourceStatus::default(), ResourceStatus::Starting);
        assert_eq!(RecordingStatus::default(), RecordingStatus::Recording);
        assert_eq!(StudioType::default(), StudioType::JackTrip);
        assert_eq!(SampleRate::default(), SampleRate::Rate48000);
    }
}
