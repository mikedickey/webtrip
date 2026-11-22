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
