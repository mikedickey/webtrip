//! The session's connect/disconnect state machine, and its sibling for audio
//! start/stop ([`AudioLifecycle`]), which has the same epoch discipline.
//!
//! Portable by construction: no browser types, and every transition is a
//! single compare-and-swap on one [`AtomicU64`], so the invariants below hold
//! whichever agent or OS thread drives the session (see AGENTS.md, "Threading
//! Model and Portability"). `WebTripSession` (in `webtrip-web`)
//! owns the resources (transport, audio engine) and asks this machine for
//! permission before acquiring, publishing, or releasing them.
//!
//! ```text
//! Idle          ── begin_connect ─────▶ Connecting
//! Connecting    ── finish_connect(e) ─▶ Connected
//! Connecting    ── abort_connect(e) ──▶ Idle           (connect failed, cleaned up;
//!                                                        bumps the epoch)
//! Connected     ── fail(e) ───────────▶ Error          (transport reported failure)
//! Connecting  ┐
//! Connected   ├─ begin_disconnect ────▶ Disconnecting  (bumps the epoch)
//! Error       ┘
//! Disconnecting ── finish_disconnect ─▶ Idle
//! ```
//!
//! Ending a connect attempt bumps the **epoch**: `begin_disconnect`, and
//! `abort_connect` after a failed connect. A connect remembers the epoch
//! it was admitted under and passes it to every later transition, so a
//! connect that a disconnect has overtaken can never publish its connection,
//! even if a *newer* connect has since moved the state back to `Connecting`
//! (an ABA that a bare state check would miss). The epoch and the state share
//! the one atomic word so that "is this still my connect?" and "make the
//! transition" are one indivisible step rather than a check-then-act.

use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};

/// Where the session is in its connect/disconnect lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SessionState {
    /// No connection and nothing in flight; the only state a connect may start from
    Idle = 0,
    /// Dialing the hub (and, for a combined connect, bringing audio up)
    Connecting = 1,
    /// Transport connected. Audio is tracked separately and may or may not be
    /// running.
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
pub enum ConnectRejected {
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
    pub fn message(self) -> &'static str {
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
pub enum DisconnectStart {
    /// The session is already `Idle`; there is nothing to tear down
    AlreadyIdle,
    /// Another disconnect owns the teardown; wait for it instead of starting one
    AlreadyDisconnecting,
    /// This caller owns the teardown and must call `finish_disconnect` when done
    Started,
}

const STATE_BITS: u32 = 8;
const STATE_MASK: u64 = (1 << STATE_BITS) - 1;

/// A state that can live in an [`EpochWord`]: its zero code is the initial one.
trait Phase: Copy + PartialEq + 'static {
    const ALL: &'static [Self];
    fn code(self) -> u8;
}

impl Phase for SessionState {
    const ALL: &'static [Self] = &ALL_STATES;
    fn code(self) -> u8 {
        self as u8
    }
}

/// An (epoch, state) pair packed in one atomic word, so "is this still my
/// operation?" and "make the transition" are one indivisible step. Shared by
/// the network and audio lifecycles, which differ only in their states.
struct EpochWord<P> {
    word: AtomicU64,
    _phase: PhantomData<fn() -> P>,
}

impl<P> Default for EpochWord<P> {
    fn default() -> Self {
        Self { word: AtomicU64::new(0), _phase: PhantomData }
    }
}

impl<P> std::fmt::Debug for EpochWord<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("EpochWord").field(&self.word).finish()
    }
}

impl<P: Phase> EpochWord<P> {
    fn pack(epoch: u64, phase: P) -> u64 {
        (epoch << STATE_BITS) | u64::from(phase.code())
    }

    fn unpack(word: u64) -> (u64, P) {
        let bits = (word & STATE_MASK) as u8;
        let phase = P::ALL
            .iter()
            .copied()
            .find(|p| p.code() == bits)
            .expect("only `pack` writes the lifecycle word");
        (word >> STATE_BITS, phase)
    }

    fn load(&self) -> (u64, P) {
        Self::unpack(self.word.load(Ordering::Acquire))
    }

    /// Apply `transition` to the current (epoch, state) atomically. Returns the
    /// (epoch, state) it was applied to, or the one that made it decline.
    fn transition(
        &self,
        transition: impl Fn(u64, P) -> Option<(u64, P)>,
    ) -> Result<(u64, P), (u64, P)> {
        self.word
            .try_update(Ordering::AcqRel, Ordering::Acquire, |word| {
                let (epoch, phase) = Self::unpack(word);
                transition(epoch, phase).map(|(e, p)| Self::pack(e, p))
            })
            .map(Self::unpack)
            .map_err(Self::unpack)
    }
}

