//! JackTrip Wire Protocol Implementation
//!
//! This module implements the JackTrip network audio protocol for WebRTC data channels.
//! The protocol is designed for low-latency, high-quality audio streaming.
//!
//! ## Packet Format
//!
//! ```text
//! +----------------+----------------+----------------+----------------+
//! |  Sequence (8)  |  Timestamp (8) |   Flags (1)    |  Channels (1)  |
//! +----------------+----------------+----------------+----------------+
//! | Sample Rate(4) | Buffer Size(2) |  Bit Depth(1)  |  Reserved(1)   |
//! +----------------+----------------+----------------+----------------+
//! |                    Audio Data (variable)                         |
//! +------------------------------------------------------------------+
//! ```
//!
//! Total header size: 26 bytes

use wasm_bindgen::prelude::*;

/// Header size in bytes
pub const HEADER_SIZE: usize = 26;

/// Default sample rate (48kHz)
pub const DEFAULT_SAMPLE_RATE: u32 = 48000;

/// Default buffer size (samples per channel per packet)
pub const DEFAULT_BUFFER_SIZE: u16 = 128;

/// Default bit depth (32-bit float)
pub const DEFAULT_BIT_DEPTH: u8 = 32;

/// Packet flags
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketFlags(u8);

impl PacketFlags {
    pub const NONE: PacketFlags = PacketFlags(0);
    pub const MUTED: PacketFlags = PacketFlags(1 << 0);
    pub const STEREO: PacketFlags = PacketFlags(1 << 1);
    pub const COMPRESSED: PacketFlags = PacketFlags(1 << 2);
    pub const HEARTBEAT: PacketFlags = PacketFlags(1 << 7);

    pub fn new() -> Self {
        Self::NONE
    }

    pub fn with_muted(self) -> Self {
        PacketFlags(self.0 | Self::MUTED.0)
    }

    pub fn with_stereo(self) -> Self {
        PacketFlags(self.0 | Self::STEREO.0)
    }

    pub fn is_muted(&self) -> bool {
        self.0 & Self::MUTED.0 != 0
    }

    pub fn is_stereo(&self) -> bool {
        self.0 & Self::STEREO.0 != 0
    }

    pub fn is_heartbeat(&self) -> bool {
        self.0 & Self::HEARTBEAT.0 != 0
    }

    pub fn to_byte(&self) -> u8 {
        self.0
    }

    pub fn from_byte(b: u8) -> Self {
        PacketFlags(b)
    }
}

impl Default for PacketFlags {
    fn default() -> Self {
        Self::NONE
    }
}

/// JackTrip packet header
#[derive(Debug, Clone, Copy)]
pub struct PacketHeader {
    /// Monotonically increasing sequence number for packet ordering
    pub sequence_number: u64,
    /// Timestamp in samples since stream start
    pub timestamp: u64,
    /// Packet flags (muted, stereo, etc.)
    pub flags: PacketFlags,
    /// Number of audio channels (1 or 2)
    pub num_channels: u8,
    /// Sample rate in Hz (typically 44100 or 48000)
    pub sample_rate: u32,
    /// Number of samples per channel in this packet
    pub buffer_size: u16,
    /// Bit depth (16, 24, or 32)
    pub bit_depth: u8,
}

impl PacketHeader {
    /// Create a new packet header with default settings (mono, 48kHz, 128 samples, 32-bit)
    pub fn new(sequence_number: u64, timestamp: u64) -> Self {
        Self {
            sequence_number,
            timestamp,
            flags: PacketFlags::NONE,
            num_channels: 1,
            sample_rate: DEFAULT_SAMPLE_RATE,
            buffer_size: DEFAULT_BUFFER_SIZE,
            bit_depth: DEFAULT_BIT_DEPTH,
        }
    }

    /// Create a stereo header
    pub fn stereo(sequence_number: u64, timestamp: u64) -> Self {
        Self {
            sequence_number,
            timestamp,
            flags: PacketFlags::NONE.with_stereo(),
            num_channels: 2,
            sample_rate: DEFAULT_SAMPLE_RATE,
            buffer_size: DEFAULT_BUFFER_SIZE,
            bit_depth: DEFAULT_BIT_DEPTH,
        }
    }

    /// Calculate the expected audio data size in bytes
    pub fn audio_data_size(&self) -> usize {
        let samples = self.buffer_size as usize * self.num_channels as usize;
        let bytes_per_sample = (self.bit_depth as usize + 7) / 8;
        samples * bytes_per_sample
    }

    /// Total packet size (header + audio data)
    pub fn total_packet_size(&self) -> usize {
        HEADER_SIZE + self.audio_data_size()
    }

