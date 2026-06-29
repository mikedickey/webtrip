//! JackTrip Wire Protocol Implementation
//!
//! This module implements the JackTrip network audio protocol for WebRTC data channels.
//! The protocol matches JackTrip's native C++ implementation for interoperability.
//!
//! ## Packet Format (DefaultHeaderStruct)
//!
//! The header format matches JackTrip's native 16-byte header:
//!
//! ```text
//! +----------------+----------------+----------------+----------------+
//! |            TimeStamp (8 bytes, little-endian)                     |
//! +----------------+----------------+----------------+----------------+
//! |  SeqNumber (2) | BufferSize (2) | SampleRate (1) | BitRes (1)     |
//! +----------------+----------------+----------------+----------------+
//! | NumInChans (1) | NumOutChans(1) |                                 |
//! +----------------+----------------+                                 +
//! |                    Audio Data (variable)                          |
//! +-------------------------------------------------------------------+
//! ```
//!
//! Total header size: 16 bytes
//! Multi-byte fields (TimeStamp, SeqNumber, BufferSize) use little-endian byte order.
//!
//! ## Sample Rate Encoding
//!
//! Sample rate is encoded as a single byte:
//! - 0 = 22050 Hz
//! - 1 = 32000 Hz  
//! - 2 = 44100 Hz
//! - 3 = 48000 Hz
//! - 4 = 88200 Hz
//! - 5 = 96000 Hz
//! - 6 = 192000 Hz
//!
//! ## NumOutgoingChannelsToNet Special Encoding
//!
//! The `NumOutgoingChannelsToNet` field uses a space-optimized encoding:
//! - **Value = 0**: Outgoing channels equals incoming channels (symmetric configuration)
//!   - This is the most common case and avoids redundant data
//!   - Example: If `NumIncomingChannelsFromNet = 2`, then outgoing is also 2
//! - **Value = 1-254**: Explicit outgoing channel count (asymmetric configuration)
//!   - Used when sender has different input/output channel counts
//! - **Value = 255 (0xFF)**: Special case indicating zero input channels
//!   - Sender is receive-only (no outgoing audio)

use wasm_bindgen::prelude::*;

/// JackTrip native header size (16 bytes)
pub const HEADER_SIZE: usize = 16;

/// Default sample rate (48kHz = code 3)
pub const DEFAULT_SAMPLE_RATE: u32 = 48000;
pub const DEFAULT_SAMPLE_RATE_CODE: u8 = 3;

/// Default buffer size (samples per channel per packet)
pub const DEFAULT_BUFFER_SIZE: u16 = 128;

/// Default bit depth (16-bit = 16)
pub const DEFAULT_BIT_DEPTH: u8 = 16;

/// Calculate audio data size in bytes for a given buffer configuration
fn audio_data_size(buffer_size: u16, channels: u8, bit_depth: u8) -> usize {
    let samples = buffer_size as usize * channels as usize;
    let bytes_per_sample = (bit_depth as usize + 7) / 8;
    samples * bytes_per_sample
}

/// Sample rate encoding (matches JackTrip's enum)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SampleRateCode {
    Sr22050 = 0,
    Sr32000 = 1,
    Sr44100 = 2,
    Sr48000 = 3,
    Sr88200 = 4,
    Sr96000 = 5,
    Sr192000 = 6,
}

impl SampleRateCode {
    pub fn from_hz(hz: u32) -> Self {
        match hz {
            22050 => Self::Sr22050,
            32000 => Self::Sr32000,
            44100 => Self::Sr44100,
            48000 => Self::Sr48000,
            88200 => Self::Sr88200,
            96000 => Self::Sr96000,
            192000 => Self::Sr192000,
            _ => Self::Sr48000, // Default
        }
    }

    pub fn to_hz(self) -> u32 {
        match self {
            Self::Sr22050 => 22050,
            Self::Sr32000 => 32000,
            Self::Sr44100 => 44100,
            Self::Sr48000 => 48000,
            Self::Sr88200 => 88200,
            Self::Sr96000 => 96000,
            Self::Sr192000 => 192000,
        }
    }

    pub fn from_byte(b: u8) -> Self {
        match b {
            0 => Self::Sr22050,
            1 => Self::Sr32000,
            2 => Self::Sr44100,
            3 => Self::Sr48000,
            4 => Self::Sr88200,
            5 => Self::Sr96000,
            6 => Self::Sr192000,
            _ => Self::Sr48000,
        }
    }
}

/// JackTrip packet header (matches DefaultHeaderStruct)
///
/// This structure matches the C++ JackTrip implementation exactly:
/// ```cpp
/// struct DefaultHeaderStruct {
///     uint64_t TimeStamp;                  // 8 bytes
///     uint16_t SeqNumber;                  // 2 bytes
///     uint16_t BufferSize;                 // 2 bytes
///     uint8_t  SamplingRate;               // 1 byte (encoded)
///     uint8_t  BitResolution;              // 1 byte
///     uint8_t  NumIncomingChannelsFromNet; // 1 byte
///     uint8_t  NumOutgoingChannelsToNet;   // 1 byte (special encoding)
/// };
/// ```
///
/// ## Note on `num_outgoing_channels` Field
///
/// This field is stored internally as the actual channel count, but uses special
/// encoding when serialized to the wire format:
/// - **0 on wire** = symmetric (outgoing equals incoming)
/// - **1-254 on wire** = explicit channel count
/// - **255 on wire** = receive-only (0 channels)
///
/// The encoding/decoding is handled automatically by `serialize()` and `deserialize()`.
#[derive(Debug, Clone, Copy)]
pub struct PacketHeader {
    /// Timestamp in samples since stream start (8 bytes)
    pub timestamp: u64,
    /// Sequence number for packet ordering (2 bytes, wraps at 65535)
    pub sequence_number: u16,
    /// Number of samples per channel in this packet (2 bytes)
    pub buffer_size: u16,
    /// Sample rate code (1 byte)
    pub sample_rate: SampleRateCode,
    /// Bit depth (1 byte): 8, 16, 24, or 32
    pub bit_depth: u8,
    /// Number of incoming audio channels (from network to us)
    pub num_incoming_channels: u8,
    /// Number of outgoing audio channels (from us to network)
    /// Note: This is the decoded value; wire format uses special encoding
    pub num_outgoing_channels: u8,
}

