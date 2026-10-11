//! The session's connect/disconnect state machine.
//!
//! Portable by construction: no browser types, and every transition is a
//! single compare-and-swap on one [`AtomicU64`], so the invariants below hold
//! whichever agent or OS thread drives the session (see AGENTS.md, "Threading
//! Model and Portability"). [`WebTripSession`](crate::session::WebTripSession)
//! owns the resources (transport, audio engine) and asks this machine for
//! permission before acquiring, publishing, or releasing them.
//!
//! ```text
//! Idle          ── begin_connect ─────▶ Connecting
//! Connecting    ── finish_connect(e) ─▶ Connected
//! Connecting    ── abort_connect(e) ──▶ Idle           (connect failed, cleaned up)
//! Connected     ── fail(e) ───────────▶ Error          (transport reported failure)
//! Connecting  ┐
//! Connected   ├─ begin_disconnect ────▶ Disconnecting  (bumps the epoch)
//! Error       ┘
//! Disconnecting ── finish_disconnect ─▶ Idle
//! ```
//!
//! Each `begin_disconnect` bumps the **epoch**. A connect remembers the epoch
//! it was admitted under and passes it to every later transition, so a
//! connect that a disconnect has overtaken can never publish its connection,
//! even if a *newer* connect has since moved the state back to `Connecting`
//! (an ABA that a bare state check would miss). The epoch and the state share
//! the one atomic word so that "is this still my connect?" and "make the
//! transition" are one indivisible step rather than a check-then-act.

use std::sync::atomic::{AtomicU64, Ordering};

/// Where the session is in its connect/disconnect lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionState {
    /// No connection and nothing in flight; the only state a connect may start from
    Idle = 0,
    /// Dialing the hub and bringing audio up
    Connecting = 1,
    /// Transport connected and audio running
    Connected = 2,
    /// The transport reported that an established connection failed; its
    /// resources are still held until `disconnect()` releases them
    Error = 3,
    /// Tearing down; a connect issued now waits for this to finish
    Disconnecting = 4,
}

const ALL_STATES: [SessionState; 5] = [
    SessionState::Idle,
    SessionState::Connecting,
    SessionState::Connected,
    SessionState::Error,
    SessionState::Disconnecting,
];

impl SessionState {
    /// The string reported to JavaScript by both `getState()` and the
    /// `set_on_state_change` callback. A cross-language contract: matched by
    /// the `SessionState` union in `website/src/lib/webtrip.ts`.
    pub fn as_str(self) -> &'static str {
        match self {
            SessionState::Idle => "idle",
            SessionState::Connecting => "connecting",
            SessionState::Connected => "connected",
            SessionState::Error => "error",
            SessionState::Disconnecting => "disconnecting",
        }
    }
}

/// Why a connect was not admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectRejected {
    /// Another connect is in flight
    AlreadyConnecting,
    /// A connection is already up
    AlreadyConnected,
    /// The last connection failed and still holds its resources
    InErrorState,
    /// A disconnect is in flight; retry once it finishes
    Disconnecting,
}

impl ConnectRejected {
    pub(crate) fn message(self) -> &'static str {
        match self {
            ConnectRejected::AlreadyConnecting => "A connection is already in progress",
            ConnectRejected::AlreadyConnected => "Already connected; call disconnect() first",
            ConnectRejected::InErrorState => {
                "The previous connection failed; call disconnect() before reconnecting"
            }
            ConnectRejected::Disconnecting => "A disconnect is in progress",
        }
    }
}

/// What a `begin_disconnect` call must do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DisconnectStart {
    /// The session is already `Idle`; there is nothing to tear down
    AlreadyIdle,
    /// Another disconnect owns the teardown; wait for it instead of starting one
    AlreadyDisconnecting,
    /// This caller owns the teardown and must call `finish_disconnect` when done
    Started,
}

const STATE_BITS: u32 = 8;
const STATE_MASK: u64 = (1 << STATE_BITS) - 1;

fn pack(epoch: u64, state: SessionState) -> u64 {
    (epoch << STATE_BITS) | state as u64
}

fn unpack(word: u64) -> (u64, SessionState) {
    let bits = (word & STATE_MASK) as u8;
    let state = ALL_STATES
        .into_iter()
        .find(|s| *s as u8 == bits)
        .expect("only `pack` writes the lifecycle word");
    (word >> STATE_BITS, state)
}

/// The atomic (epoch, state) pair; see the module docs.
#[derive(Debug, Default)]
pub(crate) struct Lifecycle {
    word: AtomicU64,
}

impl Lifecycle {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn state(&self) -> SessionState {
        unpack(self.word.load(Ordering::Acquire)).1
    }

    /// Whether no disconnect has started since `epoch` was handed out.
    pub(crate) fn is_current(&self, epoch: u64) -> bool {
        unpack(self.word.load(Ordering::Acquire)).0 == epoch
    }

