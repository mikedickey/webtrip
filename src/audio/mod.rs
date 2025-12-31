//! Audio subsystem modules
//!
//! This module contains all audio-related functionality including:
//! - Audio device enumeration and management
//! - Audio engine for capture and playback
//! - Audio processing and effects
//! - Audio parameters and configuration
//! - JackTrip protocol implementation
//! - AudioWorklet integration
//! - Ring and jitter buffers
//! - WebRTC transport
//! - Hub server signaling

// Core audio modules
pub mod processor;
pub mod params;
pub mod devices;
pub mod worklet;
pub mod engine;

// Buffer modules
pub mod ring_buffer;
pub mod jitter_buffer;

// JackTrip protocol modules
pub mod protocol;
pub mod client;

// Transport and signaling modules
pub mod webrtc;
pub mod signaling;

// Re-export core audio types
pub use processor::AudioProcessor;
pub use params::AudioParams;
pub use devices::DeviceInfo;
pub use worklet::ProcessorHandle;
pub use engine::AudioEngine;

// Re-export buffer types
pub use ring_buffer::RingBuffer;
pub use jitter_buffer::{
    LockFreeJitterBuffer, JitterBuffer, JitterBufferConfig, JitterBufferStats,
};

// Re-export JackTrip types
pub use protocol::{AudioFormat, AudioPacket, PacketHeader, StreamStats};
pub use client::{AudioClient, JackTripConfig, ClientState};

// Re-export transport types
pub use webrtc::{ConnectionState, TransportConfig, WebRtcTransport};

// Re-export signaling types
pub use signaling::{HubSignaling, HubConnectionState, SignalingMessage};