impl PacketHeader {
    /// Create a new packet header with default settings (mono, 48kHz, 128 samples, 16-bit)
    pub fn new(sequence_number: u16, timestamp: u64) -> Self {
        Self {
            timestamp,
            sequence_number,
            buffer_size: DEFAULT_BUFFER_SIZE,
            sample_rate: SampleRateCode::Sr48000,
            bit_depth: DEFAULT_BIT_DEPTH,
            num_incoming_channels: 1,
            num_outgoing_channels: 1,
        }
    }

    /// Create a stereo header
    pub fn stereo(sequence_number: u16, timestamp: u64) -> Self {
        Self {
            timestamp,
            sequence_number,
            buffer_size: DEFAULT_BUFFER_SIZE,
            sample_rate: SampleRateCode::Sr48000,
            bit_depth: DEFAULT_BIT_DEPTH,
            num_incoming_channels: 2,
            num_outgoing_channels: 2,
        }
    }

    /// Get sample rate in Hz
    pub fn sample_rate_hz(&self) -> u32 {
        self.sample_rate.to_hz()
    }

    /// Encode `num_outgoing_channels` according to JackTrip protocol spec
    ///
    /// This implements the space-optimized encoding where:
    /// - 0 means symmetric (outgoing = incoming)
    /// - 1-254 is explicit channel count
    /// - 255 means zero input channels (receive-only)
    fn encode_outgoing_channels(&self) -> u8 {
        if self.num_outgoing_channels == 0 {
            // Zero input channels (receive-only)
            255
        } else if self.num_outgoing_channels == self.num_incoming_channels {
            // Symmetric case - bandwidth optimization
            0
        } else {
            // Explicit count for asymmetric case
            self.num_outgoing_channels
        }
    }

    /// Decode `num_outgoing_channels` from wire format
    ///
    /// Takes the encoded value and the `num_incoming_channels` to resolve
    /// the actual outgoing channel count.
    fn decode_outgoing_channels(encoded: u8, num_incoming: u8) -> u8 {
        match encoded {
            0 => num_incoming,  // Symmetric: outgoing = incoming
            255 => 0,           // Receive-only: no outgoing channels
            n => n,             // Explicit count (1-254)
        }
    }

    /// Total packet size for outgoing packets (header + audio data)
    pub fn total_packet_size_out(&self) -> usize {
        HEADER_SIZE + audio_data_size(self.buffer_size, self.num_outgoing_channels, self.bit_depth)
    }

    /// Serialize header to bytes (little-endian byte order)
    pub fn serialize(&self, buffer: &mut [u8]) -> Result<(), ProtocolError> {
        if buffer.len() < HEADER_SIZE {
            return Err(ProtocolError::BufferTooSmall);
        }

        // TimeStamp (8 bytes, little-endian)
        buffer[0..8].copy_from_slice(&self.timestamp.to_le_bytes());
        // SeqNumber (2 bytes, little-endian)
        buffer[8..10].copy_from_slice(&self.sequence_number.to_le_bytes());
        // BufferSize (2 bytes, little-endian)
        buffer[10..12].copy_from_slice(&self.buffer_size.to_le_bytes());
        
        // SamplingRate (1 byte)
        buffer[12] = self.sample_rate as u8;
        // BitResolution (1 byte)
        buffer[13] = self.bit_depth;
        // NumIncomingChannelsFromNet (1 byte)
        buffer[14] = self.num_incoming_channels;
        // NumOutgoingChannelsToNet (1 byte) - use special encoding
        buffer[15] = self.encode_outgoing_channels();

        Ok(())
    }

    /// Deserialize header from bytes (little-endian byte order)
    pub fn deserialize(buffer: &[u8]) -> Result<Self, ProtocolError> {
        if buffer.len() < HEADER_SIZE {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::error_1(&format!("❌ Buffer too small: {} bytes (need {})", buffer.len(), HEADER_SIZE).into());
            return Err(ProtocolError::BufferTooSmall);
        }

        let timestamp = u64::from_le_bytes(buffer[0..8].try_into().unwrap());
        let sequence_number = u16::from_le_bytes(buffer[8..10].try_into().unwrap());
        let buffer_size = u16::from_le_bytes(buffer[10..12].try_into().unwrap());
        let sample_rate = SampleRateCode::from_byte(buffer[12]);
        let bit_depth = buffer[13];
        let num_incoming_channels = buffer[14];
        let num_outgoing_channels_encoded = buffer[15];

        // Decode the special encoding for outgoing channels
        let num_outgoing_channels = Self::decode_outgoing_channels(
            num_outgoing_channels_encoded,
            num_incoming_channels
        );

        // Validate - only num_incoming_channels matters for received packets
        // (it tells us how many channels of audio data are in this packet)
        if num_incoming_channels == 0 || num_incoming_channels > 8 {
            return Err(ProtocolError::InvalidChannelCount);
        }
        
        // Validate decoded outgoing channels
        // After decoding, it can be 0 (receive-only) or 1-8 (normal range)
        if num_outgoing_channels > 8 {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::error_1(&format!(
                "❌ Invalid outgoing channel count: {} (too high, max is 8)", 
                num_outgoing_channels
            ).into());
            return Err(ProtocolError::InvalidChannelCount);
        }
        
        if ![8, 16, 24, 32].contains(&bit_depth) {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::error_1(&format!(
                "❌ Invalid bit depth: {} (expected 8, 16, 24, or 32)", 
                bit_depth
            ).into());
            return Err(ProtocolError::InvalidBitDepth);
        }
        if buffer_size == 0 || buffer_size > 4096 {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::error_1(&format!(
                "❌ Invalid buffer size: {} (expected 1-4096)", 
                buffer_size
            ).into());
            return Err(ProtocolError::InvalidBufferSize);
        }

        Ok(Self {
            timestamp,
            sequence_number,
            buffer_size,
            sample_rate,
            bit_depth,
            num_incoming_channels,
            num_outgoing_channels,
        })
    }
}

