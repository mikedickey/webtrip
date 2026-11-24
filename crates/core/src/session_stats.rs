//! Session statistics and the pure decision kernels the browser host's loops
//! call. Portable: plain data and arithmetic, no browser API.

use crate::stream::regulator::RegulatorStats;
#[cfg(feature = "wasm")]
use wasm_bindgen::prelude::*;

/// Session statistics (the shape JavaScript reads): the aggregate over every
/// live connection. One connection's statistics, as built by
/// [`build_connection_stats`], have the same fields ([`ConnectionStats`]); the
/// session folds them with [`aggregate_session_stats`].
#[cfg_attr(feature = "wasm", wasm_bindgen)]
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    pub packets_sent: u64,
    pub packets_received: u64,

    // Ring buffer stats (send path)
    pub send_buffer_available: u32,
    pub ring_buffer_writes: u64,
    pub ring_buffer_samples_written: u64,
    pub ring_buffer_overruns: u64,

    // Regulator stats (receive path with Burg PLC)
    /// Current adaptive tolerance in milliseconds
    pub regulator_tolerance_ms: f64,
    /// Current headroom (extra buffering) in milliseconds
    pub regulator_headroom_ms: f64,
    /// Maximum latency observed in this period (ms)
    pub regulator_max_latency_ms: f64,
    /// Current buffer depth (packets buffered)
    pub regulator_depth: u32,
    /// Current latency in milliseconds
    pub regulator_latency_ms: f32,
    /// Whether regulator is initialized (received first packet)
    pub regulator_initialized: bool,
    /// Packets successfully played
    pub regulator_packets_played: u64,
    /// Number of PLC activations (packet loss concealment)
    pub regulator_plc_count: u64,
    /// Last packet sequence number received (u16, wraps at 65535)
    pub regulator_last_seq: u16,
    /// Packets rejected by the regulator (bad channel count, a channel count
    /// that changed mid-stream, a bad or changed fpp, or a size mismatch)
    pub regulator_packets_rejected: u64,
}

/// One connection's statistics: the send ring's counters and the regulator's
/// receive-path numbers, in the same shape as the session's.
pub type ConnectionStats = SessionStats;