    /// Serialize header to bytes (big-endian network order)
    pub fn serialize(&self, buffer: &mut [u8]) -> Result<(), ProtocolError> {
        if buffer.len() < HEADER_SIZE {
            return Err(ProtocolError::BufferTooSmall);
        }

        // Sequence number (8 bytes)
        buffer[0..8].copy_from_slice(&self.sequence_number.to_be_bytes());
        // Timestamp (8 bytes)
        buffer[8..16].copy_from_slice(&self.timestamp.to_be_bytes());
        // Flags (1 byte)
        buffer[16] = self.flags.to_byte();
        // Channels (1 byte)
        buffer[17] = self.num_channels;
        // Sample rate (4 bytes)
        buffer[18..22].copy_from_slice(&self.sample_rate.to_be_bytes());
        // Buffer size (2 bytes)
        buffer[22..24].copy_from_slice(&self.buffer_size.to_be_bytes());
        // Bit depth (1 byte)
        buffer[24] = self.bit_depth;
        // Reserved (1 byte)
        buffer[25] = 0;

        Ok(())
    }

    /// Deserialize header from bytes
    pub fn deserialize(buffer: &[u8]) -> Result<Self, ProtocolError> {
        if buffer.len() < HEADER_SIZE {
            return Err(ProtocolError::BufferTooSmall);
        }

        let sequence_number = u64::from_be_bytes(buffer[0..8].try_into().unwrap());
        let timestamp = u64::from_be_bytes(buffer[8..16].try_into().unwrap());
        let flags = PacketFlags::from_byte(buffer[16]);
        let num_channels = buffer[17];
        let sample_rate = u32::from_be_bytes(buffer[18..22].try_into().unwrap());
        let buffer_size = u16::from_be_bytes(buffer[22..24].try_into().unwrap());
        let bit_depth = buffer[24];

        // Validate
        if num_channels == 0 || num_channels > 2 {
            return Err(ProtocolError::InvalidChannelCount);
        }
        if ![16, 24, 32].contains(&bit_depth) {
            return Err(ProtocolError::InvalidBitDepth);
        }
        if buffer_size == 0 || buffer_size > 4096 {
            return Err(ProtocolError::InvalidBufferSize);
        }

        Ok(Self {
            sequence_number,
            timestamp,
            flags,
            num_channels,
            sample_rate,
            buffer_size,
            bit_depth,
        })
    }
}

/// A complete JackTrip audio packet
#[derive(Debug, Clone)]
pub struct AudioPacket {
    pub header: PacketHeader,
    /// Audio samples as 32-bit floats (interleaved for stereo)
    pub samples: Vec<f32>,
}

impl AudioPacket {
    /// Create a new audio packet
    pub fn new(header: PacketHeader, samples: Vec<f32>) -> Self {
        Self { header, samples }
    }

    /// Create a mono packet from samples
    pub fn mono(sequence_number: u64, timestamp: u64, samples: Vec<f32>) -> Self {
        let mut header = PacketHeader::new(sequence_number, timestamp);
        header.buffer_size = samples.len() as u16;
        Self { header, samples }
    }

    /// Create a stereo packet from interleaved samples
    pub fn stereo(sequence_number: u64, timestamp: u64, samples: Vec<f32>) -> Self {
        let mut header = PacketHeader::stereo(sequence_number, timestamp);
        header.buffer_size = (samples.len() / 2) as u16;
        Self { header, samples }
    }

    /// Create a heartbeat packet (no audio data)
    pub fn heartbeat(sequence_number: u64) -> Self {
        let mut header = PacketHeader::new(sequence_number, 0);
        header.flags = PacketFlags(PacketFlags::HEARTBEAT.0);
        header.buffer_size = 0;
        Self {
            header,
            samples: Vec::new(),
        }
    }

    /// Serialize the entire packet to bytes
    pub fn serialize(&self) -> Result<Vec<u8>, ProtocolError> {
        let total_size = self.header.total_packet_size();
        let mut buffer = vec![0u8; total_size];

        // Serialize header
        self.header.serialize(&mut buffer)?;

        // Serialize audio data as 32-bit floats (network byte order)
        let audio_start = HEADER_SIZE;
        for (i, sample) in self.samples.iter().enumerate() {
            let offset = audio_start + i * 4;
            if offset + 4 <= buffer.len() {
                buffer[offset..offset + 4].copy_from_slice(&sample.to_be_bytes());
            }
        }

        Ok(buffer)
    }

    /// Deserialize a packet from bytes
    pub fn deserialize(buffer: &[u8]) -> Result<Self, ProtocolError> {
        let header = PacketHeader::deserialize(buffer)?;
        
        let audio_data_size = header.audio_data_size();
        if buffer.len() < HEADER_SIZE + audio_data_size {
            return Err(ProtocolError::BufferTooSmall);
        }

        // For 32-bit float audio
        let num_samples = header.buffer_size as usize * header.num_channels as usize;
        let mut samples = Vec::with_capacity(num_samples);

        let audio_start = HEADER_SIZE;
        for i in 0..num_samples {
            let offset = audio_start + i * 4;
            let sample_bytes: [u8; 4] = buffer[offset..offset + 4].try_into().unwrap();
            samples.push(f32::from_be_bytes(sample_bytes));
        }

        Ok(Self { header, samples })
    }
}

