//! Channel ceilings. Two independent concepts that were once one constant:
//! what the *wire* may carry and what a *device* may expose. Both are rails
//! with a stated reason, never silent ceilings.

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
/// will address. Bounds the worklet ABI's scratch buffers and the
/// `AudioProcessor` device-width buffers, which are preallocated at this width
/// so a wide interface never allocates on the render thread.
pub const MAX_DEVICE_CHANNELS: u8 = 32;

/// Frames in one Web Audio render quantum — the callback size the worklet
/// hands the processor. Sizes every render-thread scratch buffer.
pub const RENDER_QUANTUM_FRAMES: usize = 128;

/// The rate network streams run at, and the rate the regulator and every
/// packet header assume (JackTrip code 3). The audio device may run at a
/// different rate; the mixer converts between the two.
pub const WIRE_SAMPLE_RATE: u32 = 48_000;

/// Lowest device (`AudioContext`) sample rate the engine accepts. Browsers
/// allow 8 kHz contexts; below that nothing real exists, and the floor bounds
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
/// render quantum at any supported device rate (the slowest device needs the
/// most). The regulator is built before the context's rate is known, so it is
/// sized for this worst case.
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
}