#[cfg_attr(feature = "wasm", wasm_bindgen)]
impl SessionStats {
    #[cfg_attr(feature = "wasm", wasm_bindgen(constructor))]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Construct a [`ConnectionStats`] from its raw component values.
///
/// This is the single authoritative mapping point; the browser host's
/// `WebTripSession::get_stats` delegates here (via
/// [`aggregate_session_stats`]) so the mapping can be tested without
/// constructing the full session (which requires a live `AudioEngine` and
/// `Transport`).
pub fn build_connection_stats(
    reg_stats: RegulatorStats,
    regulator_depth: u32,
    regulator_latency_ms: f32,
    regulator_initialized: bool,
    send_buffer_available: u32,
    ring_buffer_writes: u64,
    ring_buffer_samples_written: u64,
    ring_buffer_overruns: u64,
) -> ConnectionStats {
    ConnectionStats {
        packets_sent: 0, // TODO: Get from transport if needed
        packets_received: reg_stats.packets_received,

        send_buffer_available,
        ring_buffer_writes,
        ring_buffer_samples_written,
        ring_buffer_overruns,

        regulator_tolerance_ms: reg_stats.tolerance_ms,
        regulator_headroom_ms: reg_stats.headroom_ms,
        regulator_max_latency_ms: reg_stats.max_latency_ms,
        regulator_depth,
        regulator_latency_ms,
        regulator_initialized,
        regulator_packets_played: reg_stats.packets_played,
        regulator_plc_count: reg_stats.glitches,
        regulator_last_seq: reg_stats.last_seq_received,
        regulator_packets_rejected: reg_stats.packets_rejected,
    }
}

/// Fold every live connection's stats into the session's: counts and depths
/// add; tolerance, headroom and latencies take the worst (largest) connection;
/// the session is initialized once any connection is; `last_seq` is the first
/// connection's (a sequence number means nothing across connections). With no
/// connection every field is zero, and with one the result is that
/// connection's stats unchanged.
pub fn aggregate_session_stats(connections: &[ConnectionStats]) -> SessionStats {
    let mut total = SessionStats::default();
    for c in connections {
        total.packets_sent += c.packets_sent;
        total.packets_received += c.packets_received;
        total.send_buffer_available += c.send_buffer_available;
        total.ring_buffer_writes += c.ring_buffer_writes;
        total.ring_buffer_samples_written += c.ring_buffer_samples_written;
        total.ring_buffer_overruns += c.ring_buffer_overruns;
        total.regulator_tolerance_ms = total.regulator_tolerance_ms.max(c.regulator_tolerance_ms);
        total.regulator_headroom_ms = total.regulator_headroom_ms.max(c.regulator_headroom_ms);
        total.regulator_max_latency_ms = total.regulator_max_latency_ms.max(c.regulator_max_latency_ms);
        total.regulator_depth += c.regulator_depth;
        total.regulator_latency_ms = total.regulator_latency_ms.max(c.regulator_latency_ms);
        total.regulator_initialized |= c.regulator_initialized;
        total.regulator_packets_played += c.regulator_packets_played;
        total.regulator_plc_count += c.regulator_plc_count;
        total.regulator_packets_rejected += c.regulator_packets_rejected;
    }
    total.regulator_last_seq = connections.first().map_or(0, |c| c.regulator_last_seq);
    total
}

/// What the tick loop should do in a single iteration.
#[derive(Debug, PartialEq, Eq)]
pub enum TickDecision {
    /// Not connected: drain the ring buffer so `Atomics.waitAsync` can sleep.
    Drain,
    /// No work to do this iteration: exit the loop.
    Idle,
    /// At least one direction has data — attempt send and/or receive.
    Process {
        /// Ring buffer has enough samples to form one outgoing packet.
        should_send: bool,
        /// Receive queue has at least one incoming packet.
        have_receive: bool,
    },
}

/// Compute what the tick loop should do given the current transport state.
///
/// This is the pure decision kernel of `do_tick`; it takes plain integers and
/// booleans so it can be called from native unit tests without any browser API.
pub fn tick_decision(
    is_connected: bool,
    available: u32,
    samples_needed: u32,
    have_receive: bool,
) -> TickDecision {
    if !is_connected {
        return TickDecision::Drain;
    }
    let should_send = available >= samples_needed;
    if should_send || have_receive {
        TickDecision::Process { should_send, have_receive }
    } else {
        TickDecision::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // ConnectionStats mapping and session aggregation
    // -----------------------------------------------------------------------

    #[test]
    fn build_connection_stats_maps_all_fields() {
        let reg = RegulatorStats {
            tolerance_ms: 12.5,
            headroom_ms: 5.0,
            max_latency_ms: 30.0,
            glitches: 3,
            packets_received: 1000,
            packets_played: 997,
            last_seq_received: 42,
            packets_rejected: 7,
        };

        let s = build_connection_stats(
            reg,
            /* regulator_depth */ 5,
            /* regulator_latency_ms */ 15.0,
            /* regulator_initialized */ true,
            /* send_buffer_available */ 128,
            /* ring_buffer_writes */ 200,
            /* ring_buffer_samples_written */ 25_600,
            /* ring_buffer_overruns */ 0,
        );

        // packets_sent is always 0 (not yet tracked by transport)
        assert_eq!(s.packets_sent, 0);
        assert_eq!(s.packets_received, 1000);

        assert!((s.regulator_tolerance_ms - 12.5).abs() < 1e-9);
        assert!((s.regulator_headroom_ms - 5.0).abs() < 1e-9);
        assert!((s.regulator_max_latency_ms - 30.0).abs() < 1e-9);
        assert_eq!(s.regulator_plc_count, 3);
        assert_eq!(s.regulator_packets_played, 997);
        assert_eq!(s.regulator_last_seq, 42);
        assert_eq!(s.regulator_packets_rejected, 7);
        assert_eq!(s.regulator_depth, 5);
        assert!((s.regulator_latency_ms - 15.0).abs() < 1e-4);
        assert!(s.regulator_initialized);

        assert_eq!(s.send_buffer_available, 128);
        assert_eq!(s.ring_buffer_writes, 200);
        assert_eq!(s.ring_buffer_samples_written, 25_600);
        assert_eq!(s.ring_buffer_overruns, 0);
    }

    /// Zero connections is the all-zero session; one is that connection
    /// unchanged (the only case today); several add counts and take the worst
    /// timing, so a second connection's trouble is never averaged away.
    #[test]
    fn aggregate_session_stats_folds_connections() {
        let a = ConnectionStats {
            packets_received: 10,
            send_buffer_available: 128,
            ring_buffer_overruns: 1,
            regulator_tolerance_ms: 4.0,
            regulator_latency_ms: 20.0,
            regulator_depth: 2,
            regulator_initialized: false,
            regulator_plc_count: 3,
            regulator_last_seq: 7,
            ..Default::default()
        };
        let b = ConnectionStats {
            packets_received: 5,
            send_buffer_available: 64,
            regulator_tolerance_ms: 9.0,
            regulator_latency_ms: 12.0,
            regulator_depth: 1,
            regulator_initialized: true,
            regulator_plc_count: 1,
            regulator_last_seq: 99,
            ..Default::default()
        };

        let none = aggregate_session_stats(&[]);
        assert_eq!((none.packets_received, none.regulator_last_seq, none.regulator_initialized), (0, 0, false));

        let one = aggregate_session_stats(std::slice::from_ref(&a));
        assert_eq!(one.packets_received, 10);
        assert_eq!(one.regulator_last_seq, 7);
        assert_eq!(one.regulator_tolerance_ms, 4.0);
        assert!(!one.regulator_initialized);

        let both = aggregate_session_stats(&[a, b]);
        assert_eq!(both.packets_received, 15);
        assert_eq!(both.send_buffer_available, 192);
        assert_eq!(both.ring_buffer_overruns, 1);
        assert_eq!(both.regulator_depth, 3);
        assert_eq!(both.regulator_plc_count, 4);
        assert_eq!(both.regulator_tolerance_ms, 9.0);
        assert_eq!(both.regulator_latency_ms, 20.0);
        assert!(both.regulator_initialized);
        assert_eq!(both.regulator_last_seq, 7);
    }

    // ── Tick decision logic ───────────────────────────────────────────────────

    #[test]
    fn tick_decision_drains_when_not_connected() {
        // Regardless of how much data is available, not-connected → Drain.
        assert_eq!(tick_decision(false, 128, 128, false), TickDecision::Drain);
        assert_eq!(tick_decision(false, 1024, 128, true),  TickDecision::Drain);
        assert_eq!(tick_decision(false, 0,    128, false), TickDecision::Drain);
    }

    #[test]
    fn tick_decision_process_receive_only_when_queue_has_data() {
        // Not enough samples to send, but receive queue has data → Process (receive only).
        let decision = tick_decision(true, 0, 128, true);
        assert_eq!(
            decision,
            TickDecision::Process { should_send: false, have_receive: true },
        );
    }

    #[test]
    fn tick_decision_send_at_exact_sample_threshold() {
        // available == samples_needed → should_send is true (not strictly greater than).
        assert_eq!(
            tick_decision(true, 128, 128, false),
            TickDecision::Process { should_send: true, have_receive: false },
        );
        // One sample short → should_send is false.
        assert_eq!(
            tick_decision(true, 127, 128, false),
            TickDecision::Idle,
        );
    }
}