/// Map an interleaved sample index to its non-interleaved (planar) wire position.
///
/// JackTrip's wire format stores multi-channel audio in planar order:
///   `[L0, L1, ..., L(N-1), R0, R1, ..., R(N-1)]`
///
/// Given an interleaved index `i` (where frame = `i / channels`, channel = `i % channels`),
/// this returns the corresponding position in the planar wire layout.
/// For mono (`channels == 1`) the mapping is the identity.
#[inline]
fn wire_idx(i: usize, channels: usize, buf_size: usize) -> usize {
    if channels > 1 {
        let frame = i / channels;
        let ch = i % channels;
        ch * buf_size + frame
    } else {
        i
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
    pub fn mono(sequence_number: u16, timestamp: u64, samples: Vec<f32>) -> Self {
        let mut header = PacketHeader::new(sequence_number, timestamp);
        header.buffer_size = samples.len() as u16;
        // bit_depth defaults to DEFAULT_BIT_DEPTH (16) in PacketHeader::new()
        Self { header, samples }
    }

    /// Create a stereo packet from interleaved samples
    pub fn stereo(sequence_number: u16, timestamp: u64, samples: Vec<f32>) -> Self {
        let mut header = PacketHeader::stereo(sequence_number, timestamp);
        header.buffer_size = (samples.len() / 2) as u16;
        // bit_depth defaults to DEFAULT_BIT_DEPTH (16) in PacketHeader::stereo()
        Self { header, samples }
    }

    /// Serialize samples directly into a buffer without creating an AudioPacket (no allocation)
    ///
    /// This is optimized for the send path where we want to avoid cloning the samples vector.
    /// Returns the number of bytes written.
    pub fn serialize_samples_into(
        sequence_number: u16,
        timestamp: u64,
        samples: &[f32],
        channels: u8,
        buffer: &mut [u8],
    ) -> Result<usize, ProtocolError> {
        // Create header inline
        let mut header = if channels == 1 {
            PacketHeader::new(sequence_number, timestamp)
        } else {
            PacketHeader::stereo(sequence_number, timestamp)
        };
        
        header.buffer_size = if channels == 1 {
            samples.len() as u16
        } else {
            (samples.len() / channels as usize) as u16
        };
        header.num_incoming_channels = channels;
        header.num_outgoing_channels = channels;

        let total_size = header.total_packet_size_out();
        if buffer.len() < total_size {
            return Err(ProtocolError::BufferTooSmall);
        }

        // Serialize header
        header.serialize(&mut buffer[..HEADER_SIZE])?;

        let audio_start = HEADER_SIZE;
        let num_channels = channels as usize;
        let buf_size = header.buffer_size as usize;

        match header.bit_depth {
            16 => {
                // 16-bit: convert f32 [-1.0, 1.0] to i16 (little-endian)
                for (i, &sample) in samples.iter().enumerate() {
                    let offset = audio_start + wire_idx(i, num_channels, buf_size) * 2;
                    let int_sample = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                    buffer[offset..offset + 2].copy_from_slice(&int_sample.to_le_bytes());
                }
            }
            _ => {
                // For other bit depths, fall back to the standard path
                // (in practice we always use 16-bit)
                return Err(ProtocolError::InvalidBitDepth);
            }
        }

        Ok(total_size)
    }

    /// Serialize the entire packet into a provided buffer (no allocation)
    ///
    /// Returns the number of bytes written.
    /// Audio samples are serialized according to the bit_depth field in the header.
    /// f32 samples in the range [-1.0, 1.0] are converted to the appropriate integer format.
    pub fn serialize_into(&self, buffer: &mut [u8]) -> Result<usize, ProtocolError> {
        let total_size = self.header.total_packet_size_out();
        if buffer.len() < total_size {
            return Err(ProtocolError::BufferTooSmall);
        }

        // Serialize header
        self.header.serialize(&mut buffer[..HEADER_SIZE])?;

        let audio_start = HEADER_SIZE;
        let num_channels = self.header.num_outgoing_channels as usize;
        let buf_size = self.header.buffer_size as usize;

        match self.header.bit_depth {
            8 => {
                // 8-bit: convert f32 [-1.0, 1.0] to i8
                for (i, &sample) in self.samples.iter().enumerate() {
                    let offset = audio_start + wire_idx(i, num_channels, buf_size);
                    let int_sample = (sample.clamp(-1.0, 1.0) * 128.0) as i8;
                    buffer[offset] = int_sample as u8;
                }
            }
            16 => {
                // 16-bit: convert f32 [-1.0, 1.0] to i16 (little-endian)
                for (i, &sample) in self.samples.iter().enumerate() {
                    let offset = audio_start + wire_idx(i, num_channels, buf_size) * 2;
                    let int_sample = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                    buffer[offset..offset + 2].copy_from_slice(&int_sample.to_le_bytes());
                }
            }
            24 => {
                // 24-bit: convert f32 [-1.0, 1.0] to i32 in 3 bytes (little-endian)
                for (i, &sample) in self.samples.iter().enumerate() {
                    let offset = audio_start + wire_idx(i, num_channels, buf_size) * 3;
                    let int_sample = (sample.clamp(-1.0, 1.0) * 8388607.0) as i32;
                    let bytes = int_sample.to_le_bytes();
                    buffer[offset..offset + 3].copy_from_slice(&bytes[0..3]);
                }
            }
            32 => {
                // 32-bit: serialize as f32 (little-endian)
                for (i, &sample) in self.samples.iter().enumerate() {
                    let offset = audio_start + wire_idx(i, num_channels, buf_size) * 4;
                    buffer[offset..offset + 4].copy_from_slice(&sample.to_le_bytes());
                }
            }
            _ => {
                return Err(ProtocolError::InvalidBitDepth);
            }
        }

        Ok(total_size)
    }

    /// Serialize the entire packet to bytes (allocating version, for compatibility)
    ///
    /// Audio samples are serialized according to the bit_depth field in the header.
    /// f32 samples in the range [-1.0, 1.0] are converted to the appropriate integer format.
    pub fn serialize(&self) -> Result<Vec<u8>, ProtocolError> {
        let total_size = self.header.total_packet_size_out();
        let mut buffer = vec![0u8; total_size];
        self.serialize_into(&mut buffer)?;
        Ok(buffer)
    }

    /// Deserialize a packet from bytes into a provided samples buffer (no allocation)
    ///
    /// Clears and fills the provided samples vector. Returns the packet header.
    pub fn deserialize_into(buffer: &[u8], samples: &mut Vec<f32>) -> Result<PacketHeader, ProtocolError> {
        let header = PacketHeader::deserialize(buffer)?;

        // For incoming packets, use num_incoming_channels
        let num_samples = header.buffer_size as usize * header.num_incoming_channels as usize;
        let bytes_per_sample = (header.bit_depth as usize + 7) / 8;
        let audio_data_size = num_samples * bytes_per_sample;

        if buffer.len() < HEADER_SIZE + audio_data_size {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::error_1(&format!(
                "❌ Buffer too small for audio data: got {} bytes, need {} (header) + {} (audio) = {} total",
                buffer.len(), HEADER_SIZE, audio_data_size, HEADER_SIZE + audio_data_size
            ).into());
            return Err(ProtocolError::BufferTooSmall);
        }

        // Reuse the provided buffer
        samples.clear();
        samples.reserve(num_samples);
        let audio_start = HEADER_SIZE;

        let num_channels = header.num_incoming_channels as usize;
        let buf_size = header.buffer_size as usize;

        // Deserialize based on bit depth
        match header.bit_depth {
            8 => {
                // 8-bit: signed i8, convert to f32 in range [-1.0, 1.0]
                for i in 0..num_samples {
                    let offset = audio_start + wire_idx(i, num_channels, buf_size);
                    let sample = buffer[offset] as i8;
                    samples.push(sample as f32 / 128.0);
                }
            }
            16 => {
                // 16-bit: signed i16 (little-endian), convert to f32 in range [-1.0, 1.0]
                for i in 0..num_samples {
                    let offset = audio_start + wire_idx(i, num_channels, buf_size) * 2;
                    let sample_bytes: [u8; 2] = buffer[offset..offset + 2].try_into().unwrap();
                    let sample = i16::from_le_bytes(sample_bytes);
                    samples.push(sample as f32 / 32768.0);
                }
            }
            24 => {
                // 24-bit: signed i32 in 3 bytes (little-endian), convert to f32
                for i in 0..num_samples {
                    let offset = audio_start + wire_idx(i, num_channels, buf_size) * 3;
                    // Read 3 bytes and sign-extend to i32
                    let b0 = buffer[offset] as i32;
                    let b1 = buffer[offset + 1] as i32;
                    let b2 = buffer[offset + 2] as i32;
                    let sample = (b0 | (b1 << 8) | (b2 << 16)) << 8 >> 8; // Sign extend
                    samples.push(sample as f32 / 8388608.0); // 2^23
                }
            }
            32 => {
                // 32-bit: could be i32 or f32, assume f32 for now
                for i in 0..num_samples {
                    let offset = audio_start + wire_idx(i, num_channels, buf_size) * 4;
                    let sample_bytes: [u8; 4] = buffer[offset..offset + 4].try_into().unwrap();
                    samples.push(f32::from_le_bytes(sample_bytes));
                }
            }
            _ => {
                return Err(ProtocolError::InvalidBitDepth);
            }
        }

        Ok(header)
    }

    /// Deserialize a packet from bytes (allocating version, for compatibility)
    pub fn deserialize(buffer: &[u8]) -> Result<Self, ProtocolError> {
        let mut samples = Vec::new();
        let header = Self::deserialize_into(buffer, &mut samples)?;
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

/// Fixed byte length of a JackTrip control packet (and therefore an exit packet).
pub const CONTROL_PACKET_SIZE: usize = 63;

/// Builds a JackTrip exit packet: a 63-byte control packet with every byte set to `0xFF`.
///
/// The hub server recognizes an all-`0xFF` control packet as a graceful disconnect
/// signal, allowing it to reclaim the client slot immediately rather than waiting for
/// an inactivity timeout.
pub fn make_exit_packet() -> Vec<u8> {
    vec![0xFF; CONTROL_PACKET_SIZE]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_exit_packet() {
        let pkt = make_exit_packet();
        assert_eq!(pkt.len(), CONTROL_PACKET_SIZE);
        assert_eq!(pkt.len(), 63);
        assert!(pkt.iter().all(|&b| b == 0xFF));
    }

    #[test]
    fn test_header_size() {
        // Ensure our header is exactly 16 bytes as expected by JackTrip
        assert_eq!(HEADER_SIZE, 16);
    }

    #[test]
    fn test_header_roundtrip() {
        let header = PacketHeader::new(42, 1000);
        let mut buffer = vec![0u8; HEADER_SIZE];
        header.serialize(&mut buffer).unwrap();
        let decoded = PacketHeader::deserialize(&buffer).unwrap();

        assert_eq!(header.sequence_number, decoded.sequence_number);
        assert_eq!(header.timestamp, decoded.timestamp);
        assert_eq!(header.num_incoming_channels, decoded.num_incoming_channels);
        assert_eq!(header.sample_rate as u8, decoded.sample_rate as u8);
    }

    #[test]
    fn test_packet_roundtrip_mono() {
        let samples: Vec<f32> = (0..128).map(|i| (i as f32) / 128.0).collect();
        let packet = AudioPacket::mono(1, 0, samples.clone());

        let serialized = packet.serialize().unwrap();
        let decoded = AudioPacket::deserialize(&serialized).unwrap();

        assert_eq!(packet.header.sequence_number, decoded.header.sequence_number);
        assert_eq!(packet.samples.len(), decoded.samples.len());

        for (a, b) in packet.samples.iter().zip(decoded.samples.iter()) {
            assert!((a - b).abs() < 1e-4);
        }
    }

    #[test]
    fn test_mono_wire_layout_unchanged() {
        // For mono, interleaved == non-interleaved; serialization must not reorder samples.
        let samples: Vec<f32> = vec![0.1, 0.2, 0.3, 0.4];
        let mut header = PacketHeader::new(0, 0);
        header.buffer_size = 4;
        let packet = AudioPacket::new(header, samples.clone());

        let serialized = packet.serialize().unwrap();

        // Read wire samples sequentially and verify they match input order
        for (i, &expected) in samples.iter().enumerate() {
            let offset = HEADER_SIZE + i * 2;
            let wire_val = i16::from_le_bytes(serialized[offset..offset + 2].try_into().unwrap());
            let wire_f32 = wire_val as f32 / 32768.0;
            assert!((wire_f32 - expected).abs() < 1e-4, "mono sample {i} mismatch");
        }
    }

    #[test]
    fn test_stereo_wire_layout_noninterleaved() {
        // Stereo interleaved input: [L0, R0, L1, R1]
        // Expected wire layout (non-interleaved): [L0, L1, R0, R1]
        let buf_size: usize = 2;
        let samples: Vec<f32> = vec![0.1, 0.5, 0.2, 0.6]; // [L0, R0, L1, R1]

        let mut header = PacketHeader::stereo(0, 0);
        header.buffer_size = buf_size as u16;
        let packet = AudioPacket::new(header, samples.clone());

        let serialized = packet.serialize().unwrap();

        let read_wire = |idx: usize| -> f32 {
            let offset = HEADER_SIZE + idx * 2;
            let wire_val = i16::from_le_bytes(serialized[offset..offset + 2].try_into().unwrap());
            wire_val as f32 / 32768.0
        };

        // Wire positions 0..buf_size are channel 0 (left): L0, L1
        assert!((read_wire(0) - 0.1).abs() < 1e-4, "wire[0] should be L0");
        assert!((read_wire(1) - 0.2).abs() < 1e-4, "wire[1] should be L1");
        // Wire positions buf_size..2*buf_size are channel 1 (right): R0, R1
        assert!((read_wire(2) - 0.5).abs() < 1e-4, "wire[2] should be R0");
        assert!((read_wire(3) - 0.6).abs() < 1e-4, "wire[3] should be R1");
    }

    #[test]
    fn test_stereo_roundtrip() {
        // Verify that serialize → deserialize preserves interleaved stereo samples.
        let buf_size = 128usize;
        let samples: Vec<f32> = (0..buf_size * 2)
            .map(|i| if i % 2 == 0 { (i as f32) / 256.0 } else { -(i as f32) / 256.0 })
            .collect();

        let packet = AudioPacket::stereo(5, 1000, samples.clone());
        let serialized = packet.serialize().unwrap();
        let decoded = AudioPacket::deserialize(&serialized).unwrap();

        assert_eq!(decoded.header.num_incoming_channels, 2);
        assert_eq!(decoded.samples.len(), samples.len());

        for (i, (a, b)) in samples.iter().zip(decoded.samples.iter()).enumerate() {
            assert!((a - b).abs() < 1e-4, "stereo sample {i} mismatch: {a} vs {b}");
        }
    }

    #[test]
    fn test_sample_rate_encoding() {
        assert_eq!(SampleRateCode::from_hz(48000) as u8, 3);
        assert_eq!(SampleRateCode::Sr48000.to_hz(), 48000);
        assert_eq!(SampleRateCode::from_byte(3).to_hz(), 48000);
    }

    #[test]
    fn test_sample_rate_from_hz_all_rates_and_fallback() {
        // Every mapped rate must map to the matching variant and round-trip via to_hz().
        let mapped = [
            (22050u32, SampleRateCode::Sr22050),
            (32000, SampleRateCode::Sr32000),
            (44100, SampleRateCode::Sr44100),
            (48000, SampleRateCode::Sr48000),
            (88200, SampleRateCode::Sr88200),
            (96000, SampleRateCode::Sr96000),
            (192000, SampleRateCode::Sr192000),
        ];
        for (hz, code) in mapped {
            assert_eq!(SampleRateCode::from_hz(hz), code, "from_hz({hz})");
            assert_eq!(code.to_hz(), hz, "to_hz round-trip for {hz}");
        }

        // Unknown rates fall back to the 48 kHz default.
        assert_eq!(SampleRateCode::from_hz(11025), SampleRateCode::Sr48000);
        assert_eq!(SampleRateCode::from_hz(0), SampleRateCode::Sr48000);
    }

    #[test]
    fn test_sample_rate_from_byte_all_bytes_and_fallback() {
        let expected = [
            SampleRateCode::Sr22050,
            SampleRateCode::Sr32000,
            SampleRateCode::Sr44100,
            SampleRateCode::Sr48000,
            SampleRateCode::Sr88200,
            SampleRateCode::Sr96000,
            SampleRateCode::Sr192000,
        ];
        for (b, code) in expected.iter().enumerate() {
            assert_eq!(SampleRateCode::from_byte(b as u8), *code, "from_byte({b})");
        }

        // Out-of-range bytes fall back to the 48 kHz default.
        assert_eq!(SampleRateCode::from_byte(7), SampleRateCode::Sr48000);
        assert_eq!(SampleRateCode::from_byte(255), SampleRateCode::Sr48000);
    }

    #[test]
    fn test_audio_data_size_across_depths_and_channels() {
        // payload = buffer_size * channels * ceil(bit_depth / 8)
        assert_eq!(audio_data_size(128, 1, 8), 128);
        assert_eq!(audio_data_size(128, 1, 16), 256);
        assert_eq!(audio_data_size(128, 1, 24), 384);
        assert_eq!(audio_data_size(128, 1, 32), 512);

        assert_eq!(audio_data_size(64, 2, 16), 256);
        assert_eq!(audio_data_size(32, 8, 24), 768);
    }

    #[test]
    fn test_audio_data_size_matches_serialize_samples_into() {
        // serialize_samples_into is 16-bit only; the bytes it writes minus the
        // header must equal audio_data_size for the same configuration.
        for (channels, frames) in [(1u8, 128usize), (2, 64)] {
            let samples = vec![0.0f32; frames * channels as usize];
            let buf_size = frames as u16;
            let expected_audio = audio_data_size(buf_size, channels, 16);

            let mut buffer = vec![0u8; HEADER_SIZE + expected_audio];
            let written = AudioPacket::serialize_samples_into(0, 0, &samples, channels, &mut buffer)
                .unwrap();

            assert_eq!(written - HEADER_SIZE, expected_audio, "channels={channels}");
        }
    }

    #[test]
    fn test_audio_data_size_matches_serialize_into_all_depths() {
        // Cross-check audio_data_size against the full serialize path for every
        // supported bit depth rather than re-deriving the wire layout.
        let buf_size: u16 = 16;
        let channels: u8 = 2;
        let samples = vec![0.0f32; buf_size as usize * channels as usize];

        for bit_depth in [8u8, 16, 24, 32] {
            let mut header = PacketHeader::stereo(0, 0);
            header.buffer_size = buf_size;
            header.bit_depth = bit_depth;
            let packet = AudioPacket::new(header, samples.clone());

            let serialized = packet.serialize().unwrap();
            let expected_audio = audio_data_size(buf_size, channels, bit_depth);

            assert_eq!(
                serialized.len() - HEADER_SIZE,
                expected_audio,
                "bit_depth={bit_depth}"
            );
        }
    }

    #[test]
    fn test_wire_idx_mono_is_identity() {
        let buf_size = 8;
        for i in 0..buf_size {
            assert_eq!(wire_idx(i, 1, buf_size), i, "mono index {i}");
        }
    }

    #[test]
    fn test_wire_idx_stereo_interleaving() {
        // Interleaved input [L0, R0, L1, R1] (channels=2, buf_size=2) maps to the
        // planar wire layout [L0, L1, R0, R1].
        let buf_size = 2;
        assert_eq!(wire_idx(0, 2, buf_size), 0); // L0 -> wire[0]
        assert_eq!(wire_idx(1, 2, buf_size), 2); // R0 -> wire[2]
        assert_eq!(wire_idx(2, 2, buf_size), 1); // L1 -> wire[1]
        assert_eq!(wire_idx(3, 2, buf_size), 3); // R1 -> wire[3]
    }

    #[test]
    fn test_packet_loss_percent() {
        // No packets seen yet -> avoid divide-by-zero, report 0%.
        let stats = StreamStats::new();
        assert_eq!(stats.packet_loss_percent(), 0.0);

        // 1 lost out of 100 total -> 1%.
        let mut normal = StreamStats::new();
        normal.packets_received = 99;
        normal.packets_lost = 1;
        assert!((normal.packet_loss_percent() - 1.0).abs() < 1e-6);

        // Every packet lost -> 100%.
        let mut total_loss = StreamStats::new();
        total_loss.packets_received = 0;
        total_loss.packets_lost = 10;
        assert!((total_loss.packet_loss_percent() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn test_audio_format_helpers() {
        // mono: 48000 Hz, 1 ch, 128-sample buffer, 16-bit
        let mono = AudioFormat::mono();
        assert_eq!(mono.samples_per_second(), 48000);
        assert_eq!(mono.bytes_per_second(), 96000); // 48000 * 1 * 2
        assert!((mono.buffer_duration_ms() - (128.0 / 48000.0 * 1000.0)).abs() < 1e-6);
        assert!((mono.packets_per_second() - 375.0).abs() < 1e-6); // 48000 / 128

        // stereo: 48000 Hz, 2 ch, 128-sample buffer, 16-bit
        let stereo = AudioFormat::stereo();
        assert_eq!(stereo.samples_per_second(), 96000);
        assert_eq!(stereo.bytes_per_second(), 192000); // 96000 * 2
        assert!((stereo.buffer_duration_ms() - (128.0 / 48000.0 * 1000.0)).abs() < 1e-6);
        assert!((stereo.packets_per_second() - 375.0).abs() < 1e-6);

        // Non-default format: 44100 Hz, 2 ch, 256-sample buffer, 24-bit
        let custom = AudioFormat {
            sample_rate: 44100,
            channels: 2,
            buffer_size: 256,
            bit_depth: 24,
        };
        assert_eq!(custom.samples_per_second(), 88200); // 44100 * 2
        assert_eq!(custom.bytes_per_second(), 264600); // 88200 * 3
        assert!((custom.buffer_duration_ms() - (256.0 / 44100.0 * 1000.0)).abs() < 1e-6);
        assert!((custom.packets_per_second() - (44100.0 / 256.0)).abs() < 1e-6);
    }

    #[test]
    fn test_outgoing_channels_encoding_symmetric() {
        // Symmetric case: outgoing = incoming should encode to 0
        let mut header = PacketHeader::new(1, 0);
        header.num_incoming_channels = 2;
        header.num_outgoing_channels = 2;
        
        assert_eq!(header.encode_outgoing_channels(), 0);
        
        // Serialize and deserialize to verify roundtrip
        let mut buffer = vec![0u8; HEADER_SIZE];
        header.serialize(&mut buffer).unwrap();
        
        // Byte 15 should be 0 (encoded value)
        assert_eq!(buffer[15], 0);
        
        let decoded = PacketHeader::deserialize(&buffer).unwrap();
        assert_eq!(decoded.num_outgoing_channels, 2);
    }

    #[test]
    fn test_outgoing_channels_encoding_asymmetric() {
        // Asymmetric case: different in/out should encode to explicit count
        let mut header = PacketHeader::new(1, 0);
        header.num_incoming_channels = 2;
        header.num_outgoing_channels = 4;
        
        assert_eq!(header.encode_outgoing_channels(), 4);
        
        // Serialize and deserialize to verify roundtrip
        let mut buffer = vec![0u8; HEADER_SIZE];
        header.serialize(&mut buffer).unwrap();
        
        // Byte 15 should be 4 (encoded value)
        assert_eq!(buffer[15], 4);
        
        let decoded = PacketHeader::deserialize(&buffer).unwrap();
        assert_eq!(decoded.num_outgoing_channels, 4);
    }

    #[test]
    fn test_outgoing_channels_encoding_receive_only() {
        // Receive-only case: 0 outgoing channels should encode to 255
        let mut header = PacketHeader::new(1, 0);
        header.num_incoming_channels = 2;
        header.num_outgoing_channels = 0;
        
        assert_eq!(header.encode_outgoing_channels(), 255);
        
        // Serialize and deserialize to verify roundtrip
        let mut buffer = vec![0u8; HEADER_SIZE];
        header.serialize(&mut buffer).unwrap();
        
        // Byte 15 should be 255 (encoded value)
        assert_eq!(buffer[15], 255);
        
        let decoded = PacketHeader::deserialize(&buffer).unwrap();
        assert_eq!(decoded.num_outgoing_channels, 0);
    }

    #[test]
    fn test_outgoing_channels_decoding() {
        // Test decode_outgoing_channels static method
        assert_eq!(PacketHeader::decode_outgoing_channels(0, 2), 2);    // Symmetric
        assert_eq!(PacketHeader::decode_outgoing_channels(4, 2), 4);    // Explicit
        assert_eq!(PacketHeader::decode_outgoing_channels(255, 2), 0);  // Receive-only
        
        // Verify all explicit values 1-254 pass through unchanged
        for n in 1..=254 {
            assert_eq!(PacketHeader::decode_outgoing_channels(n, 2), n);
        }
    }

    // ----- Malformed-header rejection branches (PacketHeader::deserialize) -----
    //
    // These tests build a *valid* header with the protocol's own serialize
    // helper, then mutate a single field to the value under test. This reuses
    // the real wire layout instead of hand-rolling byte buffers (per AGENTS.md).

    /// Serialize a `PacketHeader` into a fresh 16-byte buffer using the real
    /// protocol helper. `serialize()` performs no validation, so this is the
    /// canonical way to construct both valid and deliberately-invalid headers.
    fn header_bytes(header: &PacketHeader) -> Vec<u8> {
        let mut buffer = vec![0u8; HEADER_SIZE];
        header.serialize(&mut buffer).unwrap();
        buffer
    }

    /// A baseline header whose every field is valid: mono, 48 kHz, 128-sample
    /// buffer, 16-bit. Tests mutate one field at a time off of this.
    fn valid_header() -> PacketHeader {
        PacketHeader::new(7, 12345)
    }

    #[test]
    fn test_deserialize_rejects_buffer_too_small() {
        // Any buffer shorter than the 16-byte header is rejected outright.
        for len in [0usize, 1, 8, 15] {
            let buffer = vec![0u8; len];
            assert_eq!(
                PacketHeader::deserialize(&buffer).unwrap_err(),
                ProtocolError::BufferTooSmall,
                "len={len} should be too small"
            );
        }

        // Exactly 16 bytes (a full, valid header) is accepted.
        let buffer = header_bytes(&valid_header());
        assert!(PacketHeader::deserialize(&buffer).is_ok());
    }

    #[test]
    fn test_deserialize_incoming_channel_count_boundaries() {
        // 0 channels is invalid (zero audio channels makes no sense).
        let mut header = valid_header();
        header.num_incoming_channels = 0;
        // Keep outgoing explicit & valid so the incoming check is what fires.
        header.num_outgoing_channels = 1;
        assert_eq!(
            PacketHeader::deserialize(&header_bytes(&header)).unwrap_err(),
            ProtocolError::InvalidChannelCount,
            "0 incoming channels must be rejected"
        );

        // 1..=8 channels are the valid range.
        for ch in 1u8..=8 {
            let mut header = valid_header();
            header.num_incoming_channels = ch;
            header.num_outgoing_channels = ch; // symmetric -> encodes to 0
            let decoded = PacketHeader::deserialize(&header_bytes(&header))
                .unwrap_or_else(|e| panic!("{ch} incoming channels should be valid: {e:?}"));
            assert_eq!(decoded.num_incoming_channels, ch);
        }

        // 9 channels (one past the max) is invalid.
        let mut header = valid_header();
        header.num_incoming_channels = 9;
        header.num_outgoing_channels = 1;
        assert_eq!(
            PacketHeader::deserialize(&header_bytes(&header)).unwrap_err(),
            ProtocolError::InvalidChannelCount,
            "9 incoming channels must be rejected"
        );
    }

    #[test]
    fn test_deserialize_outgoing_channel_count_boundaries() {
        // Decoded outgoing channels are validated separately from incoming.
        // Use an explicit (asymmetric) encoding so the wire byte equals the
        // outgoing count directly. Incoming stays valid (2) throughout.

        // Decoded outgoing == 8 is the highest valid value.
        let mut header = valid_header();
        header.num_incoming_channels = 2;
        header.num_outgoing_channels = 8;
        let decoded = PacketHeader::deserialize(&header_bytes(&header))
            .expect("8 outgoing channels should be valid");
        assert_eq!(decoded.num_outgoing_channels, 8);

        // Decoded outgoing == 9 is one past the max and must be rejected.
        let mut header = valid_header();
        header.num_incoming_channels = 2;
        header.num_outgoing_channels = 9;
        assert_eq!(
            PacketHeader::deserialize(&header_bytes(&header)).unwrap_err(),
            ProtocolError::InvalidChannelCount,
            "9 outgoing channels must be rejected"
        );

        // A large explicit outgoing count (still < 255, so not the receive-only
        // sentinel) is likewise rejected.
        let mut header = valid_header();
        header.num_incoming_channels = 2;
        header.num_outgoing_channels = 200;
        assert_eq!(
            PacketHeader::deserialize(&header_bytes(&header)).unwrap_err(),
            ProtocolError::InvalidChannelCount,
            "200 outgoing channels must be rejected"
        );
    }

    #[test]
    fn test_deserialize_bit_depth_boundaries() {
        // Only 8, 16, 24, 32 are accepted.
        for depth in [8u8, 16, 24, 32] {
            let mut header = valid_header();
            header.bit_depth = depth;
            let decoded = PacketHeader::deserialize(&header_bytes(&header))
                .unwrap_or_else(|e| panic!("bit depth {depth} should be valid: {e:?}"));
            assert_eq!(decoded.bit_depth, depth);
        }

        // Everything else is InvalidBitDepth, including values adjacent to the
        // valid set (7, 33) and 0.
        for depth in [0u8, 7, 9, 12, 33, 64, 255] {
            let mut header = valid_header();
            header.bit_depth = depth;
            assert_eq!(
                PacketHeader::deserialize(&header_bytes(&header)).unwrap_err(),
                ProtocolError::InvalidBitDepth,
                "bit depth {depth} must be rejected"
            );
        }
    }

    #[test]
    fn test_deserialize_buffer_size_boundaries() {
        // buffer_size 0 is invalid (empty packet).
        let mut header = valid_header();
        header.buffer_size = 0;
        assert_eq!(
            PacketHeader::deserialize(&header_bytes(&header)).unwrap_err(),
            ProtocolError::InvalidBufferSize,
            "buffer_size 0 must be rejected"
        );

        // 1 and 4096 are the inclusive valid bounds.
        for size in [1u16, 4096] {
            let mut header = valid_header();
            header.buffer_size = size;
            let decoded = PacketHeader::deserialize(&header_bytes(&header))
                .unwrap_or_else(|e| panic!("buffer_size {size} should be valid: {e:?}"));
            assert_eq!(decoded.buffer_size, size);
        }

        // 4097 (one past the max) is invalid.
        let mut header = valid_header();
        header.buffer_size = 4097;
        assert_eq!(
            PacketHeader::deserialize(&header_bytes(&header)).unwrap_err(),
            ProtocolError::InvalidBufferSize,
            "buffer_size 4097 must be rejected"
        );
    }

    // ----- AudioPacket::deserialize_into payload guards & bit-depth arms -----

    #[test]
    fn test_deserialize_into_rejects_truncated_audio() {
        // A header that is itself valid but whose declared payload is larger
        // than the bytes actually present must be rejected as BufferTooSmall.
        // Mono / 128 samples / 16-bit => 256 bytes of audio expected.
        let buffer = header_bytes(&valid_header());
        let mut samples = Vec::new();
        // PacketHeader has no PartialEq, so assert on the error value directly.
        assert_eq!(
            AudioPacket::deserialize_into(&buffer, &mut samples).unwrap_err(),
            ProtocolError::BufferTooSmall,
            "header-only buffer with non-empty declared payload must be rejected"
        );

        // One byte short of the full payload is still rejected.
        let full = AudioPacket::mono(0, 0, vec![0.25f32; 128]).serialize().unwrap();
        let truncated = &full[..full.len() - 1];
        assert_eq!(
            AudioPacket::deserialize_into(truncated, &mut samples).unwrap_err(),
            ProtocolError::BufferTooSmall,
            "payload one byte short must be rejected"
        );

        // The exact-length buffer succeeds (boundary on the valid side).
        assert!(AudioPacket::deserialize_into(&full, &mut samples).is_ok());
    }

    /// Build an `AudioPacket` with the given channel count and bit depth, filled
    /// with deterministic samples spanning negative and positive values.
    fn make_packet(channels: u8, bit_depth: u8, buf_size: u16) -> AudioPacket {
        let n = buf_size as usize * channels as usize;
        let samples: Vec<f32> = (0..n)
            .map(|i| (i as f32 / n as f32) - 0.5) // range [-0.5, 0.5)
            .collect();
        let mut header = if channels == 1 {
            PacketHeader::new(0, 0)
        } else {
            PacketHeader::stereo(0, 0)
        };
        header.buffer_size = buf_size;
        header.bit_depth = bit_depth;
        AudioPacket::new(header, samples)
    }

    #[test]
    fn test_deserialize_into_all_bit_depths_roundtrip() {
        // Exercises the 8/16/24/32-bit decode arms of deserialize_into for both
        // mono and stereo (planar) wire layouts.
        for bit_depth in [8u8, 16, 24, 32] {
            // 8-bit has only ~1/128 resolution; the rest are far finer.
            let tol = if bit_depth == 8 { 1e-2 } else { 1e-4 };
            for channels in [1u8, 2] {
                let packet = make_packet(channels, bit_depth, 16);
                let serialized = packet.serialize().unwrap();
                let decoded = AudioPacket::deserialize(&serialized).unwrap();

                assert_eq!(
                    decoded.samples.len(),
                    packet.samples.len(),
                    "sample count, depth={bit_depth} channels={channels}"
                );
                for (i, (a, b)) in packet.samples.iter().zip(decoded.samples.iter()).enumerate() {
                    assert!(
                        (a - b).abs() < tol,
                        "depth={bit_depth} channels={channels} sample {i}: {a} vs {b}"
                    );
                }
            }
        }
    }

    // ----- ProtocolError Display & AudioFormat constructors -----

    #[test]
    fn test_protocol_error_display() {
        // Every variant must render a distinct, non-empty human-readable string.
        let variants = [
            ProtocolError::BufferTooSmall,
            ProtocolError::InvalidChannelCount,
            ProtocolError::InvalidBitDepth,
            ProtocolError::InvalidBufferSize,
            ProtocolError::InvalidPacket,
            ProtocolError::SequenceGap,
        ];
        let mut seen = Vec::new();
        for v in variants {
            let s = v.to_string();
            assert!(!s.is_empty(), "{v:?} should render a message");
            assert!(!seen.contains(&s), "{v:?} message must be unique");
            seen.push(s);
        }
    }

    #[test]
    fn test_audio_format_constructor_and_default() {
        // The wasm_bindgen constructor and the Default impl both delegate to
        // mono(); confirm they produce the documented mono configuration.
        let via_new = AudioFormat::new();
        let via_default = AudioFormat::default();
        let mono = AudioFormat::mono();

        for fmt in [via_new, via_default] {
            assert_eq!(fmt.sample_rate, mono.sample_rate);
            assert_eq!(fmt.channels, mono.channels);
            assert_eq!(fmt.buffer_size, mono.buffer_size);
            assert_eq!(fmt.bit_depth, mono.bit_depth);
        }

        assert_eq!(via_new.sample_rate, DEFAULT_SAMPLE_RATE);
        assert_eq!(via_new.channels, 1);
        assert_eq!(via_new.buffer_size, DEFAULT_BUFFER_SIZE);
        assert_eq!(via_new.bit_depth, DEFAULT_BIT_DEPTH);
    }
}