/// The atomic (epoch, state) pair; see the module docs.
#[derive(Debug, Default)]
pub struct Lifecycle {
    word: EpochWord<SessionState>,
}

impl Lifecycle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> SessionState {
        self.word.load().1
    }

    /// Whether no disconnect has started since `epoch` was handed out.
    pub fn is_current(&self, epoch: u64) -> bool {
        self.word.load().0 == epoch
    }

    fn transition(
        &self,
        transition: impl Fn(u64, SessionState) -> Option<(u64, SessionState)>,
    ) -> Result<(u64, SessionState), (u64, SessionState)> {
        self.word.transition(transition)
    }

    /// `Idle → Connecting`. Returns the epoch the connect must quote to every
    /// later transition.
    pub fn begin_connect(&self) -> Result<u64, ConnectRejected> {
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
    pub fn finish_connect(&self, epoch: u64) -> bool {
        self.connecting_to(epoch, epoch, SessionState::Connected)
    }

    /// `Connecting → Idle` after a failed connect has released what it
    /// created, only for the connect admitted under `epoch`. Bumps the epoch
    /// so a late report from the failed attempt's transport cannot reach the
    /// next connection.
    pub fn abort_connect(&self, epoch: u64) -> bool {
        self.connecting_to(epoch, epoch.wrapping_add(1), SessionState::Idle)
    }

    fn connecting_to(&self, epoch: u64, next_epoch: u64, to: SessionState) -> bool {
        self.transition(|e, state| {
            (e == epoch && state == SessionState::Connecting).then_some((next_epoch, to))
        })
        .is_ok()
    }

    /// `Connected → Error`, only for the connection published under `epoch`,
    /// so a late failure report from an already torn-down transport cannot
    /// fail the connection that replaced it.
    pub fn fail(&self, epoch: u64) -> bool {
        self.transition(|e, state| {
            (e == epoch && state == SessionState::Connected).then_some((e, SessionState::Error))
        })
        .is_ok()
    }

    /// `Connecting | Connected | Error → Disconnecting`, bumping the epoch so
    /// any connect still in flight is cancelled.
    pub fn begin_disconnect(&self) -> DisconnectStart {
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
    pub fn finish_disconnect(&self) {
        let finished = self.transition(|epoch, state| {
            (state == SessionState::Disconnecting).then_some((epoch, SessionState::Idle))
        });
        debug_assert!(finished.is_ok(), "only the teardown owner leaves Disconnecting");
    }
}

/// The session-level network state over its connections' states: the worst
/// news first (`Error`), then whatever is still in motion (`Connecting`,
/// `Disconnecting`), then `Connected`; no connections is `Idle`. One
/// connection passes through unchanged.
pub fn aggregate_network_state(states: &[SessionState]) -> SessionState {
    [
        SessionState::Error,
        SessionState::Connecting,
        SessionState::Disconnecting,
        SessionState::Connected,
    ]
    .into_iter()
    .find(|state| states.contains(state))
    .unwrap_or(SessionState::Idle)
}

/// Whether the audio engine is running, independent of any connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioState {
    Stopped,
    Running,
}

impl AudioState {
    /// The string reported to JavaScript by `getAudioState()`.
    pub fn as_str(self) -> &'static str {
        match self {
            AudioState::Stopped => "stopped",
            AudioState::Running => "running",
        }
    }
}

/// Where audio is between [`AudioState::Stopped`] and [`AudioState::Running`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AudioPhase {
    Stopped = 0,
    /// Bringing the engine up (possibly waiting on a permission prompt)
    Starting = 1,
    Running = 2,
    /// Tearing the engine down; a start issued now waits for this to finish
    Stopping = 3,
}

impl Phase for AudioPhase {
    const ALL: &'static [Self] = &[AudioPhase::Stopped, AudioPhase::Starting, AudioPhase::Running, AudioPhase::Stopping];
    fn code(self) -> u8 {
        self as u8
    }
}

/// Why an audio start was not admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartRejected {
    AlreadyStarting,
    AlreadyRunning,
    /// A stop is in flight; retry once it finishes
    Stopping,
}