    /// Apply `transition` to the current (epoch, state) atomically. Returns the
    /// (epoch, state) it was applied to, or the one that made it decline.
    fn transition(
        &self,
        transition: impl Fn(u64, SessionState) -> Option<(u64, SessionState)>,
    ) -> Result<(u64, SessionState), (u64, SessionState)> {
        self.word
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |word| {
                let (epoch, state) = unpack(word);
                transition(epoch, state).map(|(e, s)| pack(e, s))
            })
            .map(unpack)
            .map_err(unpack)
    }

    /// `Idle → Connecting`. Returns the epoch the connect must quote to every
    /// later transition.
    pub(crate) fn begin_connect(&self) -> Result<u64, ConnectRejected> {
        self.transition(|epoch, state| {
            (state == SessionState::Idle).then_some((epoch, SessionState::Connecting))
        })
        .map(|(epoch, _)| epoch)
        .map_err(|(_, state)| match state {
            SessionState::Connecting => ConnectRejected::AlreadyConnecting,
            SessionState::Connected => ConnectRejected::AlreadyConnected,
            SessionState::Error => ConnectRejected::InErrorState,
            SessionState::Disconnecting => ConnectRejected::Disconnecting,
            SessionState::Idle => unreachable!("Idle always admits a connect"),
        })
    }

    /// `Connecting → Connected`, only for the connect admitted under `epoch`.
    pub(crate) fn finish_connect(&self, epoch: u64) -> bool {
        self.connecting_to(epoch, SessionState::Connected)
    }

    /// `Connecting → Idle` after a failed connect has released what it
    /// created, only for the connect admitted under `epoch`.
    pub(crate) fn abort_connect(&self, epoch: u64) -> bool {
        self.connecting_to(epoch, SessionState::Idle)
    }

    fn connecting_to(&self, epoch: u64, to: SessionState) -> bool {
        self.transition(|e, state| {
            (e == epoch && state == SessionState::Connecting).then_some((e, to))
        })
        .is_ok()
    }

    /// `Connected → Error`, only for the connection published under `epoch`,
    /// so a late failure report from an already torn-down transport cannot
    /// fail the connection that replaced it.
    pub(crate) fn fail(&self, epoch: u64) -> bool {
        self.transition(|e, state| {
            (e == epoch && state == SessionState::Connected).then_some((e, SessionState::Error))
        })
        .is_ok()
    }

    /// `Connecting | Connected | Error → Disconnecting`, bumping the epoch so
    /// any connect still in flight is cancelled.
    pub(crate) fn begin_disconnect(&self) -> DisconnectStart {
        match self.transition(|epoch, state| match state {
            SessionState::Connecting | SessionState::Connected | SessionState::Error => {
                Some((epoch.wrapping_add(1), SessionState::Disconnecting))
            }
            SessionState::Idle | SessionState::Disconnecting => None,
        }) {
            Ok(_) => DisconnectStart::Started,
            Err((_, SessionState::Disconnecting)) => DisconnectStart::AlreadyDisconnecting,
            Err(_) => DisconnectStart::AlreadyIdle,
        }
    }

    /// `Disconnecting → Idle`, by the caller that got `DisconnectStart::Started`.
    pub(crate) fn finish_disconnect(&self) {
        let finished = self.transition(|epoch, state| {
            (state == SessionState::Disconnecting).then_some((epoch, SessionState::Idle))
        });
        debug_assert!(finished.is_ok(), "only the teardown owner leaves Disconnecting");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

    /// A lifecycle driven into `state` through the public transitions, plus the
    /// epoch its most recent connect was admitted under.
    fn lifecycle_in(state: SessionState) -> (Lifecycle, u64) {
        let lc = Lifecycle::new();
        let epoch = lc.begin_connect().unwrap();
        match state {
            SessionState::Idle => assert!(lc.abort_connect(epoch)),
            SessionState::Connecting => {}
            SessionState::Connected => assert!(lc.finish_connect(epoch)),
            SessionState::Error => {
                assert!(lc.finish_connect(epoch));
                assert!(lc.fail(epoch));
            }
            SessionState::Disconnecting => {
                assert_eq!(lc.begin_disconnect(), DisconnectStart::Started)
            }
        }
        assert_eq!(lc.state(), state);
        (lc, epoch)
    }

    /// Pins the cross-language contract: `getState()` and the state-change
    /// callback emit these strings, and `website/src/lib/webtrip.ts` matches them.
    #[test]
    fn state_strings_are_the_js_contract() {
        let strings: Vec<_> = ALL_STATES.iter().map(|s| s.as_str()).collect();
        assert_eq!(strings, ["idle", "connecting", "connected", "error", "disconnecting"]);
    }

    #[test]
    fn connect_is_admitted_only_from_idle() {
        let cases = [
            (SessionState::Idle, Ok(())),
            (SessionState::Connecting, Err(ConnectRejected::AlreadyConnecting)),
            (SessionState::Connected, Err(ConnectRejected::AlreadyConnected)),
            (SessionState::Error, Err(ConnectRejected::InErrorState)),
            (SessionState::Disconnecting, Err(ConnectRejected::Disconnecting)),
        ];
        for (state, expected) in cases {
            let (lc, _) = lifecycle_in(state);
            let result = lc.begin_connect();
            assert_eq!(result.map(|_| ()), expected, "begin_connect from {state:?}");
            let after = if expected.is_ok() { SessionState::Connecting } else { state };
            assert_eq!(lc.state(), after, "state after begin_connect from {state:?}");
        }
    }

    #[test]
    fn disconnect_starts_once_and_cancels_the_epoch() {
        let cases = [
            (SessionState::Idle, DisconnectStart::AlreadyIdle),
            (SessionState::Connecting, DisconnectStart::Started),
            (SessionState::Connected, DisconnectStart::Started),
            (SessionState::Error, DisconnectStart::Started),
            (SessionState::Disconnecting, DisconnectStart::AlreadyDisconnecting),
        ];
        for (state, expected) in cases {
            let (lc, epoch) = lifecycle_in(state);
            let was_current = lc.is_current(epoch);
            assert_eq!(lc.begin_disconnect(), expected, "begin_disconnect from {state:?}");
            if expected == DisconnectStart::Started {
                assert_eq!(lc.state(), SessionState::Disconnecting);
                assert!(!lc.is_current(epoch), "starting a disconnect from {state:?} must cancel the epoch");
                lc.finish_disconnect();
                assert_eq!(lc.state(), SessionState::Idle);
            } else {
                assert_eq!(lc.state(), state, "{state:?} must be left alone");
                assert_eq!(lc.is_current(epoch), was_current, "{state:?} must keep its epoch");
            }
        }
    }

    /// A connect overtaken by a disconnect fails every later checkpoint: the
    /// one before it publishes its transport (`is_current`) and the one that
    /// moves to `Connected` (`finish_connect`). The checks must still fail once
    /// a newer connect is back in `Connecting` (the ABA case).
    #[test]
    fn overtaken_connect_cannot_publish_even_after_a_new_connect() {
        let lc = Lifecycle::new();
        let stale = lc.begin_connect().unwrap();
        assert_eq!(lc.begin_disconnect(), DisconnectStart::Started);

        assert!(!lc.is_current(stale), "cancelled before storing the transport");
        assert!(!lc.finish_connect(stale), "cancelled before reaching Connected");
        assert!(!lc.abort_connect(stale), "a cancelled connect must not reset the state");
        assert_eq!(lc.state(), SessionState::Disconnecting);

        lc.finish_disconnect();
        let fresh = lc.begin_connect().unwrap();
        assert_ne!(fresh, stale);
        assert!(!lc.finish_connect(stale), "stale epoch must not complete the new connect");
        assert!(!lc.abort_connect(stale), "stale epoch must not abort the new connect");
        assert_eq!(lc.state(), SessionState::Connecting);
        assert!(lc.finish_connect(fresh));
        assert_eq!(lc.state(), SessionState::Connected);
    }

    /// Only the live connection may fail: a transport reporting failure while
    /// still dialing (the connect's own `Err` handles that) or after being torn
    /// down (a late "closed" from the old transport) must change nothing.
    #[test]
    fn only_the_current_connection_can_fail() {
        let (lc, epoch) = lifecycle_in(SessionState::Connecting);
        assert!(!lc.fail(epoch), "failure while still connecting");
        assert_eq!(lc.state(), SessionState::Connecting);

        let (lc, old) = lifecycle_in(SessionState::Connected);
        assert_eq!(lc.begin_disconnect(), DisconnectStart::Started);
        lc.finish_disconnect();
        let new = lc.begin_connect().unwrap();
        assert!(lc.finish_connect(new));
        assert!(!lc.fail(old), "late failure from a torn-down transport");
        assert_eq!(lc.state(), SessionState::Connected);

        assert!(lc.fail(new));
        assert_eq!(lc.state(), SessionState::Error);
        assert!(!lc.fail(new), "Error is not re-entered");
    }

    /// Racing threads must each see exactly one winner. A check-then-set built
    /// from a separate load and store lets several threads through at once
    /// (several dials, or several overlapping teardowns).
    #[test]
    fn concurrent_callers_admit_exactly_one_connect_and_one_teardown() {
        const THREADS: usize = 8;
        const ROUNDS: usize = 200;

        fn race<T: Send + 'static>(lc: &Arc<Lifecycle>, op: fn(&Lifecycle) -> T) -> Vec<T> {
            let barrier = Arc::new(Barrier::new(THREADS));
            let handles: Vec<_> = (0..THREADS)
                .map(|_| {
                    let (lc, barrier) = (Arc::clone(lc), Arc::clone(&barrier));
                    std::thread::spawn(move || {
                        barrier.wait();
                        op(&lc)
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        }

        let lc = Arc::new(Lifecycle::new());
        for round in 0..ROUNDS {
            let connects = race(&lc, |lc| lc.begin_connect().is_ok());
            assert_eq!(connects.iter().filter(|ok| **ok).count(), 1, "round {round}: connects admitted");

            let starts = race(&lc, Lifecycle::begin_disconnect);
            let started = starts.iter().filter(|s| **s == DisconnectStart::Started).count();
            assert_eq!(started, 1, "round {round}: teardowns started");
            lc.finish_disconnect();
        }
    }
}
