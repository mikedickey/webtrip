//! The bounds every layer agrees on: channel counts, frames per packet, sample
//! rates, packet size and connection count.
//!
//! Wire limits (what a network stream may carry) and device limits (what a
//! local audio device may expose) are independent. Each is a rail with a stated
//! reason, never a silent ceiling.

/// Maximum audio channels a single network stream may carry.
///
/// A WebTrip choice, not a JackTrip constraint: the wire header's channel
/// fields are single bytes with a special encoding above 254 (see
/// `network::protocol`). Kept tight because packet size is
/// `16 + fpp * channels * 2` bytes and QUIC datagrams / unfragmented SCTP
/// messages must fit one ~1200-byte packet. `stream::regulator` mirrors this
/// value deliberately (that module has no `crate::` imports); a test pins the two.
pub const MAX_WIRE_CHANNELS: u8 = 8;

/// Maximum channels of a local audio device (capture or playback) the engine
/// will address. Bounds the audio host's scratch buffers and the
/// [`AudioProcessor`](crate::audio::processor::AudioProcessor) device-width
/// buffers, which are preallocated at this width so a wide interface never
/// allocates on the render thread.
pub const MAX_DEVICE_CHANNELS: u8 = 32;

/// Fewest frames per packet a stream may carry, in either direction: JackTrip's
/// low-latency setting. Header overhead at small fpp is a cost, not a
/// correctness limit, so it sets no higher floor. `stream::regulator` mirrors
/// this value (no `crate::` imports there); a test pins the two.
pub const MIN_FPP: usize = 16;

/// Most frames per packet a stream may carry, in either direction: JackTrip's
/// largest common buffer size. Bounds the regulator's preallocated per-packet
/// state, so a peer announcing more is refused rather than reallocating on the
/// audio thread. Mirrored by `stream::regulator`, pinned by a test.
pub const MAX_FPP: usize = 1024;

/// Most network connections a session holds at once. The session is built as a
/// list of connections (each with its own stream buffers); only one is
/// supported, and a connect beyond the limit is refused with an error naming
/// it rather than ignored.
pub const MAX_CONNECTIONS: usize = 1;

/// Largest packet WebTrip puts on the wire when the transport cannot say how
/// much it carries. A QUIC DATAGRAM frame must fit one packet (RFC 9221) and
/// QUIC forbids IP fragmentation (RFC 9000 §14), so ~1200 bytes is the
/// fragmentation-free floor; WebRTC data channels use the same figure so a
/// packet never fragments across SCTP chunks (the channel is unordered with
/// no retransmits, so every fragment multiplies loss).
pub const MAX_UNFRAGMENTED_PACKET_BYTES: usize = 1200;

/// Frames in one Web Audio render quantum: the callback size the browser host
/// hands the processor, and the default for [`AudioProcessor::new`](crate::audio::processor::AudioProcessor::new).
/// A native host with another callback size passes its own to
/// [`AudioProcessor::with_network`](crate::audio::processor::AudioProcessor::with_network)
/// and sizes its regulator with [`wire_frames_per_callback`].
pub const RENDER_QUANTUM_FRAMES: usize = 128;

/// The rate network streams run at, and the rate the regulator and every
/// packet header assume (JackTrip code 3). The audio device may run at a
/// different rate; the mixer converts between the two.
pub const WIRE_SAMPLE_RATE: u32 = 48_000;

/// Lowest device sample rate the engine accepts. Browsers allow 8 kHz audio
/// contexts; below that nothing real exists, and the floor bounds
/// how many wire frames one callback can need (see [`wire_frames_per_callback`]),
/// which sizes the regulator's frame FIFO before the context's rate is known.
pub const MIN_DEVICE_SAMPLE_RATE: u32 = 8_000;

/// Highest device sample rate the engine accepts.
pub const MAX_DEVICE_SAMPLE_RATE: u32 = 768_000;

/// Most wire-rate frames one device callback of `callback_frames` frames can
/// consume or produce when converting between `device_rate` and `wire_rate`:
/// the exact ratio rounded up, plus one frame of resampler phase slack when a
/// conversion runs (none does when the rates match). Sizes every wire-side
/// scratch buffer so the render thread never allocates.
pub const fn wire_frames_per_callback(callback_frames: usize, device_rate: u32, wire_rate: u32) -> usize {
    if device_rate == wire_rate {
        return callback_frames;
    }
    (callback_frames * wire_rate as usize).div_ceil(device_rate as usize) + 1
}

/// Wire frames the regulator must be able to serve in one pop to feed one
/// [`RENDER_QUANTUM_FRAMES`] callback at any supported device rate (the slowest
/// device needs the most). The browser host builds its regulator before the
/// device rate is known, so it sizes for this worst case; a host with another
/// callback size uses [`wire_frames_per_callback`] instead.
pub const MAX_WIRE_FRAMES_PER_CALLBACK: usize =
    wire_frames_per_callback(RENDER_QUANTUM_FRAMES, MIN_DEVICE_SAMPLE_RATE, WIRE_SAMPLE_RATE);

#[cfg(test)]
mod tests {
    use super::*;

    /// `stream::regulator` mirrors the wire ceiling by value (it has no
    /// `crate::` imports); if they drift, packets the protocol accepts are
    /// rejected by the regulator, or the reverse.
    #[test]
    fn regulator_mirror_matches_wire_ceiling() {
        assert_eq!(crate::stream::regulator::MAX_CHANNELS, MAX_WIRE_CHANNELS as usize);
    }

    /// Same mirror rule for the fpp range: the regulator rejects what its
    /// preallocated state cannot hold, and the protocol accepts what the
    /// regulator rejects if the two drift.
    #[test]
    fn regulator_mirror_matches_fpp_range() {
        assert_eq!(crate::stream::regulator::MIN_FPP, MIN_FPP);
        assert_eq!(crate::stream::regulator::MAX_FPP, MAX_FPP);
    }
}