impl StartRejected {
    pub fn message(self) -> &'static str {
        match self {
            StartRejected::AlreadyStarting => "Audio is already starting",
            StartRejected::AlreadyRunning => "Audio is already running; call stopAudio() first",
            StartRejected::Stopping => "Audio is stopping",
        }
    }
}

/// What a `begin_stop` call must do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopStart {
    AlreadyStopped,
    AlreadyStopping,
    /// This caller owns the teardown and must call `finish_stop` when done
    Started,
}

/// The audio engine's start/stop state machine: the network [`Lifecycle`]'s
/// sibling, with the same epoch discipline. Stopping (or a failed start) bumps
/// the epoch, so a start that a stop has overtaken can never publish its
/// engine.
///
/// ```text
/// Stopped  ── begin_start ──▶ Starting
/// Starting ── finish_start(e) ▶ Running
/// Starting ── abort_start(e) ─▶ Stopped    (start failed, cleaned up; bumps the epoch)
/// Starting ┐
/// Running  ┴─ begin_stop ────▶ Stopping    (bumps the epoch)
/// Stopping ── finish_stop ───▶ Stopped
/// ```
#[derive(Debug, Default)]
pub struct AudioLifecycle {
    word: EpochWord<AudioPhase>,
}

impl AudioLifecycle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn phase(&self) -> AudioPhase {
        self.word.load().1
    }

    pub fn state(&self) -> AudioState {
        match self.phase() {
            AudioPhase::Running => AudioState::Running,
            _ => AudioState::Stopped,
        }
    }

    /// Whether no stop has started since `epoch` was handed out.
    pub fn is_current(&self, epoch: u64) -> bool {
        self.word.load().0 == epoch
    }

    /// `Stopped → Starting`. Returns the epoch the start must quote to every
    /// later transition.
    pub fn begin_start(&self) -> Result<u64, StartRejected> {
        self.word
            .transition(|epoch, phase| (phase == AudioPhase::Stopped).then_some((epoch, AudioPhase::Starting)))
            .map(|(epoch, _)| epoch)
            .map_err(|(_, phase)| match phase {
                AudioPhase::Starting => StartRejected::AlreadyStarting,
                AudioPhase::Running => StartRejected::AlreadyRunning,
                AudioPhase::Stopping => StartRejected::Stopping,
                AudioPhase::Stopped => unreachable!("Stopped always admits a start"),
            })
    }

    /// `Starting → Running`, only for the start admitted under `epoch`.
    pub fn finish_start(&self, epoch: u64) -> bool {
        self.starting_to(epoch, epoch, AudioPhase::Running)
    }

    /// `Starting → Stopped` after a failed start has released what it created,
    /// only for the start admitted under `epoch`.
    pub fn abort_start(&self, epoch: u64) -> bool {
        self.starting_to(epoch, epoch.wrapping_add(1), AudioPhase::Stopped)
    }

    fn starting_to(&self, epoch: u64, next_epoch: u64, to: AudioPhase) -> bool {
        self.word
            .transition(|e, phase| (e == epoch && phase == AudioPhase::Starting).then_some((next_epoch, to)))
            .is_ok()
    }

    /// `Starting | Running → Stopping`, bumping the epoch so a start still in
    /// flight is cancelled.
    pub fn begin_stop(&self) -> StopStart {
        match self.word.transition(|epoch, phase| match phase {
            AudioPhase::Starting | AudioPhase::Running => Some((epoch.wrapping_add(1), AudioPhase::Stopping)),
            AudioPhase::Stopped | AudioPhase::Stopping => None,
        }) {
            Ok(_) => StopStart::Started,
            Err((_, AudioPhase::Stopping)) => StopStart::AlreadyStopping,
            Err(_) => StopStart::AlreadyStopped,
        }
    }

    /// `Stopping → Stopped`, by the caller that got `StopStart::Started`.
    pub fn finish_stop(&self) {
        let finished = self
            .word
            .transition(|epoch, phase| (phase == AudioPhase::Stopping).then_some((epoch, AudioPhase::Stopped)));
        debug_assert!(finished.is_ok(), "only the teardown owner leaves Stopping");
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
    /// down (a late "closed" from the old transport, whether a disconnect or a
    /// failed connect ended it) must change nothing.
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

        let (lc, failed) = lifecycle_in(SessionState::Connecting);
        assert!(lc.abort_connect(failed));
        let next = lc.begin_connect().unwrap();
        assert_ne!(next, failed, "a failed connect must retire its epoch");
        assert!(lc.finish_connect(next));
        assert!(!lc.fail(failed), "late failure from a failed connect's transport");
        assert_eq!(lc.state(), SessionState::Connected);
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

    #[test]
    fn network_state_aggregates_worst_first() {
        use SessionState::*;
        assert_eq!(aggregate_network_state(&[]), Idle);
        // One connection passes through unchanged.
        for state in ALL_STATES {
            assert_eq!(aggregate_network_state(&[state]), state);
        }
        // Several: failure, then motion, then connected.
        assert_eq!(aggregate_network_state(&[Connected, Error, Connecting]), Error);
        assert_eq!(aggregate_network_state(&[Connected, Connecting, Disconnecting]), Connecting);
        assert_eq!(aggregate_network_state(&[Idle, Connected, Disconnecting]), Disconnecting);
        assert_eq!(aggregate_network_state(&[Idle, Connected]), Connected);
        assert_eq!(aggregate_network_state(&[Idle, Idle]), Idle);
    }

    /// Pins the cross-language contract: `getAudioState()` emits these strings.
    #[test]
    fn audio_state_strings_are_the_js_contract() {
        assert_eq!(AudioState::Stopped.as_str(), "stopped");
        assert_eq!(AudioState::Running.as_str(), "running");
    }

    /// An audio lifecycle driven into `phase`, plus the epoch its start was
    /// admitted under.
    fn audio_in(phase: AudioPhase) -> (AudioLifecycle, u64) {
        let lc = AudioLifecycle::new();
        let epoch = lc.begin_start().unwrap();
        match phase {
            AudioPhase::Stopped => assert!(lc.abort_start(epoch)),
            AudioPhase::Starting => {}
            AudioPhase::Running => assert!(lc.finish_start(epoch)),
            AudioPhase::Stopping => assert_eq!(lc.begin_stop(), StopStart::Started),
        }
        assert_eq!(lc.phase(), phase);
        (lc, epoch)
    }

    #[test]
    fn audio_start_and_stop_are_admitted_by_phase() {
        let starts = [
            (AudioPhase::Stopped, Ok(())),
            (AudioPhase::Starting, Err(StartRejected::AlreadyStarting)),
            (AudioPhase::Running, Err(StartRejected::AlreadyRunning)),
            (AudioPhase::Stopping, Err(StartRejected::Stopping)),
        ];
        for (phase, expected) in starts {
            let (lc, _) = audio_in(phase);
            assert_eq!(lc.begin_start().map(|_| ()), expected, "begin_start from {phase:?}");
        }

        let stops = [
            (AudioPhase::Stopped, StopStart::AlreadyStopped),
            (AudioPhase::Starting, StopStart::Started),
            (AudioPhase::Running, StopStart::Started),
            (AudioPhase::Stopping, StopStart::AlreadyStopping),
        ];
        for (phase, expected) in stops {
            let (lc, epoch) = audio_in(phase);
            let was_current = lc.is_current(epoch);
            assert_eq!(lc.begin_stop(), expected, "begin_stop from {phase:?}");
            if expected == StopStart::Started {
                assert!(!lc.is_current(epoch), "stopping from {phase:?} must cancel the epoch");
                lc.finish_stop();
                assert_eq!(lc.phase(), AudioPhase::Stopped);
            } else {
                assert_eq!(lc.phase(), phase, "{phase:?} must be left alone");
                assert_eq!(lc.is_current(epoch), was_current);
            }
        }
    }

    /// A start overtaken by a stop can neither publish (`finish_start`) nor
    /// reset the state (`abort_start`), even once a newer start is `Starting`.
    #[test]
    fn overtaken_audio_start_cannot_publish_even_after_a_new_start() {
        let lc = AudioLifecycle::new();
        let stale = lc.begin_start().unwrap();
        assert_eq!(lc.begin_stop(), StopStart::Started);
        assert!(!lc.finish_start(stale));
        assert!(!lc.abort_start(stale));
        lc.finish_stop();

        let fresh = lc.begin_start().unwrap();
        assert_ne!(fresh, stale);
        assert!(!lc.finish_start(stale), "stale epoch must not complete the new start");
        assert!(!lc.abort_start(stale), "stale epoch must not abort the new start");
        assert!(lc.finish_start(fresh));
        assert_eq!(lc.state(), AudioState::Running);

        // A failed start retires its epoch too.
        let (lc, failed) = audio_in(AudioPhase::Starting);
        assert!(lc.abort_start(failed));
        assert!(!lc.is_current(failed));
        assert_eq!(lc.state(), AudioState::Stopped);
    }
}
