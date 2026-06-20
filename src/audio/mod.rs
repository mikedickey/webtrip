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
pub mod audio_callback_loop;

// Buffer modules
pub mod ring_buffer;
pub mod regulator;

// JackTrip protocol modules
pub mod protocol;

// Transport and signaling modules
pub mod transport;
pub mod webrtc;
pub mod mock_transport;
pub mod webtransport;
pub mod webtransport_worker;
pub mod signaling;

// Re-export core audio types
pub use processor::AudioProcessor;
pub use params::AudioParams;
pub use devices::DeviceInfo;
pub use worklet::ProcessorHandle;
pub use engine::AudioEngine;
pub use audio_callback_loop::{AudioCallbackLoop, has_atomics_wait_async};

// Re-export buffer types
pub use ring_buffer::RingBuffer;
pub use regulator::{Regulator, RegulatorStats};

// Re-export JackTrip types
pub use protocol::{AudioFormat, AudioPacket, PacketHeader, StreamStats};

// Re-export transport types
pub use transport::{Transport, TransportType, TransportState};
pub use webrtc::{TransportConfig, WebRtcTransport};
pub use mock_transport::{MockTransport, SineWaveConfig};
pub use webtransport::WebTransportImpl;

// Re-export signaling types
pub use signaling::{HubSignaling, HubConnectionState, SignalingMessage};