/// Protocol errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    BufferTooSmall,
    InvalidChannelCount,
    InvalidBitDepth,
    InvalidBufferSize,
    InvalidPacket,
    SequenceGap,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolError::BufferTooSmall => write!(f, "Buffer too small"),
            ProtocolError::InvalidChannelCount => write!(f, "Invalid channel count"),
            ProtocolError::InvalidBitDepth => write!(f, "Invalid bit depth"),
            ProtocolError::InvalidBufferSize => write!(f, "Invalid buffer size"),
            ProtocolError::InvalidPacket => write!(f, "Invalid packet"),
            ProtocolError::SequenceGap => write!(f, "Sequence gap detected"),
        }
    }
}

/// Stream statistics for monitoring connection quality
#[wasm_bindgen]
#[derive(Debug, Clone, Default)]
pub struct StreamStats {
    /// Total packets sent
    pub packets_sent: u64,
    /// Total packets received
    pub packets_received: u64,
    /// Packets lost (based on sequence gaps)
    pub packets_lost: u64,
    /// Packets arrived out of order
    pub packets_out_of_order: u64,
    /// Current jitter estimate in milliseconds
    pub jitter_ms: f32,
    /// Round-trip time estimate in milliseconds
    pub rtt_ms: f32,
}

#[wasm_bindgen]
impl StreamStats {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Calculate packet loss percentage
    pub fn packet_loss_percent(&self) -> f32 {
        if self.packets_received + self.packets_lost == 0 {
            0.0
        } else {
            (self.packets_lost as f32 / (self.packets_received + self.packets_lost) as f32) * 100.0
        }
    }
}

/// Audio format configuration
#[wasm_bindgen]
#[derive(Debug, Clone, Copy)]
pub struct AudioFormat {
    pub sample_rate: u32,
    pub channels: u8,
    pub buffer_size: u16,
    pub bit_depth: u8,
}

#[wasm_bindgen]
impl AudioFormat {
    /// Create a mono format (most common for JackTrip)
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::mono()
    }

    pub fn mono() -> Self {
        Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            channels: 1,
            buffer_size: DEFAULT_BUFFER_SIZE,
            bit_depth: DEFAULT_BIT_DEPTH,
        }
    }

    pub fn stereo() -> Self {
        Self {
            sample_rate: DEFAULT_SAMPLE_RATE,
            channels: 2,
            buffer_size: DEFAULT_BUFFER_SIZE,
            bit_depth: DEFAULT_BIT_DEPTH,
        }
    }

    /// Samples per second (sample_rate * channels)
    pub fn samples_per_second(&self) -> u32 {
        self.sample_rate * self.channels as u32
    }

    /// Bytes per second of audio data
    pub fn bytes_per_second(&self) -> u32 {
        self.samples_per_second() * (self.bit_depth as u32 / 8)
    }

    /// Duration of one buffer in milliseconds
    pub fn buffer_duration_ms(&self) -> f32 {
        (self.buffer_size as f32 / self.sample_rate as f32) * 1000.0
    }

    /// Packets per second at this format
    pub fn packets_per_second(&self) -> f32 {
        self.sample_rate as f32 / self.buffer_size as f32
    }
}

impl Default for AudioFormat {
    fn default() -> Self {
        Self::mono()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_roundtrip() {
        let header = PacketHeader::new(42, 1000);
        let mut buffer = vec![0u8; HEADER_SIZE];
        header.serialize(&mut buffer).unwrap();
        let decoded = PacketHeader::deserialize(&buffer).unwrap();
        
        assert_eq!(header.sequence_number, decoded.sequence_number);
        assert_eq!(header.timestamp, decoded.timestamp);
        assert_eq!(header.num_channels, decoded.num_channels);
        assert_eq!(header.sample_rate, decoded.sample_rate);
    }

    #[test]
    fn test_packet_roundtrip() {
        let samples: Vec<f32> = (0..128).map(|i| (i as f32) / 128.0).collect();
        let packet = AudioPacket::mono(1, 0, samples.clone());
        
        let serialized = packet.serialize().unwrap();
        let decoded = AudioPacket::deserialize(&serialized).unwrap();
        
        assert_eq!(packet.header.sequence_number, decoded.header.sequence_number);
        assert_eq!(packet.samples.len(), decoded.samples.len());
        
        for (a, b) in packet.samples.iter().zip(decoded.samples.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }
}

