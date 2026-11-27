//! WebTrip Session Manager
//!
//! High-level session management that coordinates all components:
//! - AudioEngine (capture/playback via AudioWorklet)
//! - Transport (abstraction for network layer - WebRTC or WebTransport)
//! - Connection (the transport plus its own RingBuffer, send path: worklet ->
//!   network, and Regulator, receive path: network -> worklet)
//!
//! Audio and network are independent. `startAudio()` brings the engine up with
//! no network (the worklet monitors the input and plays nothing back);
//! `connectNetwork()` dials a hub with no audio (WebRTC is ticked on an interval, since
//! no audio callback exists to wake it). Whichever comes second attaches the
//! engine's worklet node to the connection's stream buffers. `connect()`
//! is both, and owns the audio it started: `disconnect()` stops it again, while
//! `disconnectNetwork()` ends the connection and leaves audio running.
//!
//! ## Connection Flow
//!
//! ```text
//! 1. connect(server_host, port, ...)
//!    └─> Waits out any disconnect still tearing down
//!    └─> Admitted only from Idle (see `webtrip_core::lifecycle`)
//!    └─> Starts audio (unless it is already running), then dials the hub over
//!        the selected transport; WebRTC signaling (WebSocket, SDP offer/answer,
//!        ICE, data channel) happens inside the WebRTC transport's connect
//!
//! 2. Once the transport is connected
//!    └─> Transport published; a running engine's worklet node is rebuilt
//!        against this connection's fresh stream buffers → Connected
//!    └─> Tick clock begins
//!
//! 3. Audio streaming
//!    └─> tick() loop sends/receives packets via Transport trait
//! ```
//!
//! `disconnect()` may be called at any point: it cancels a connect in flight
//! (audio bring-up *and* transport dial), waits for that connect to release what
//! it created, then tears down the published connection. Every network state
//! change is reported through `setOnStateChange` and readable with
//! `getState()`; audio has its own `getAudioState()`.
//!
//! ## Buffer lifetime
//!
//! Every connection allocates its own stream buffers and never reuses them. At
//! teardown they are *retired*, not reset: freed only once the render thread
//! can no longer reach them (see the `connection` module).

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::Poll;

use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::connection::{check_connection_capacity, Connection, ConsumerHold, RetiredList, StreamBuffers};
use crate::js_util::{cancelled_error, named_error};
use crate::audio::engine::{close_audio_context_now, resume_context, route_output_sink, AudioEngine};
use crate::tick_clock::{TickClock, TickSource};
use webtrip_core::audio::params::AudioParams;
use webtrip_core::limits::{MAX_DEVICE_CHANNELS, MAX_WIRE_CHANNELS};
use webtrip_core::stream::regulator::Regulator;
use webtrip_core::session_stats::aggregate_session_stats;
use webtrip_core::stream::ring_buffer::RingBuffer;
use webtrip_core::stream::shared_ptr::SharedPtr;
use webtrip_core::network::protocol::DEFAULT_BUFFER_SIZE;
use webtrip_core::network::transport::{fit_send_fpp, transport_state_is_failure, transport_state_str, AudioBufferConfig, StateCallback, Transport, TransportState, TransportType};
use crate::network::webrtc::{TransportConfig as WebRtcConfig, WebRtcTransport};
use crate::network::webtransport::{is_webtransport_available, WebTransportImpl};
use webtrip_core::lifecycle::{
    aggregate_network_state, AudioLifecycle, AudioPhase, AudioState, ConnectRejected, DisconnectStart, Lifecycle,
    StartRejected, StopStart,
};

pub use webtrip_core::lifecycle::SessionState;
pub use webtrip_core::session_stats::SessionStats;

/// Default signaling port (same as JackTrip TCP port)
pub const DEFAULT_SIGNALING_PORT: u16 = 4464;

/// How long `connect` waits for the transport to connect before giving up and
/// rejecting with a `"TimeoutError"`. It covers only the dial, not audio
/// bring-up (which may wait on a permission prompt).
pub const CONNECT_TIMEOUT_MS: i32 = 45_000;

// ============================================================================
// Pure decision helpers (no browser APIs — fully testable with `cargo test`)
// ============================================================================

/// Returns `true` when `channels` is within `[1, max]`. `max` is
/// `MAX_WIRE_CHANNELS` for the wire settings (send/receive) and
/// `MAX_DEVICE_CHANNELS` for the device settings (input/output).
pub(crate) fn is_valid_channel_count(channels: u8, max: u8) -> bool {
    channels >= 1 && channels <= max
}

// ============================================================================
// Callback-boundary plumbing
// ============================================================================

/// The app's state-change callback, shared with each transport's failure
/// bridge so a callback replaced after connecting still hears about failures.
type StateCallbackSlot = Rc<RefCell<Option<js_sys::Function>>>;

/// Report `state` to the app's callback, if one is set.
fn emit_state(callback: &StateCallbackSlot, state: SessionState) {
    // Clone it out first: the callback may re-enter the session (e.g. replace
    // itself via `set_on_state_change`).
    let callback = callback.borrow().clone();
    if let Some(callback) = callback {
        let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(state.as_str()));
    }
}

fn js_error(message: &str) -> JsValue {
    js_sys::Error::new(message).into()
}

/// Publishes "an operation is in flight" in `slot` for as long as it lives;
/// dropping it clears the slot and wakes everyone awaiting it through
/// [`await_in_flight`].
struct InFlight<'a> {
    slot: &'a RefCell<Option<js_sys::Promise>>,
    resolve: js_sys::Function,
}

impl<'a> InFlight<'a> {
    fn publish(slot: &'a RefCell<Option<js_sys::Promise>>) -> Self {
        let (promise, resolve, _reject) = crate::js_util::make_promise();
        *slot.borrow_mut() = Some(promise);
        Self { slot, resolve }
    }
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.slot.borrow_mut().take();
        let _ = self.resolve.call0(&JsValue::NULL);
    }
}

/// Wait for the operation published in `slot`, if any, to finish.
async fn await_in_flight(slot: &RefCell<Option<js_sys::Promise>>) {
    let pending = slot.borrow().clone();
    if let Some(promise) = pending {
        let _ = JsFuture::from(promise).await;
    }
}

/// Make `abort` resolve with a timeout error after `timeout_ms`, returning the
/// timer handle to clear once it is no longer needed. `abort` is the function
/// that also cancels the operation on request, so whichever comes first wins.
fn arm_timeout(abort: &js_sys::Function, timeout_ms: i32) -> Result<i32, JsValue> {
    let window = web_sys::window().ok_or_else(|| js_error("No window available"))?;
    let timed_out = named_error("TimeoutError", &format!("Connection timed out after {timeout_ms} ms"));
    window.set_timeout_with_callback_and_timeout_and_arguments_1(abort, timeout_ms, &timed_out)
}

/// Await `work` unless `abort` settles first, in which case `work` is dropped
/// and the abort's value becomes the error.
///
/// Used for the transport dial (the caller closes the abandoned transport) and
/// as the outer race for engine bring-up (where `work` awaits a spawned task's
/// "done" signal — see [`WebTripSession::start_engine_unless_aborted`]).
async fn unless_aborted<T>(
    work: impl Future<Output = Result<T, JsValue>>,
    abort: js_sys::Promise,
) -> Result<T, JsValue> {
    let mut work = std::pin::pin!(work);
    let mut abort = JsFuture::from(abort);
    std::future::poll_fn(|cx| {
        if let Poll::Ready(result) = work.as_mut().poll(cx) {
            return Poll::Ready(result);
        }
        match Pin::new(&mut abort).poll(cx) {
            Poll::Ready(Ok(reason) | Err(reason)) => Poll::Ready(Err(reason)),
            Poll::Pending => Poll::Pending,
        }
    })
    .await
}

/// Construct an unconnected transport of `transport_type`.
fn new_transport(transport_type: TransportType) -> Result<Box<dyn Transport>, JsValue> {
    match transport_type {
        TransportType::WebRTC => Ok(Box::new(WebRtcTransport::new(Some(WebRtcConfig::low_latency()))?)),
        TransportType::WebTransport => {
            if !is_webtransport_available() {
                return Err(js_error("WebTransport is not supported in this browser; use WebRTC instead."));
            }
            Ok(Box::new(WebTransportImpl::new()?))
        }
    }
}

/// Audio capture options for starting the engine.
struct CaptureParams {
    device_id: Option<String>,
    auto_gain_control: bool,
    echo_cancellation: bool,
    noise_suppression: bool,
}

/// The engine worklet's `(send ring, receive regulator)` pointers.
type NetworkBuffers = (*mut RingBuffer, *mut Regulator);

/// `buffers`, or null pointers (a worklet node that reads no connection).
fn or_detached(buffers: Option<NetworkBuffers>) -> NetworkBuffers {
    buffers.unwrap_or((std::ptr::null_mut(), std::ptr::null_mut()))
}

/// A connection's stream buffers as the engine's worklet is pointed at them.
#[derive(Clone, Copy)]
struct Published {
    id: u64,
    ring: *mut RingBuffer,
    regulator: *mut Regulator,
}

/// Create an `AudioEngine` and start capture, honouring `cancelled` after each
/// await so an aborted start never installs a worklet on shared buffers.
/// `network` is the connection to attach the first worklet node to, or none.
///
/// `capture_gate` (tests) holds bring-up after construction to simulate a
/// stuck permission prompt; `capture_waiting` is set while that gate is held.
async fn bring_up_engine(
    params_addr: usize,
    network: Option<NetworkBuffers>,
    capture: CaptureParams,
    output_device_id: Option<String>,
    input_channels: u32,
    output_channels: u32,
    cancelled: Arc<AtomicBool>,
    context_slot: Rc<RefCell<Option<web_sys::AudioContext>>>,
    capture_gate: Option<js_sys::Promise>,
    capture_waiting: Option<Rc<Cell<bool>>>,
) -> Result<AudioEngine, JsValue> {
    // `AudioEngine` is a `#[wasm_bindgen]` boundary type, so it takes raw
    // pointers / addresses here and wraps them in `SharedPtr` internally
    // when it builds the `AudioProcessor`.
    let (local_to_network, network_to_local) = or_detached(network);
    let mut engine =
        AudioEngine::create_with_network(params_addr, local_to_network, network_to_local).await?;
    // Lets an abort close the context while `getUserMedia` is still pending.
    *context_slot.borrow_mut() = Some(engine.context());
    if cancelled.load(Ordering::Acquire) {
        engine.stop_capture().await;
        return Err(cancelled_error());
    }

    if let Some(gate) = capture_gate {
        if let Some(flag) = &capture_waiting {
            flag.set(true);
        }
        let _ = JsFuture::from(gate).await;
        if cancelled.load(Ordering::Acquire) {
            engine.stop_capture().await;
            return Err(cancelled_error());
        }
    }

    web_sys::console::log_1(&"🎙️ Starting audio capture".into());
    let started = engine
        .start_capture_cancellable(
            capture.device_id,
            output_device_id,
            capture.auto_gain_control,
            capture.echo_cancellation,
            capture.noise_suppression,
            input_channels,
            output_channels,
            Some(&*cancelled),
        )
        .await;
    if let Err(e) = started {
        engine.stop_capture().await;
        return Err(e);
    }
    // No `cancelled` re-check: `start_capture_cancellable` checks after its
    // last await and everything after that (worklet build, spawned resume) is
    // synchronous, so the flag cannot flip before this returns.
    Ok(engine)
}

/// What detaching the engine's worklet node from a connection's buffers leaves
/// to wait for.
enum Detached {
    /// The engine was not attached to them
    NotAttached,
    /// Attached; the replaced node's `"stopped"` proves it let go
    Pending(js_sys::Promise),
    /// The node could not be replaced, so it may still be reading them
    Failed,
    /// The engine is starting or stopping, so a node holding them may be
    /// running that this cannot replace; only that engine's own successful
    /// close proves it gone
    InTransit,
}

/// A WebTrip session: the audio engine plus at most one hub connection.
///
/// A session runs two independent state machines:
///
/// - **Audio** (`getAudioState()`): `"stopped"` or `"running"`. `startAudio()`
///   captures from the input device and plays to the output device;
///   `stopAudio()` ends it. No callback fires on audio changes.
/// - **Network** (`getState()`, reported to `setOnStateChange`): `"idle"`,
///   `"connecting"`, `"connected"`, `"error"` or `"disconnecting"`.
///   `connectNetwork()` dials a hub; `disconnectNetwork()` hangs up.
///
/// `connect()` and `disconnect()` do both: `connect()` starts audio if it is
/// not already running, and `disconnect()` stops only the audio that
/// `connect()` started. Either half may come first; whichever comes second
/// attaches the running audio to the connection.
///
/// | Call | Network `"idle"` | `"connecting"` | `"connected"` | `"error"` | `"disconnecting"` |
/// |------|------------------|----------------|---------------|-----------|-------------------|
/// | `connect` / `connectNetwork` | dials | rejects | rejects | rejects (call `disconnect()` first) | waits, then dials |
/// | `disconnect` / `disconnectNetwork` | resolves at once | cancels the dial, then tears down | tears down | tears down | waits for that teardown |
///
/// | Call | Audio stopped | starting | running | stopping |
/// |------|---------------|----------|---------|----------|
/// | `startAudio` | starts | rejects | rejects | waits, then starts |
/// | `stopAudio` | resolves at once | cancels the start | stops | waits for that stop |
///
/// Any method may be called while another call's promise is pending (for
/// example `disconnect()` during a `connect()` stuck on a permission prompt).
/// All methods must be called from the agent (page or worker) that created the
/// session.
///
/// To discard a session, await `disconnect()` (and `stopAudio()` if you started
/// audio yourself), then call `free()`. `free()` throws while any of the
/// session's promises is pending. A session freed while connected still tears
/// its connection down in the background.
// The lifecycle invariants (connect only from `Idle`, a disconnect cancels a
// connect in flight, one teardown at a time; likewise for audio start/stop)
// live in the atomic `Lifecycle` and `AudioLifecycle`. Every method takes
// `&self` because wasm-bindgen holds an async method's borrow until its
// promise settles, so an `&mut self` method would throw while any promise is
// pending. The remaining interior mutability (`Cell`/`RefCell`/`Rc`) holds
// browser objects and the JS callbacks wired to them, confined to the agent
// that created the session: it holds `js_sys` values, so it is neither `Send`
// nor `Sync`, and its callbacks run on that agent's event loop. No `RefCell`
// borrow is held across an `.await`.
#[wasm_bindgen]
pub struct WebTripSession {
    // Audio components
    audio_params: SharedPtr<AudioParams>,
    audio_engine: RefCell<Option<AudioEngine>>,
    audio_lifecycle: Arc<AudioLifecycle>,
    /// Which engine is current; tags the buffers retired against its worklet
    /// node, so its successful close frees exactly those
    engine_generation: Cell<u64>,
    /// The connection whose buffers the engine's worklet node reads, or may
    /// still read: set when an engine start begins (the node is handed the
    /// pointers before the engine is published) and cleared only once the
    /// engine's close has settled (`stopAudio` unpublishes the engine well
    /// before its worklet is proven gone). With no engine published it means
    /// the engine is in transit, which [`Detached::InTransit`] reports.
    engine_attached: Cell<Option<u64>>,
    /// Settles when the audio start in flight has finished (successfully or
    /// after releasing everything it created)
    audio_start_in_flight: RefCell<Option<js_sys::Promise>>,
    /// Aborts the audio start in flight
    abort_audio_in_flight: RefCell<Option<js_sys::Function>>,
    /// Settles when the audio stop in flight has returned audio to `Stopped`
    audio_teardown_in_flight: RefCell<Option<js_sys::Promise>>,
    /// Whether the audio running was started by `connect`, and so
    /// stops with its connection
    audio_owned_by_connect: Cell<bool>,
    /// Set by a `disconnect()` that found another disconnect's teardown in
    /// flight: the teardown's winner then stops connect-owned audio on its
    /// behalf, whichever disconnect claimed the teardown
    disconnect_stops_audio: Cell<bool>,

    // The live connections, each owning its transport and stream buffers
    // (shared with the AudioWorklet and the transport via pointers). At most
    // `MAX_CONNECTIONS`.
    connections: RefCell<Vec<Connection>>,
    next_connection_id: Cell<u64>,
    /// Buffers of connections that have ended, kept alive until nothing can
    /// still reach them. Shared with the tasks awaiting a worklet node's
    /// `"stopped"` acknowledgement (see [`Self::retire_until_stopped`]).
    retired: Rc<RefCell<RetiredList<StreamBuffers>>>,
    transport_type: Cell<TransportType>,
    /// Ticks the published transport: from the audio callback while audio runs,
    /// on an interval otherwise
    tick_clock: RefCell<TickClock>,

    lifecycle: Arc<Lifecycle>,
    /// Settles when the connect in flight has finished (successfully or after
    /// releasing everything it created)
    connect_in_flight: RefCell<Option<js_sys::Promise>>,
    /// Aborts the in-flight connect's audio bring-up and/or transport dial
    abort_in_flight: RefCell<Option<js_sys::Function>>,
    /// Settles when the disconnect in flight has returned the session to `Idle`
    teardown_in_flight: RefCell<Option<js_sys::Promise>>,
    /// Test-only: when set, engine bring-up awaits this promise after creating
    /// the `AudioEngine` and before `start_capture` (simulates a stuck
    /// `getUserMedia` / permission prompt).
    #[cfg(all(test, target_arch = "wasm32"))]
    capture_gate: RefCell<Option<js_sys::Promise>>,
    /// Test-only: set when bring-up is blocked on [`capture_gate`].
    #[cfg(all(test, target_arch = "wasm32"))]
    capture_waiting: Rc<Cell<bool>>,

    // Configuration. The wire counts (send/receive) are set only while the
    // network is Idle, the device counts (input/output) only while audio is
    // Stopped; each is fixed for the life of what it configures.
    /// Frames per packet this client prefers to send. A transport with a
    /// packet-size limit sends fewer (`fit_send_fpp`); the peer is told by the
    /// packet header, so nothing else needs to agree.
    send_fpp: usize,
    /// Frames per packet each connection's regulator starts at, before it
    /// adopts the peer's from its first packet
    receive_fpp: usize,
    /// Channels sent over the wire (payload width of every outbound packet)
    send_channels: Cell<u8>,
    /// Channels asked back from the peer (advertised in every outbound header)
    receive_channels: Cell<u8>,
    /// Input device channels to capture (the first N, clamped to the device)
    input_channels: Cell<u8>,
    /// Output device channels to play to (the first N, clamped to the device)
    output_channels: Cell<u8>,

    // Callbacks
    on_state_change: StateCallbackSlot,

    // Selected output device ID (stored until audio engine is ready)
    output_device_id: RefCell<Option<String>>,
}

#[wasm_bindgen]
impl WebTripSession {
    /// Create a session.
    ///
    /// `audio_params_addr` must be a value returned by `createAudioParams()`
    /// (in Rust, `webtrip_core::audio::params::create_audio_params`). Those
    /// parameters are never freed, so create them once per page; sessions may
    /// share them. Any other value is undefined behavior: the session and its
    /// audio thread dereference the address.
    #[wasm_bindgen(constructor)]
    pub fn new(audio_params_addr: usize) -> Result<WebTripSession, JsValue> {
        // SAFETY: `audio_params_addr` is a wasm-bindgen boundary value from JS.
        // Callers pass the address from [`create_audio_params`] (a leaked
        // `AudioParams` that outlives the session and every agent holding it).
        let audio_params = unsafe { SharedPtr::<AudioParams>::from_addr(audio_params_addr) };
        let send_fpp = DEFAULT_BUFFER_SIZE as usize;
        // Only each connection's regulator geometry until the peer's first
        // packet, whose frames per packet it adopts.
        let receive_fpp = DEFAULT_BUFFER_SIZE as usize;
        let send_channels = 1;
        let receive_channels = 2;

        Ok(WebTripSession {
            audio_params,
            audio_engine: RefCell::new(None),
            audio_lifecycle: Arc::new(AudioLifecycle::new()),
            engine_generation: Cell::new(0),
            engine_attached: Cell::new(None),
            audio_start_in_flight: RefCell::new(None),
            abort_audio_in_flight: RefCell::new(None),
            audio_teardown_in_flight: RefCell::new(None),
            audio_owned_by_connect: Cell::new(false),
            disconnect_stops_audio: Cell::new(false),
            connections: RefCell::new(Vec::new()),
            next_connection_id: Cell::new(0),
            retired: Rc::new(RefCell::new(RetiredList::new())),
            transport_type: Cell::new(TransportType::WebRTC), // Default to WebRTC
            tick_clock: RefCell::new(TickClock::default()),
            lifecycle: Arc::new(Lifecycle::new()),
            connect_in_flight: RefCell::new(None),
            abort_in_flight: RefCell::new(None),
            teardown_in_flight: RefCell::new(None),
            #[cfg(all(test, target_arch = "wasm32"))]
            capture_gate: RefCell::new(None),
            #[cfg(all(test, target_arch = "wasm32"))]
            capture_waiting: Rc::new(Cell::new(false)),
            send_fpp,
            receive_fpp,
            send_channels: Cell::new(send_channels),
            receive_channels: Cell::new(receive_channels),
            input_channels: Cell::new(2),
            output_channels: Cell::new(2),
            on_state_change: Rc::new(RefCell::new(None)),
            output_device_id: RefCell::new(None),
        })
    }

    /// Set the callback for network state changes, replacing any earlier one.
    /// It receives the same strings as `getState()`:
    ///
    /// - a connect: `"connecting"`, then `"connected"`, or `"idle"` if it fails
    ///   or is cancelled
    /// - a connection that fails after connecting: `"error"` (audio keeps
    ///   running; call `disconnect()` to clean up)
    /// - a disconnect: `"disconnecting"`, then `"idle"`
    ///
    /// It is not called with the current state when set, nor for a connect
    /// rejected before it starts (for example while already connected), nor
    /// for audio changes. It is called synchronously from inside the session
    /// and may call back into it.
    #[wasm_bindgen(js_name = setOnStateChange)]
    pub fn set_on_state_change(&self, callback: js_sys::Function) {
        *self.on_state_change.borrow_mut() = Some(callback);
    }

    /// The current network state: `"idle"`, `"connecting"`, `"connected"`,
    /// `"error"`, or `"disconnecting"`.
    #[wasm_bindgen(js_name = getState)]
    pub fn get_state(&self) -> String {
        self.state().as_str().to_string()
    }

    /// Whether audio is running, independent of any connection: `"stopped"` or
    /// `"running"`.
    ///
    /// A start or stop still in progress reports `"stopped"`; `startAudio()`
    /// called during a start rejects with "Audio is already starting".
    #[wasm_bindgen(js_name = getAudioState)]
    pub fn get_audio_state(&self) -> String {
        self.audio_lifecycle.state().as_str().to_string()
    }

    /// Current statistics, summed over the live connection (all zero when there
    /// is none).
    #[wasm_bindgen(js_name = getStats)]
    pub fn get_stats(&self) -> SessionStats {
        let stats: Vec<_> = self.connections.borrow().iter().map(|c| c.buffers.stats()).collect();
        aggregate_session_stats(&stats)
    }

    /// Set how many channels this client sends over the wire (default 1, at
    /// most 8).
    ///
    /// Channel counts are fixed for the life of what they configure: the
    /// send/receive counts can change only while the network is `"idle"`, the
    /// input/output counts only while audio is stopped. A call at any other
    /// time, or with a count out of range, is ignored with a console warning;
    /// read the getter back to check.
    ///
    /// This is each outbound packet's payload width, independent of the
    /// input device: captured audio is mixed to it (for example 2 input
    /// channels averaged into 1 sent).
    #[wasm_bindgen(js_name = setSendChannels)]
    pub fn set_send_channels(&self, channels: u8) {
        if self.can_set_channel_count("send", channels, MAX_WIRE_CHANNELS, self.network_is_idle()) {
            self.send_channels.set(channels);
        }
    }

    /// Get the configured send channel count
    #[wasm_bindgen(js_name = getSendChannels)]
    pub fn get_send_channels(&self) -> u8 {
        self.send_channels.get()
    }

    /// Set how many channels this client asks the peer to send back (default
    /// 2, at most 8), advertised in every outbound packet's header. Only while
    /// the network is `"idle"`; see `setSendChannels` for when a call is
    /// ignored.
    ///
    /// The peer decides what it actually sends: whatever channel count its
    /// packets carry is mapped onto the output device.
    #[wasm_bindgen(js_name = setReceiveChannels)]
    pub fn set_receive_channels(&self, channels: u8) {
        if self.can_set_channel_count("receive", channels, MAX_WIRE_CHANNELS, self.network_is_idle()) {
            self.receive_channels.set(channels);
        }
    }

    /// Get the configured receive channel count
    #[wasm_bindgen(js_name = getReceiveChannels)]
    pub fn get_receive_channels(&self) -> u8 {
        self.receive_channels.get()
    }

    /// Set how many input device channels to capture (default 2, at most 32):
    /// the device's first N, clamped at capture time to what the browser
    /// grants. Only while audio is stopped; see `setSendChannels` for when a
    /// call is ignored.
    #[wasm_bindgen(js_name = setInputChannels)]
    pub fn set_input_channels(&self, channels: u8) {
        if self.can_set_channel_count("input", channels, MAX_DEVICE_CHANNELS, self.audio_is_stopped()) {
            self.input_channels.set(channels);
        }
    }

    /// Get the configured input device channel count
    #[wasm_bindgen(js_name = getInputChannels)]
    pub fn get_input_channels(&self) -> u8 {
        self.input_channels.get()
    }

    /// Set how many output device channels to play to (default 2, at most 32):
    /// the device's first N, clamped at capture time to the destination's
    /// `maxChannelCount`. Only while audio is stopped; see `setSendChannels`
    /// for when a call is ignored.
    #[wasm_bindgen(js_name = setOutputChannels)]
    pub fn set_output_channels(&self, channels: u8) {
        if self.can_set_channel_count("output", channels, MAX_DEVICE_CHANNELS, self.audio_is_stopped()) {
            self.output_channels.set(channels);
        }
    }

    /// Get the configured output device channel count
    #[wasm_bindgen(js_name = getOutputChannels)]
    pub fn get_output_channels(&self) -> u8 {
        self.output_channels.get()
    }

    /// Choose the transport for the next connection (default WebRTC). Only
    /// while the network is `"idle"`; otherwise the call is ignored with a
    /// console warning. The session never falls back on its own: with
    /// WebTransport selected in a browser without it, `connect()` rejects.
    #[wasm_bindgen(js_name = setTransportType)]
    pub fn set_transport_type(&self, transport_type: TransportType) {
        if self.state() == SessionState::Idle {
            self.transport_type.set(transport_type);
            web_sys::console::log_1(&format!("🚀 Transport type set to: {}", transport_type.name()).into());
        } else {
            web_sys::console::warn_1(&"⚠️ Cannot change transport type while connected".into());
        }
    }

    /// Get the current transport type
    #[wasm_bindgen(js_name = getTransportType)]
    pub fn get_transport_type(&self) -> TransportType {
        self.transport_type.get()
    }

    /// Whether this browser has the `WebTransport` API. See
    /// [browser support](https://webtrip.dev/docs/browser-support) for versions.
    #[wasm_bindgen(js_name = isWebTransportAvailable)]
    pub fn is_webtransport_available_check() -> bool {
        is_webtransport_available()
    }

    /// Set the output device for playback: a `deviceId` from
    /// `getAudioDevices()`, or `""`, `undefined` or `null` for the default.
    ///
    /// While audio is stopped this stores the device for the next start; while
    /// it runs it also routes the live audio, whatever the network is doing.
    /// Rejects while an audio start is in progress. The device is stored even
    /// if routing then fails. In a browser without `AudioContext.setSinkId`
    /// audio stays on the default output, with a console warning.
    // Rejected mid-start because the engine is not published yet, and a
    // store-only update would be lost or force a deferred re-apply that races
    // stop (WEB-72).
    #[wasm_bindgen(js_name = setOutputDevice)]
    pub async fn set_output_device(&self, device_id: Option<String>) -> Result<(), JsValue> {
        if self.audio_lifecycle.phase() == AudioPhase::Starting {
            return Err(js_error("Cannot change output device while audio is starting"));
        }
        *self.output_device_id.borrow_mut() = device_id.clone();

        // If audio engine exists, apply immediately
        let ctx = self.audio_engine.borrow().as_ref().map(AudioEngine::context);
        if let Some(ctx) = ctx {
            route_output_sink(ctx.as_ref(), device_id).await?;
            // The engine may have been torn down while the sink was routing.
            if let Some(engine) = self.audio_engine.borrow_mut().as_mut() {
                engine.reconfigure_for_sink()?;
            }
        }
        Ok(())
    }

    // ========== Audio ==========

    /// Start the audio engine: capture from the input device and play to the
    /// output device, independent of any network connection. With no
    /// connection the engine monitors the input and plays nothing back; a
    /// connection made now or later is attached to it.
    ///
    /// Arguments: an input `deviceId` from `getAudioDevices()` (`undefined`
    /// for the default), then whether to enable auto gain control, echo
    /// cancellation and noise suppression.
    ///
    /// This creates the `AudioContext` and may show the microphone permission
    /// prompt. Call it from a user gesture (a click handler): otherwise the
    /// browser may start the context suspended (see `isAudioSuspended()`).
    ///
    /// Rejects unless audio is stopped ("Audio is already starting", "Audio is
    /// already running; call stopAudio() first"); a stop still in progress is
    /// waited out. Errors from `getUserMedia` (for example a denied permission)
    /// reject it too. A `stopAudio()` while this is pending (for instance on an
    /// unanswered permission prompt) makes it release whatever it has created
    /// and reject with an `Error` named `"AbortError"`.
    #[wasm_bindgen(js_name = startAudio)]
    pub async fn start_audio(
        &self,
        device_id: Option<String>,
        auto_gain_control: bool,
        echo_cancellation: bool,
        noise_suppression: bool,
    ) -> Result<(), JsValue> {
        let (abort, abort_fn, _reject) = crate::js_util::make_promise();
        let capture = CaptureParams { device_id, auto_gain_control, echo_cancellation, noise_suppression };
        self.start_audio_with(capture, abort, abort_fn).await
    }

    /// Stop the audio engine, resolving once audio is `"stopped"`. Any
    /// connection stays up (its transport keeps running, with nothing to send
    /// and nothing playing what it receives). Safe to call at any time and any
    /// number of times; it also cancels a start in progress. Audio started
    /// after this is yours: a later `disconnect()` does not stop it, even if
    /// `connect()` had started the audio this call stopped.
    #[wasm_bindgen(js_name = stopAudio)]
    pub async fn stop_audio(&self) {
        self.audio_owned_by_connect.set(false);
        self.stop_audio_inner().await;
    }

    // ========== Network ==========

    /// Connect to a JackTrip hub server over the network only: audio is
    /// neither started nor required. Running audio is attached to the
    /// connection; audio started later attaches itself. Rejections, timeout
    /// and cancellation are those of `connect()`.
    ///
    /// Arguments: the hub host name (a studio's `serverHost`), the signaling
    /// port (`undefined` for 4464), and an optional client name shown to other
    /// participants.
    #[wasm_bindgen(js_name = connectNetwork)]
    pub async fn connect_network(
        &self,
        server_host: String,
        port: Option<u16>,
        client_name: Option<String>,
    ) -> Result<(), JsValue> {
        let transport_type = self.transport_type.get();
        self.connect_with(
            || new_transport(transport_type),
            &server_host,
            port.unwrap_or(DEFAULT_SIGNALING_PORT),
            client_name.as_deref().unwrap_or(""),
            None,
            CONNECT_TIMEOUT_MS,
        )
        .await
    }

    /// Start audio and connect to a JackTrip hub server: the main entry point
    /// for joining a studio.
    ///
    /// Arguments: the hub host name (a studio's `serverHost`), the signaling
    /// port (`undefined` for 4464), then the `startAudio()` arguments (input
    /// `deviceId`, auto gain control, echo cancellation, noise suppression),
    /// then an optional client name shown to other participants.
    ///
    /// Steps:
    /// 1. Waits for any disconnect still in progress, then rejects unless the
    ///    network is `"idle"`.
    /// 2. Starts audio, unless it is already running (then the capture
    ///    arguments are ignored and `disconnect()` leaves audio running). A
    ///    `startAudio()` still in progress is waited for rather than raced.
    /// 3. Dials the selected transport (for WebRTC: WebSocket signaling, SDP
    ///    offer/answer, ICE, data channel), giving up after 45 seconds. The
    ///    timeout covers only this dial, not a permission prompt in step 2.
    ///
    /// Audio this call started is stopped again by `disconnect()` and by a
    /// failed connect. Call it from a user gesture, as for `startAudio()`.
    ///
    /// Rejects with:
    /// - "A connection is already in progress", "Already connected; call
    ///   disconnect() first", or "The previous connection failed; call
    ///   disconnect() before reconnecting" when the network is not `"idle"`
    /// - any `startAudio()` error, such as a denied microphone permission
    /// - "WebTransport is not supported in this browser" when WebTransport is
    ///   selected but unavailable
    /// - an error saying the packet size does not fit the transport
    /// - "The connection failed while it was being set up", or the
    ///   transport's own error, when the dial fails
    /// - an `Error` named `"TimeoutError"` when the dial times out
    /// - an `Error` named `"AbortError"` when `disconnect()` is called while
    ///   this is pending; it never reaches `"connected"` afterwards
    ///
    /// A connect that fails after it was admitted leaves the network
    /// `"idle"`. A connection that fails later moves to `"error"`, with audio
    /// still running, until you call `disconnect()`.
    #[wasm_bindgen(js_name = connect)]
    pub async fn connect(
        &self,
        server_host: String,
        port: Option<u16>,
        device_id: Option<String>,
        auto_gain_control: bool,
        echo_cancellation: bool,
        noise_suppression: bool,
        client_name: Option<String>,
    ) -> Result<(), JsValue> {
        let transport_type = self.transport_type.get();
        self.connect_with(
            || new_transport(transport_type),
            &server_host,
            port.unwrap_or(DEFAULT_SIGNALING_PORT),
            client_name.as_deref().unwrap_or(""),
            Some(CaptureParams {
                device_id,
                auto_gain_control,
                echo_cancellation,
                noise_suppression,
            }),
            CONNECT_TIMEOUT_MS,
        )
        .await
    }

    /// Disconnect from the hub server, resolving once the network is
    /// `"idle"`, and stop the audio `connect()` started. Audio you started
    /// yourself with `startAudio()` keeps running, and so does audio left by
    /// `disconnectNetwork()`.
    ///
    /// Never rejects. Safe to call at any time and any number of times: when
    /// `"idle"` it resolves at once, and a call while another disconnect is
    /// running waits for that one. A connect in progress is cancelled (it
    /// rejects with `"AbortError"`) and its cleanup is awaited first. A
    /// connect issued meanwhile waits for this to finish.
    // Teardown awaits full transport shutdown before retiring the connection's
    // buffers. That ordering is load-bearing: the `Regulator` is shared via a
    // raw pointer with the transport (for WebTransport, the worker thread
    // writes to it), so freeing it before the transport has quiesced would let
    // a late `regulator.push()` write freed memory. Awaiting
    // `transport.close()` establishes the happens-before ordering: for
    // WebTransport the future resolves after the worker posts `"disconnected"`
    // (both send and receive loops have exited); for WebRTC the teardown is
    // fully synchronous. The 2s WebTransport force-terminate fallback is a
    // residual race — see `WebTransportImpl::close`. The buffers are then
    // freed only once the audio side is quiescent too (see the `connection`
    // module).
    pub async fn disconnect(&self) {
        self.disconnect_with(true).await;
    }

    /// Disconnect from the hub server over the network only, with the same
    /// guarantees as `disconnect()`, but audio is left running even when
    /// `connect()` started it. That audio is then yours to stop with
    /// `stopAudio()`: a later `disconnect()` does not touch it.
    #[wasm_bindgen(js_name = disconnectNetwork)]
    pub async fn disconnect_network(&self) {
        self.disconnect_with(false).await;
    }
}

impl WebTripSession {
    /// The body of `disconnect` and `disconnectNetwork`: `stop_owned_audio`
    /// says whether audio that `connect()` started goes with the connection.
    async fn disconnect_with(&self, stop_owned_audio: bool) {
        match self.lifecycle.begin_disconnect() {
            DisconnectStart::AlreadyIdle => return,
            DisconnectStart::AlreadyDisconnecting => {
                // The winner may have been asked to leave audio running; this
                // call was asked to stop it, and the winner has not decided yet
                // (it decides after its network teardown, with no await after).
                if stop_owned_audio {
                    self.disconnect_stops_audio.set(true);
                }
                await_in_flight(&self.teardown_in_flight).await;
                return;
            }
            DisconnectStart::Started => {}
        }
        // Published before anything can re-enter (the state callback below),
        // so a connect issued from it finds the teardown to wait on.
        let teardown = InFlight::publish(&self.teardown_in_flight);
        emit_state(&self.on_state_change, SessionState::Disconnecting);

        // Abort a dial or audio bring-up still in progress, then let that
        // connect release what it created (or detach engine cleanup when
        // capture cannot be aborted — WEB-69).
        let abort = self.abort_in_flight.borrow_mut().take();
        if let Some(abort) = abort {
            let _ = abort.call1(&JsValue::NULL, &cancelled_error());
        }
        await_in_flight(&self.connect_in_flight).await;

        self.stop_network_loop();
        let (connection, transport) = self.take_live_transport();
        if let Some(connection) = connection {
            self.end_connection(connection, transport).await;
        }
        let stop_audio = self.disconnect_stops_audio.replace(false) || stop_owned_audio;
        if self.audio_owned_by_connect.replace(false) && stop_audio {
            self.stop_audio_inner().await;
        }

        self.lifecycle.finish_disconnect();
        drop(teardown);
        emit_state(&self.on_state_change, SessionState::Idle);
    }
}

#[wasm_bindgen]
impl WebTripSession {

    /// Whether audio is running but its `AudioContext` is suspended (`false`
    /// when audio is stopped).
    ///
    /// Browsers start a context suspended when it is not created from a user
    /// gesture, and iOS Safari may keep it suspended even after a successful
    /// `startAudio()` or `connect()`. Check this afterwards and, if it is
    /// `true`, show a "Tap to enable audio" control that calls `resumeAudio()`.
    #[wasm_bindgen(js_name = isAudioSuspended)]
    pub fn is_audio_suspended(&self) -> bool {
        self.audio_engine
            .borrow()
            .as_ref()
            .map(|e| e.is_suspended())
            .unwrap_or(false)
    }

    /// Resume a suspended `AudioContext` (see `isAudioSuspended()`). Call it
    /// directly from a user-gesture handler (for example a button's `onclick`)
    /// so the browser allows audio output. Resolves at once when audio is
    /// stopped.
    #[wasm_bindgen(js_name = resumeAudio)]
    pub async fn resume_audio(&self) -> Result<(), JsValue> {
        let ctx = self.audio_engine.borrow().as_ref().map(AudioEngine::context);
        if let Some(ctx) = ctx {
            resume_context(&ctx).await?;
        }
        Ok(())
    }
}

impl WebTripSession {
    /// The current network state: the aggregate over the connections' states
    /// (there is one lifecycle for the at most one connection).
    pub fn state(&self) -> SessionState {
        aggregate_network_state(&[self.lifecycle.state()])
    }

    fn network_is_idle(&self) -> bool {
        self.state() == SessionState::Idle
    }

    fn audio_is_stopped(&self) -> bool {
        self.audio_lifecycle.phase() == AudioPhase::Stopped
    }

    /// Connect the session over a caller-supplied, already-constructed transport,
    /// bypassing the real per-type transport construction in `connect`.
    ///
    /// Unit-test only. Runs the same [`connect_with`] path as
    /// `connect` (admission, audio bring-up, dial, `Connected`)
    /// against a mock transport with no live hub. With `with_audio` false it
    /// is `connect_network()`'s path instead: no audio is started. Pair with
    /// [`disconnect`] for the teardown half.
    ///
    /// [`connect_with`]: Self::connect_with
    /// [`disconnect`]: Self::disconnect
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) async fn connect_with_test_transport(
        &self,
        with_audio: bool,
        transport: Box<dyn Transport>,
        timeout_ms: i32,
    ) -> Result<(), JsValue> {
        let capture = with_audio.then_some(CaptureParams {
            device_id: None,
            auto_gain_control: false,
            echo_cancellation: false,
            noise_suppression: false,
        });
        self.connect_with(|| Ok(transport), "test-host", DEFAULT_SIGNALING_PORT, "", capture, timeout_ms)
            .await
    }

    /// Whether a connected transport is published (test-only observer).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn has_transport(&self) -> bool {
        self.connections.borrow().iter().any(|c| c.transport.borrow().is_some())
    }

    /// The live connection's send-ring width (test-only).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn wire_channels(&self) -> Option<usize> {
        self.with_send_ring(|ring| ring.wire_channels())
    }

    /// How many retired buffer sets are still being kept alive (test-only).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn retired_count(&self) -> usize {
        self.retired.borrow().len()
    }

    /// The current connection's (channels, fpp, initialized) as its regulator
    /// sees the peer (test-only).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn peer_shape(&self) -> Option<(usize, usize, bool)> {
        self.connections.borrow().first().map(|c| {
            let regulator = &c.buffers.regulator;
            (regulator.channels(), regulator.fpp(), regulator.is_initialized())
        })
    }

    /// Feed the current connection's regulator a packet as its transport
    /// would (test-only).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn push_peer_packet(&self, seq: u16, channels: usize, fpp: usize) {
        let connections = self.connections.borrow();
        let regulator = &connections.first().expect("a connection to push to").buffers.regulator;
        regulator.push_at(seq, channels, fpp, &vec![0.0; channels * fpp], 0.0);
    }

    /// Whether an audio engine is published (test-only observer).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn has_engine(&self) -> bool {
        self.audio_engine.borrow().is_some()
    }

    /// Which connection the engine's worklet node reads, if any (test-only).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn engine_attached(&self) -> Option<u64> {
        self.engine_attached.get()
    }

    /// The live connection's id (test-only).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn connection_id(&self) -> Option<u64> {
        self.connections.borrow().first().map(|c| c.id)
    }

    /// What currently ticks the published transport (test-only). Taking the
    /// transport from its slot alone stops `Transport::tick` calls; this
    /// observes the clock itself (WEB-73).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn tick_kind(&self) -> crate::tick_clock::TickKind {
        self.tick_clock.borrow().kind()
    }

    /// Hold engine bring-up after `AudioEngine` creation until `gate` resolves
    /// (test-only). Simulates a never-settling capture step such as an
    /// unanswered mic-permission prompt.
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn set_capture_gate(&self, gate: js_sys::Promise) {
        self.capture_waiting.set(false);
        *self.capture_gate.borrow_mut() = Some(gate);
    }

    /// Whether bring-up is currently blocked on the capture gate (test-only).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn is_capture_waiting(&self) -> bool {
        self.capture_waiting.get()
    }

    /// The stored output-device id (test-only), for asserting a rejected
    /// `set_output_device` did not overwrite it (WEB-72).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn stored_output_device_id(&self) -> Option<String> {
        self.output_device_id.borrow().clone()
    }

    // ========== Audio ==========

    /// Admit an audio start, waiting out a stop still tearing down, and bring
    /// the engine up under `abort` (see [`start_engine_unless_aborted`]).
    /// Anything created is released before an `Err` is returned.
    ///
    /// [`start_engine_unless_aborted`]: Self::start_engine_unless_aborted
    async fn start_audio_with(
        &self,
        capture: CaptureParams,
        abort: js_sys::Promise,
        abort_fn: js_sys::Function,
    ) -> Result<(), JsValue> {
        let epoch = self.admit_audio_start().await?;
        // Published before anything can re-enter, so a `stopAudio` finds the
        // start to abort and to wait for.
        let _start = InFlight::publish(&self.audio_start_in_flight);
        *self.abort_audio_in_flight.borrow_mut() = Some(abort_fn);
        let result = self.bring_up_audio(epoch, capture, abort).await;
        self.abort_audio_in_flight.borrow_mut().take();
        if result.is_err() {
            self.audio_lifecycle.abort_start(epoch);
        }
        result
    }

    /// Move `Stopped → Starting`, returning the epoch the start must quote to
    /// later transitions.
    async fn admit_audio_start(&self) -> Result<u64, JsValue> {
        loop {
            match self.audio_lifecycle.begin_start() {
                Ok(epoch) => return Ok(epoch),
                Err(StartRejected::Stopping) => await_in_flight(&self.audio_teardown_in_flight).await,
                Err(rejected) => return Err(js_error(rejected.message())),
            }
        }
    }

    /// Bring the engine up, attached to the published connection if there is
    /// one, then publish it, checking after every await that no `stopAudio`
    /// has overtaken this start (`epoch`).
    async fn bring_up_audio(&self, epoch: u64, capture: CaptureParams, abort: js_sys::Promise) -> Result<(), JsValue> {
        let attached = self.published_buffers();
        let attached_id = attached.map(|p| p.id);
        let generation = self.engine_generation.get() + 1;
        // From here the node being built may be handed these buffers, and a
        // disconnect can end their connection before the engine is published:
        // `end_connection` finds this and retires them against `generation`.
        self.engine_generation.set(generation);
        self.engine_attached.set(attached_id);
        let engine = match self
            .start_engine_unless_aborted(attached.map(|p| (p.ring, p.regulator)), capture, abort)
            .await
        {
            Ok(engine) => engine,
            Err((e, engine)) => {
                match engine {
                    Some(engine) => self.stop_engine(engine, attached_id, generation).await,
                    None => {
                        // No worklet was installed (an aborted bring-up never
                        // installs one), so nothing retired against it is read.
                        self.engine_attached.set(None);
                        self.retired.borrow_mut().free_tagged(generation);
                    }
                }
                return Err(e);
            }
        };

        // Publish with no await in between, so `Running` is never observable
        // without the engine, and a connection published while the engine was
        // starting is picked up.
        *self.audio_engine.borrow_mut() = Some(engine);
        let mut failure = None;
        if self.published_buffers().map(|p| p.id) != attached_id {
            failure = self.attach_published().err();
        }
        if failure.is_none() && !self.audio_lifecycle.finish_start(epoch) {
            failure = Some(cancelled_error());
        }
        if let Some(e) = failure {
            let engine = self.audio_engine.borrow_mut().take().expect("the engine just published");
            let attached = self.engine_attached.get();
            self.stop_engine(engine, attached, generation).await;
            return Err(e);
        }
        self.refresh_tick_clock();
        Ok(())
    }

    /// Stop audio; the body of `stopAudio`, which also serves a connect that
    /// owns its audio.
    async fn stop_audio_inner(&self) {
        match self.audio_lifecycle.begin_stop() {
            StopStart::AlreadyStopped => return,
            StopStart::AlreadyStopping => {
                await_in_flight(&self.audio_teardown_in_flight).await;
                return;
            }
            StopStart::Started => {}
        }
        let teardown = InFlight::publish(&self.audio_teardown_in_flight);

        // Abort a start still in progress and let it release what it created.
        let abort = self.abort_audio_in_flight.borrow_mut().take();
        if let Some(abort) = abort {
            let _ = abort.call1(&JsValue::NULL, &cancelled_error());
        }
        await_in_flight(&self.audio_start_in_flight).await;

        let engine = self.audio_engine.borrow_mut().take();
        if let Some(engine) = engine {
            let attached = self.engine_attached.get();
            self.stop_engine(engine, attached, self.engine_generation.get()).await;
        }
        // Audio is no longer running: a WebRTC connection falls back to the
        // interval clock.
        self.refresh_tick_clock();

        self.audio_lifecycle.finish_stop();
        drop(teardown);
    }

    /// Close `engine`'s context, then settle what that proves about the buffers
    /// its worklet node read.
    ///
    /// A fulfilled close proves the worklet gone, which frees every set retired
    /// against this engine (`generation`). One that timed out proves nothing,
    /// so a connection the node was `attached` to is held (see
    /// [`ConsumerHold`]) until the node's own `"stopped"` acknowledgement, if
    /// one ever comes.
    ///
    /// `engine_attached` stays set until the close has settled, so a connection
    /// ended meanwhile is retired against `generation` rather than freed.
    async fn stop_engine(&self, mut engine: AudioEngine, attached: Option<u64>, generation: u64) {
        let had_node = engine.is_capturing();
        let (closed, stopped) = engine.stop_capture_acknowledged().await;
        if closed {
            // This engine's own sets only: an earlier close that timed out left
            // its sets retired because its worklet may still be popping, and
            // this (different) context closing proves nothing about it.
            self.retired.borrow_mut().free_tagged(generation);
        } else {
            web_sys::console::warn_1(
                &"⚠️ AudioContext.close timed out; keeping the buffers its worklet read alive until \
                  the worklet acknowledges it stopped"
                    .into(),
            );
            if let (true, Some(id)) = (had_node, attached) {
                let hold = stopped.map_or(ConsumerHold::Forever, ConsumerHold::Until);
                if let Some(connection) = self.connections.borrow_mut().iter_mut().find(|c| c.id == id) {
                    connection.hold = hold;
                }
            }
        }
        self.engine_attached.set(None);
    }

    /// Point the engine's worklet node at the published connection's buffers,
    /// or at none when there is no such connection.
    fn attach_published(&self) -> Result<(), JsValue> {
        let target = self.published_buffers();
        let (ring, regulator) = or_detached(target.map(|p| (p.ring, p.regulator)));
        let mut engine = self.audio_engine.borrow_mut();
        let Some(engine) = engine.as_mut() else { return Ok(()) };
        // The replaced node read no connection (a connection is only attached
        // when it is published, and the one being attached is the one that was
        // not), so its acknowledgement proves nothing.
        engine.attach_network(ring, regulator)?;
        self.engine_attached.set(target.map(|p| p.id));
        Ok(())
    }

    /// Point the engine's node at no connection if it reads `id`, saying what
    /// the caller must wait for before `id`'s buffers may be freed.
    fn detach_engine_from(&self, id: u64) -> Detached {
        if self.engine_attached.get() != Some(id) {
            return Detached::NotAttached;
        }
        let mut engine = self.audio_engine.borrow_mut();
        let Some(engine) = engine.as_mut() else { return Detached::InTransit };
        match engine.attach_network(std::ptr::null_mut(), std::ptr::null_mut()) {
            Ok(replaced) => {
                self.engine_attached.set(None);
                replaced.map_or(Detached::NotAttached, Detached::Pending)
            }
            Err(e) => {
                web_sys::console::error_2(&"Could not detach the audio node from a connection".into(), &e);
                Detached::Failed
            }
        }
    }

    // ========== Network ==========

    /// Admit and dial a connection (starting audio first when `audio` is given
    /// and none is running), and publish it; the transport-agnostic body of
    /// `connectNetwork` and `connect`. On failure (including cancellation)
    /// everything the attempt created is released before this returns.
    async fn connect_with(
        &self,
        new_transport: impl FnOnce() -> Result<Box<dyn Transport>, JsValue>,
        server: &str,
        port: u16,
        client_name: &str,
        audio: Option<CaptureParams>,
        timeout_ms: i32,
    ) -> Result<(), JsValue> {
        let epoch = self.admit_connect().await?;
        // Published before anything can re-enter (the state callback below),
        // so a disconnect issued from it waits for this connect to settle.
        let _connect = InFlight::publish(&self.connect_in_flight);
        emit_state(&self.on_state_change, SessionState::Connecting);

        let (abort, abort_fn, _reject) = crate::js_util::make_promise();
        *self.abort_in_flight.borrow_mut() = Some(abort_fn.clone());
        let result = self
            .bring_up(epoch, new_transport, server, port, client_name, audio, abort, abort_fn, timeout_ms)
            .await;
        self.abort_in_flight.borrow_mut().take();

        if result.is_err() {
            // Audio this connect started goes with it, unless `stopAudio`
            // took it out of the connect's hands (and the caller may have
            // started audio of their own since). When a disconnect overtook
            // this connect, the disconnect decides: `disconnect()` stops the
            // audio, `disconnectNetwork()` leaves it running.
            if self.lifecycle.is_current(epoch) && self.audio_owned_by_connect.replace(false) {
                self.stop_audio_inner().await;
            }
            if self.lifecycle.abort_connect(epoch) {
                emit_state(&self.on_state_change, SessionState::Idle);
            }
        }
        result
    }

    /// Wait out any teardown in flight, then move `Idle → Connecting`,
    /// returning the epoch the connect must quote to later transitions.
    async fn admit_connect(&self) -> Result<u64, JsValue> {
        loop {
            match self.lifecycle.begin_connect() {
                Ok(epoch) => return Ok(epoch),
                // `disconnect` publishes its teardown before leaving
                // `Disconnecting` can be observed, so this always waits.
                Err(ConnectRejected::Disconnecting) => await_in_flight(&self.teardown_in_flight).await,
                Err(rejected) => return Err(js_error(rejected.message())),
            }
        }
    }

    /// Start audio (when asked to and none is running), dial the transport
    /// (under the connect timeout), and publish it, checking after every await
    /// that no disconnect has overtaken this connect (`epoch`). Anything
    /// created is released before an `Err` is returned, except audio this
    /// connect started, which `audio_owned_by_connect` tells the caller to stop.
    #[allow(clippy::too_many_arguments)]
    async fn bring_up(
        &self,
        epoch: u64,
        new_transport: impl FnOnce() -> Result<Box<dyn Transport>, JsValue>,
        server: &str,
        port: u16,
        client_name: &str,
        audio: Option<CaptureParams>,
        abort: js_sys::Promise,
        abort_fn: js_sys::Function,
        timeout_ms: i32,
    ) -> Result<(), JsValue> {
        // Audio first, so a permission prompt is answered before the hub is
        // dialed, and under the same abort so `disconnect()` never waits on it.
        if let Some(capture) = audio {
            // A `startAudio` already in flight is adopted rather than raced:
            // wait for it (abortably, as it may sit on a permission prompt),
            // then see whether it left audio running or failed.
            while self.audio_lifecycle.phase() == AudioPhase::Starting {
                unless_aborted(
                    async {
                        await_in_flight(&self.audio_start_in_flight).await;
                        Ok(())
                    },
                    abort.clone(),
                )
                .await?;
            }
            if self.audio_lifecycle.state() != AudioState::Running {
                self.start_audio_with(capture, abort.clone(), abort_fn.clone()).await?;
                // Owned from the moment it runs, so a `stopAudio` during the
                // dial releases it and a later caller-started engine is not
                // mistaken for this connect's.
                self.audio_owned_by_connect.set(true);
            }
            if !self.lifecycle.is_current(epoch) {
                return Err(cancelled_error());
            }
        }

        let mut transport = new_transport()?;
        let transport_type = transport.transport_type();
        let connection = self.open_connection(transport_type)?;
        let mut buffers = self.audio_buffer_config(connection);
        if let Some(max_packet_bytes) = transport.max_packet_bytes() {
            // A transport that knows its packet limit up front is fitted here;
            // one that discovers it on connecting fits itself (see `Transport`).
            match fit_send_fpp(
                &format!("{transport_type:?}"),
                max_packet_bytes,
                buffers.send_channels,
                buffers.buffer_size,
            ) {
                Ok(fpp) => buffers.buffer_size = fpp,
                Err(e) => {
                    // Nothing was shared yet, so there is nothing to wait on.
                    self.end_connection(connection, None).await;
                    return Err(js_error(e.message()));
                }
            }
        }
        transport.set_audio_buffers(buffers);
        let failed_while_connecting = Rc::new(Cell::new(false));
        transport.set_on_state_change(self.failure_bridge(
            epoch,
            transport_type,
            Rc::clone(&failed_while_connecting),
        ));

        // The timeout bounds the dial only: audio bring-up may wait on the user
        // (mic-permission prompt), so before and after it only a disconnect()
        // can settle `abort`.
        let dial_timer = match arm_timeout(&abort_fn, timeout_ms) {
            Ok(timer) => timer,
            Err(e) => {
                self.end_connection(connection, None).await;
                return Err(e);
            }
        };
        // Clone so the dial and the timeout race the same abort;
        // `unless_aborted` only drops its JsFuture, not the underlying promise.
        let dialed = unless_aborted(
            async { transport.connect(server, port, client_name).await.map_err(|e| js_error(e.message())) },
            abort,
        )
        .await;
        if let Some(window) = web_sys::window() {
            window.clear_timeout_with_handle(dial_timer);
        }
        let dialed = dialed.and_then(|()| {
            if self.lifecycle.is_current(epoch) { Ok(()) } else { Err(cancelled_error()) }
        });
        if let Err(e) = dialed {
            self.end_connection(connection, Some(transport)).await;
            return Err(e);
        }

        // A failure reported before `Connected` cannot fail the connection
        // (there is none yet), so it fails the connect instead.
        if failed_while_connecting.get() {
            self.end_connection(connection, Some(transport)).await;
            return Err(js_error("The connection failed while it was being set up"));
        }

        // Publish with no await in between, so the `Connected` state is never
        // observable without the transport, its clock, and the engine attached.
        self.publish_transport(connection, transport);
        self.with_send_ring(|ring| ring.set_streaming(true));
        let published = match self.attach_published() {
            Err(e) => Err(e),
            Ok(()) if !self.lifecycle.finish_connect(epoch) => Err(cancelled_error()),
            Ok(()) => Ok(()),
        };
        if let Err(e) = published {
            self.stop_network_loop();
            let (_, transport) = self.take_live_transport();
            self.end_connection(connection, transport).await;
            return Err(e);
        }
        self.refresh_tick_clock();
        emit_state(&self.on_state_change, SessionState::Connected);
        Ok(())
    }

    /// The state-change callback handed to the transport admitted under
    /// `epoch`: a failure report fails that connection (and only that one).
    /// One reported before the connection is published sets
    /// `failed_while_connecting` for `bring_up` to act on.
    fn failure_bridge(
        &self,
        epoch: u64,
        transport_type: TransportType,
        failed_while_connecting: Rc<Cell<bool>>,
    ) -> StateCallback {
        let lifecycle = Arc::clone(&self.lifecycle);
        let on_state_change = Rc::clone(&self.on_state_change);
        Box::new(move |state: TransportState| {
            if !transport_state_is_failure(transport_type, transport_state_str(state)) {
                return;
            }
            if lifecycle.fail(epoch) {
                emit_state(&on_state_change, SessionState::Error);
            } else {
                failed_while_connecting.set(true);
            }
        })
    }

    /// Open a connection: refuse one beyond `MAX_CONNECTIONS`, otherwise
    /// allocate its fresh stream buffers and register it. Returns its id.
    /// Every path that fails or ends it goes through
    /// [`end_connection`](Self::end_connection).
    fn open_connection(&self, transport_type: TransportType) -> Result<u64, JsValue> {
        check_connection_capacity(self.connections.borrow().len()).map_err(|e| js_error(&e))?;
        let id = self.next_connection_id.get();
        self.next_connection_id.set(id + 1);
        let buffers = StreamBuffers::new(self.send_channels.get(), self.receive_channels.get(), self.receive_fpp);
        self.connections.borrow_mut().push(Connection {
            id,
            transport: Rc::new(RefCell::new(None)),
            transport_type,
            buffers,
            hold: ConsumerHold::Clear,
        });
        Ok(id)
    }

    /// Put `transport` in connection `id`'s slot, where the tick clock finds it.
    fn publish_transport(&self, id: u64, transport: Box<dyn Transport>) {
        self.with_connection(id, |connection| *connection.transport.borrow_mut() = Some(transport));
    }

    /// Run `f` on connection `id`, if it is still live.
    fn with_connection<R>(&self, id: u64, f: impl FnOnce(&Connection) -> R) -> Option<R> {
        self.connections.borrow().iter().find(|c| c.id == id).map(f)
    }

    /// Run `f` on connection `id`'s buffers; the connection must have been
    /// opened by the caller and not yet ended.
    fn with_opened_buffers<R>(&self, id: u64, f: impl FnOnce(&StreamBuffers) -> R) -> R {
        self.with_connection(id, |c| f(&c.buffers)).expect("the connection was just opened")
    }

    /// The connection whose transport is published, with its buffers as the
    /// engine's worklet reads them. A connection still dialing is not
    /// published, so the engine never reads buffers a transport may still be
    /// setting up.
    fn published_buffers(&self) -> Option<Published> {
        self.connections.borrow().iter().find(|c| c.transport.borrow().is_some()).map(|c| Published {
            id: c.id,
            ring: c.buffers.ring_ptr(),
            regulator: c.buffers.regulator_ptr(),
        })
    }

    /// Take the published transport out of the live connection, returning its
    /// id too (`None` for both when there is no connection).
    fn take_live_transport(&self) -> (Option<u64>, Option<Box<dyn Transport>>) {
        match self.connections.borrow().first() {
            Some(connection) => (Some(connection.id), connection.transport.borrow_mut().take()),
            None => (None, None),
        }
    }

    /// Run `f` on the live connection's send ring, if there is one.
    fn with_send_ring<R>(&self, f: impl FnOnce(&RingBuffer) -> R) -> Option<R> {
        self.connections.borrow().first().map(|c| f(&c.buffers.ring))
    }

    /// End connection `id`: quiesce and drop its transport, point the engine's
    /// node away from its buffers, then retire them, freeing them once nothing
    /// can still reach them. The caller must already have stopped the tick
    /// clock if it was started.
    ///
    /// The buffers are retired rather than reset for the next connection: the
    /// next connection allocates its own. The producer is quiesced when
    /// `Transport::close` reports a clean shutdown (not a force-terminated
    /// worker); the consumer, when the node that read them has acknowledged
    /// `"stopped"` (or its context closed: see [`stop_engine`]), or there was
    /// none. Either failing leaves the retired buffers in the list, alive,
    /// since a worker or worklet may still be touching them. Audio keeps
    /// running: only the node is replaced, which is the brief glitch an
    /// output-device change already causes.
    ///
    /// [`stop_engine`]: Self::stop_engine
    async fn end_connection(&self, id: u64, transport: Option<Box<dyn Transport>>) {
        let producer_quiescent = match transport {
            // `transport` is dropped at end of scope; its `Drop` impl is a
            // no-op on already-closed state.
            Some(mut transport) => transport.close().await,
            None => true,
        };

        // No await from here: the node is replaced and the buffers retired in
        // one step, so nothing can attach the engine to them in between.
        let detached = self.detach_engine_from(id);
        let Some(connection) = self.take_connection(id) else { return };
        let mut waits = Vec::new();
        let mut engine_tag = None;
        let mut never = matches!(detached, Detached::Failed);
        let in_transit = matches!(detached, Detached::InTransit);
        if let Detached::Pending(stopped) = detached {
            waits.push(stopped);
            engine_tag = Some(self.engine_generation.get());
        }
        match connection.hold {
            ConsumerHold::Clear => {}
            ConsumerHold::Until(stopped) => {
                // An engine that already closed unproven: only its own
                // acknowledgement frees these, not the current engine's close.
                waits.push(stopped);
                engine_tag = None;
            }
            ConsumerHold::Forever => never = true,
        }

        if in_transit && producer_quiescent && !never && waits.is_empty() {
            // Freed by the closing (or failed-start) engine once it has proven
            // its worklet gone.
            self.retired.borrow_mut().retire_for(connection.buffers, self.engine_generation.get());
            return;
        }
        // An engine in transit that an earlier engine's hold also covers has
        // two proofs to wait for and only one is tracked: keep it alive.
        if !producer_quiescent || never || in_transit {
            self.retired.borrow_mut().retire(connection.buffers);
            web_sys::console::warn_1(
                &"⚠️ Teardown was not clean (the transport worker was force-terminated or an audio \
                  node could not be proven stopped); keeping the connection's buffers alive"
                    .into(),
            );
            return;
        }
        self.retire_until_stopped(connection.buffers, waits, engine_tag);
    }

    /// Remove connection `id` from the live list.
    fn take_connection(&self, id: u64) -> Option<Connection> {
        let mut connections = self.connections.borrow_mut();
        connections.iter().position(|c| c.id == id).map(|index| connections.remove(index))
    }

    /// Retire `buffers` until every promise in `stopped` settles, then free
    /// them: each is the `"stopped"` acknowledgement of a worklet node that
    /// read them, after which the render thread never touches them again. With
    /// none to wait for, they are freed at once. If one never settles (a
    /// suspended context delivers no port messages), they stay alive: no other
    /// context's close proves that node idle. `engine_tag` additionally lets
    /// that engine's fulfilled close free them (see [`stop_engine`]).
    ///
    /// [`stop_engine`]: Self::stop_engine
    fn retire_until_stopped(&self, buffers: StreamBuffers, stopped: Vec<js_sys::Promise>, engine_tag: Option<u64>) {
        let key = {
            let mut retired = self.retired.borrow_mut();
            match engine_tag {
                Some(tag) => retired.retire_for(buffers, tag),
                None => retired.retire(buffers),
            }
        };
        free_when_stopped(&self.retired, key, stopped);
    }

    /// Connection `id`'s buffers, as handed to a transport before it connects.
    fn audio_buffer_config(&self, id: u64) -> AudioBufferConfig {
        let (ring, regulator) = self.with_opened_buffers(id, |b| (b.ring_ptr(), b.regulator_ptr()));
        // SAFETY: both pointers are into the connection's boxed buffers, which
        // stay alive (live, then retired) until every agent holding them has
        // stopped; they are only ever dereferenced as shared references
        // (`SharedPtr::as_ref`), their mutable state being atomics and
        // `UnsafeCell`.
        unsafe {
            AudioBufferConfig {
                local_to_network: SharedPtr::new(ring),
                network_to_local: SharedPtr::new(regulator),
                buffer_size: self.send_fpp,
                send_channels: self.send_channels.get(),
                receive_channels: self.receive_channels.get(),
            }
        }
    }

    /// Create the `AudioEngine` and start capture, racing the work against
    /// `abort` so a `stopAudio()`/`disconnect()` can finish while
    /// `getUserMedia` is still pending. `network` is the connection the first
    /// worklet node reads, or none.
    ///
    /// On abort the `AbortError` is returned immediately. If bring-up is still
    /// pending, a detached task finishes it and stops the engine (no worklet
    /// is installed once cancelled). If bring-up had already stored a live
    /// engine, it is returned alongside the error so the caller's `stop_engine`
    /// awaits `AudioContext.close` before any buffers are freed.
    /// A cancel flag checked after each await prevents an abandoned capture
    /// from installing a worklet against buffers that are about to be freed.
    async fn start_engine_unless_aborted(
        &self,
        network: Option<NetworkBuffers>,
        capture: CaptureParams,
        abort: js_sys::Promise,
    ) -> Result<AudioEngine, (JsValue, Option<AudioEngine>)> {
        #[cfg(all(test, target_arch = "wasm32"))]
        crate::test_support::note_start_engine();
        let params_addr = self.audio_params.addr();
        let output_device_id = self.output_device_id.borrow().clone();
        let input_channels = self.input_channels.get() as u32;
        let output_channels = self.output_channels.get() as u32;
        let (capture_gate, capture_waiting) = {
            #[cfg(all(test, target_arch = "wasm32"))]
            {
                (
                    self.capture_gate.borrow_mut().take(),
                    Some(Rc::clone(&self.capture_waiting)),
                )
            }
            #[cfg(not(all(test, target_arch = "wasm32")))]
            {
                (None, None)
            }
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let outcome: Rc<RefCell<Option<Result<AudioEngine, JsValue>>>> = Rc::new(RefCell::new(None));
        let context_slot: Rc<RefCell<Option<web_sys::AudioContext>>> = Rc::new(RefCell::new(None));
        let (done, resolve_done, _) = crate::js_util::make_promise();

        {
            let cancelled = Arc::clone(&cancelled);
            let context_slot = Rc::clone(&context_slot);
            let outcome = Rc::clone(&outcome);
            wasm_bindgen_futures::spawn_local(async move {
                let result = bring_up_engine(
                    params_addr,
                    network,
                    capture,
                    output_device_id,
                    input_channels,
                    output_channels,
                    Arc::clone(&cancelled),
                    context_slot,
                    capture_gate,
                    capture_waiting,
                )
                .await;
                // No `cancelled` re-check here: nothing awaits between the last
                // check in `start_capture_cancellable` and this store. If abort
                // wins after this store, the `Err` arm below takes the engine back.
                *outcome.borrow_mut() = Some(result);
                let _ = resolve_done.call0(&JsValue::NULL);
            });
        }

        let raced = unless_aborted(async move { JsFuture::from(done).await.map(|_| ()) }, abort).await;

        match raced {
            Ok(()) => outcome
                .borrow_mut()
                .take()
                .unwrap_or_else(|| Err(js_error("engine bring-up produced no result")))
                .map_err(|e| (e, None)),
            Err(reason) => {
                cancelled.store(true, Ordering::Release);
                // If bring-up already stored a live engine (its worklet may
                // still be popping the regulator on the audio thread), hand it
                // back with the error so the caller awaits `AudioContext.close`
                // before the buffers are freed. Detaching it here would let
                // them be freed immediately.
                let engine = match outcome.borrow_mut().take() {
                    Some(Ok(engine)) => Some(engine),
                    Some(Err(_)) => None,
                    None => {
                        // Bring-up is still pending (typically an unanswered
                        // `getUserMedia` prompt, which cannot be cancelled).
                        // No worklet exists yet, so close the context now
                        // rather than holding it for as long as the prompt
                        // stays open; the detached task stops the stream when
                        // the prompt finally settles.
                        if let Some(ctx) = context_slot.borrow_mut().take() {
                            close_audio_context_now(&ctx);
                        }
                        None
                    }
                };
                Err((reason, engine))
            }
        }
    }

    // ========== Tick clock ==========

    /// Make the tick clock the one the current audio and connection call for.
    fn refresh_tick_clock(&self) {
        let source = self
            .connections
            .borrow()
            .iter()
            // WebTransport's worker drives itself; only a transport that
            // is ticked needs a clock.
            .find(|c| c.transport_type == TransportType::WebRTC && c.transport.borrow().is_some())
            .map(|c| TickSource {
                connection: c.id,
                transport: Rc::clone(&c.transport),
                flag_ptr: c.buffers.ring.get_has_data_flag_ptr(),
            });
        let audio_running = self.audio_lifecycle.state() == AudioState::Running;
        self.tick_clock.borrow_mut().ensure_running_for(audio_running, source.as_ref());
    }

    /// Stop ticking the connection and mark its send ring as not streaming.
    fn stop_network_loop(&self) {
        self.tick_clock.borrow_mut().stop();
        self.with_send_ring(|ring| ring.set_streaming(false));
    }

    /// Shared guard for the channel-count setters: a count may change only
    /// while `settable` (the network idle for the wire counts, audio stopped
    /// for the device counts) and only within [1, `max`] (the wire ceiling for
    /// send/receive, the device ceiling for input/output). Logs the reason
    /// (`which` names the setting) when it may not.
    fn can_set_channel_count(&self, which: &str, channels: u8, max: u8, settable: bool) -> bool {
        if !settable {
            web_sys::console::warn_1(
                &format!("⚠️ Cannot change {which} channels while it is in use").into(),
            );
            return false;
        }
        if !is_valid_channel_count(channels, max) {
            web_sys::console::warn_1(
                &format!("⚠️ Invalid {which} channel count: {channels}").into(),
            );
            return false;
        }
        true
    }
}

/// Free entry `key` of `retired` once every promise in `stopped` (each a worklet
/// node's `"stopped"` acknowledgement, after which its render thread never
/// touches the buffers again) settles; at once when there are none. If one never
/// settles (a suspended or closed context delivers no port messages) the entry
/// stays alive.
fn free_when_stopped(retired: &Rc<RefCell<RetiredList<StreamBuffers>>>, key: u64, stopped: Vec<js_sys::Promise>) {
    if stopped.is_empty() {
        retired.borrow_mut().free(key);
        return;
    }
    let retired = Rc::clone(retired);
    wasm_bindgen_futures::spawn_local(async move {
        for promise in stopped {
            let _ = JsFuture::from(promise).await;
        }
        retired.borrow_mut().free(key);
    });
}

impl Drop for WebTripSession {
    fn drop(&mut self) {
        // Drop can't await, but the worklet render thread and the WebTransport
        // worker both hold raw pointers into the connection's buffers, and
        // neither stops synchronously (`"stop"` and `{type:"disconnect"}` are
        // just posted messages). Freeing the buffers here would let a pending
        // `process()` or worker `push()` dereference freed storage. Instead,
        // retire them into a task that runs the same quiescing teardown as
        // `disconnect()` and frees them only afterwards. (No connect can be in
        // flight: it borrows the session for its whole duration.)
        //
        // Retire the lifecycle epoch before the deferred `transport.close()`
        // runs: WebRTC's close notifies `"closed"`, which `failure_bridge`
        // treats as a failure, and that leaked callback still holds
        // `Arc<Lifecycle>`. Without this, `fail(epoch)` would succeed and
        // emit `"error"` for a session that no longer exists (WEB-68).
        let _ = self.lifecycle.begin_disconnect();
        let _ = self.audio_lifecycle.begin_stop();
        self.stop_network_loop();
        let (_, transport) = self.take_live_transport();
        let engine = self.audio_engine.get_mut().take();
        let attached = self.engine_attached.get();
        let generation = self.engine_generation.get();
        let retired = Rc::clone(&self.retired);
        // Each connection's buffers, retired, and whether anything but this
        // engine's own close could still be reading them (`hold`).
        let sets: Vec<(u64, u64, ConsumerHold)> = self
            .connections
            .get_mut()
            .drain(..)
            .map(|connection| {
                let key = retired.borrow_mut().retire(connection.buffers);
                (key, connection.id, connection.hold)
            })
            .collect();
        wasm_bindgen_futures::spawn_local(async move {
            let producer_quiescent = match transport {
                Some(mut transport) => transport.close().await,
                None => true,
            };
            let (consumer_quiescent, stopped) = match engine {
                Some(mut engine) => engine.stop_capture_acknowledged().await,
                None => (true, None),
            };
            if !producer_quiescent {
                // A force-terminated worker may still be pushing: leak.
                return;
            }
            for (key, id, hold) in sets {
                let read_by_engine = attached == Some(id);
                match hold {
                    ConsumerHold::Clear => {
                        if consumer_quiescent || !read_by_engine {
                            retired.borrow_mut().free(key);
                        } else if let Some(stopped) = stopped.clone() {
                            // Only the close failed: the worklet's `"stopped"`
                            // still proves it is done with this set. Otherwise
                            // a worklet may still be popping and it is leaked
                            // rather than freed.
                            free_when_stopped(&retired, key, vec![stopped]);
                        }
                    }
                    // An earlier engine's close timed out: only its own
                    // acknowledgement frees the set, as in `end_connection`.
                    // Also read by this engine, two proofs are needed and only
                    // one is tracked: keep it alive.
                    ConsumerHold::Until(earlier) if !read_by_engine => free_when_stopped(&retired, key, vec![earlier]),
                    ConsumerHold::Until(_) | ConsumerHold::Forever => {}
                }
            }
            if consumer_quiescent {
                // Sets the engine's node was replaced away from earlier.
                retired.borrow_mut().free_tagged(generation);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Channel-count validation
    // -----------------------------------------------------------------------

    #[test]
    fn channel_count_validation_boundaries() {
        // Wire and device settings share the guard but not the ceiling: a
        // count valid for a device (an 18-channel interface) is not valid on
        // the wire.
        for max in [MAX_WIRE_CHANNELS, MAX_DEVICE_CHANNELS] {
            assert!(!is_valid_channel_count(0, max));
            assert!(is_valid_channel_count(1, max));
            assert!(is_valid_channel_count(max, max));
            assert!(!is_valid_channel_count(max + 1, max));
        }
        assert!(is_valid_channel_count(18, MAX_DEVICE_CHANNELS));
        assert!(!is_valid_channel_count(18, MAX_WIRE_CHANNELS));
    }

    // -----------------------------------------------------------------------
    // Browser tests (web_sys / Web Audio + async transport lifecycle)
    // -----------------------------------------------------------------------
    //
    // Real-browser coverage of the async connect/disconnect lifecycle and the
    // `AudioContext`-dependent audio controls, run in headless Chrome via
    // `npm run test:wasm`. The per-binary browser opt-in
    // (`wasm_bindgen_test_configure!(run_in_browser)`) lives once in
    // `crate::test_support`.
    //
    // The lifecycle state machine's transitions are covered natively in
    // `webtrip_core::lifecycle`; these tests cover the session's use of it: that the
    // resources follow the state (a cancelled connect releases its transport
    // and never publishes, teardown is serialized and coalesced) and that the
    // state-change callback reports it in order.
    //
    // Connects run over `test_support::MockTransport` via
    // `connect_with_test_transport`, which reuses `connect`'s whole
    // `connect_with` path without a live hub. The real WebRTC/WebTransport
    // arms' network handshakes (SDP/ICE, QUIC) still require a live JackTrip
    // hub and are covered by the integration tests; here they are only
    // exercised on their failure paths.

    #[cfg(target_arch = "wasm32")]
    use crate::test_support::{gate, recording_state_callback, wait_until, MockTransport};
    #[cfg(target_arch = "wasm32")]
    use crate::tick_clock::TickKind;
    #[cfg(target_arch = "wasm32")]
    use std::cell::RefCell;
    #[cfg(target_arch = "wasm32")]
    use std::rc::Rc;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen::JsCast;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen::closure::Closure;
    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;

    /// A session over `params`, which must outlive it (the session holds its
    /// address): declare `params` first so it is dropped last.
    #[cfg(target_arch = "wasm32")]
    fn new_session(params: &AudioParams) -> Rc<WebTripSession> {
        Rc::new(WebTripSession::new(SharedPtr::from_ref(params).addr()).expect("session construction should succeed"))
    }

    /// The session's (send, receive, input, output) channel counts.
    #[cfg(target_arch = "wasm32")]
    fn channel_config(session: &WebTripSession) -> (u8, u8, u8, u8) {
        (
            session.get_send_channels(),
            session.get_receive_channels(),
            session.get_input_channels(),
            session.get_output_channels(),
        )
    }

    /// Register a [`recording_state_callback`] on `session`, returning the
    /// shared log and the `Closure` (kept alive by the caller).
    #[cfg(target_arch = "wasm32")]
    fn attach_state_log(
        session: &WebTripSession,
    ) -> (Rc<RefCell<Vec<String>>>, Closure<dyn FnMut(String)>) {
        let (log, closure) = recording_state_callback();
        session.set_on_state_change(closure.as_ref().unchecked_ref::<js_sys::Function>().clone());
        (log, closure)
    }

    /// Run `future` as its own task, so the test can act while it is pending;
    /// the slot fills with its output once it finishes.
    #[cfg(target_arch = "wasm32")]
    fn spawn<T: 'static>(future: impl std::future::Future<Output = T> + 'static) -> Rc<RefCell<Option<T>>> {
        let output = Rc::new(RefCell::new(None));
        let slot = output.clone();
        wasm_bindgen_futures::spawn_local(async move {
            *slot.borrow_mut() = Some(future.await);
        });
        output
    }

    /// Spawn a mock connect on `session` (see [`spawn`]); `with_audio` false
    /// is the network-only connect.
    #[cfg(target_arch = "wasm32")]
    fn spawn_connect(
        session: &Rc<WebTripSession>,
        with_audio: bool,
        transport: MockTransport,
        timeout_ms: i32,
    ) -> Rc<RefCell<Option<Result<(), JsValue>>>> {
        let session = session.clone();
        spawn(async move { session.connect_with_test_transport(with_audio, Box::new(transport), timeout_ms).await })
    }

    /// Start audio with no capture options and no device (the fake one).
    #[cfg(target_arch = "wasm32")]
    async fn start_audio(session: &WebTripSession) -> Result<(), JsValue> {
        session.start_audio(None, false, false, false).await
    }

    /// Spawn a disconnect on `session` (see [`spawn`]).
    #[cfg(target_arch = "wasm32")]
    fn spawn_disconnect(session: &Rc<WebTripSession>) -> Rc<RefCell<Option<()>>> {
        let session = session.clone();
        spawn(async move { session.disconnect().await })
    }

    /// Wait (bounded) for a spawned task's output and take it.
    #[cfg(target_arch = "wasm32")]
    async fn settled<T>(output: &Rc<RefCell<Option<T>>>, what: &str) -> T {
        assert!(wait_until(|| output.borrow().is_some(), 2000, 5).await, "{what} never settled");
        output.borrow_mut().take().unwrap()
    }

    /// The `(name, message)` of a rejected connect. The names `AbortError`
    /// and `TimeoutError` are a cross-language contract: the demo tells a
    /// cancellation it caused from a failure by them.
    #[cfg(target_arch = "wasm32")]
    fn rejection(result: Result<(), JsValue>) -> (String, String) {
        let error = result.expect_err("the connect must reject").unchecked_into::<js_sys::Error>();
        (error.name().into(), error.message().into())
    }

    #[cfg(target_arch = "wasm32")]
    fn assert_log(log: &Rc<RefCell<Vec<String>>>, expected: &[&str], context: &str) {
        assert_eq!(*log.borrow(), expected, "state callbacks {context}");
    }

    /// Drive a full connect → audio-controls → disconnect cycle over the
    /// server-free [`MockTransport`], via `connect_with_test_transport`.
    ///
    /// Asserts the exact `connecting → connected → disconnecting → idle`
    /// callback sequence (and `getState()` agreeing with it), exercises the
    /// `AudioContext`-backed `is_audio_suspended`/`resume_audio` on the
    /// now-live engine, and checks the transport is published while connected
    /// and closed once by teardown. Also asserts the network tick loop calls
    /// `Transport::tick` while connected and stops after `disconnect()`
    /// (WEB-73). Along the way it pins the channel-count setters' contract:
    /// applied while `Idle`, ignored while connected, and retained across
    /// disconnect (nothing narrows them). This runs the real async/await
    /// connect path, `AudioEngine::create`, worklet bootstrap, and the
    /// `Atomics.waitAsync` audio-callback loop in the browser — only the
    /// network transport is mocked.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_mock_connect_disconnect_lifecycle() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);

        assert_eq!(session.get_state(), "idle");

        // Every count away from its default, so a setter that ignored its
        // argument (or wrote the wrong field) fails.
        let configured = (2, 1, 1, 1);
        session.set_send_channels(configured.0);
        session.set_receive_channels(configured.1);
        session.set_input_channels(configured.2);
        session.set_output_channels(configured.3);
        assert_eq!(channel_config(&session), configured, "setters must apply while Idle");

        let transport = MockTransport::new();
        let probe = transport.probe();
        session
            .connect_with_test_transport(true, Box::new(transport), CONNECT_TIMEOUT_MS)
            .await
            .expect("mock connect should succeed");

        assert_eq!(session.get_state(), "connected");
        assert!(session.has_transport());
        assert_log(&log, &["connecting", "connected"], "after connect");
        assert_eq!(
            session.wire_channels(),
            Some(usize::from(configured.0)),
            "setSendChannels must reach the connection's send ring, which the render thread conforms to"
        );

        // The connect created a live `AudioEngine`, so the AudioContext-backed
        // controls now hit their engine branch. `is_audio_suspended` reads
        // `AudioContext.state` (must not panic, returns a bool); `resume_audio`
        // drives `AudioContext.resume()` and must resolve `Ok`.
        let _suspended: bool = session.is_audio_suspended();
        session
            .resume_audio()
            .await
            .expect("resume_audio with a live engine should resolve");

        // Channel counts are fixed for the connection.
        session.set_send_channels(1);
        session.set_receive_channels(2);
        session.set_input_channels(2);
        session.set_output_channels(2);
        assert_eq!(
            channel_config(&session),
            configured,
            "channel-count setters must be ignored while connected"
        );
        assert_eq!(session.wire_channels(), Some(usize::from(configured.0)));

        // The mock reports WebRTC and audio is running, so the clock is the
        // audio-callback loop. Wait until it has actually called
        // `Transport::tick` at least once before tearing down.
        assert!(
            wait_until(|| probe.ticks.get() > 0, 2000, 5).await,
            "the network tick loop must call Transport::tick while connected (got {})",
            probe.ticks.get()
        );
        assert_eq!(
            session.tick_kind(),
            TickKind::Atomics,
            "a WebRTC connect with audio running must tick from the audio callback"
        );
        assert_eq!(session.get_audio_state(), "running");

        session.disconnect().await;
        assert_eq!(session.get_state(), "idle");
        assert_eq!(session.get_audio_state(), "stopped", "connect's audio stops with its connection");
        assert!(!session.has_transport());
        assert_eq!(probe.closes.get(), 1, "teardown must close the transport once");
        assert_log(&log, &["connecting", "connected", "disconnecting", "idle"], "after disconnect");
        assert_eq!(
            channel_config(&session),
            configured,
            "disconnect must retain the configured channel counts"
        );

        // Taking the transport from its slot stops `Transport::tick` even if
        // the clock keeps running; `tick_kind` is what fails if
        // `stop_network_loop` is dropped from teardown (WEB-73 / WEB-58).
        assert_eq!(session.tick_kind(), TickKind::None, "disconnect must stop the tick clock");
        let ticks_after_disconnect = probe.ticks.get();
        crate::test_support::sleep_ms(100).await;
        assert_eq!(
            probe.ticks.get(),
            ticks_after_disconnect,
            "Transport::tick must stop being called after disconnect"
        );
    }

    /// Connect → disconnect must close the first engine's `AudioContext`
    /// before the connection's buffers are freed (and before disconnect
    /// returns), so a reconnect cannot leave a suspended worklet as a second
    /// consumer (WEB-66), and a fulfilled close frees every retired buffer set.
    /// A second connect then succeeds with a fresh engine and fresh buffers.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_closes_audiocontext_before_freeing_buffers_on_disconnect() {
        use crate::audio::engine::audio_context_state;

        let params = AudioParams::default();
        let session = new_session(&params);

        session
            .connect_with_test_transport(true, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("first mock connect should succeed");

        let first_ctx = session
            .audio_engine
            .borrow()
            .as_ref()
            .expect("connected session must own an AudioEngine")
            .context();
        assert_ne!(
            audio_context_state(&first_ctx).as_deref(),
            Some("closed"),
            "live engine context must not already be closed"
        );

        session.disconnect().await;
        assert_eq!(session.get_state(), "idle");
        assert_eq!(
            audio_context_state(&first_ctx).as_deref(),
            Some("closed"),
            "disconnect must close the first AudioContext before it returns (WEB-66)"
        );
        assert_eq!(session.retired_count(), 0, "a fulfilled close must free the retired buffers");
        assert!(
            session.audio_engine.borrow().is_none(),
            "disconnect must drop the engine after closing its context"
        );

        session
            .connect_with_test_transport(true, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("reconnect after a closed context must succeed");
        assert_eq!(session.get_state(), "connected");
        let second_ctx = session
            .audio_engine
            .borrow()
            .as_ref()
            .expect("reconnect must publish a new AudioEngine")
            .context();
        assert_ne!(
            audio_context_state(&second_ctx).as_deref(),
            Some("closed"),
            "the reconnect's AudioContext must be live"
        );

        session.disconnect().await;
        assert_eq!(session.get_state(), "idle");
    }

    /// A transport whose close was forced (a worker terminated after its
    /// timeout, which is not a join) may still have a push in flight, so the
    /// connection's buffers stay retired instead of being freed (WEB-67).
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn forced_transport_close_keeps_the_buffers_alive() {
        let params = AudioParams::default();
        let session = new_session(&params);

        session
            .connect_with_test_transport(true, Box::new(MockTransport::new().with_forced_close()), CONNECT_TIMEOUT_MS)
            .await
            .expect("mock connect should succeed");
        session.disconnect().await;
        assert_eq!(session.get_state(), "idle");
        assert_eq!(session.retired_count(), 1, "a forced close must not free buffers a worker may still write");
    }

    /// A hanging `AudioContext.close()` must not leave the session stuck in
    /// `"disconnecting"`: the close timeout lets `disconnect()` finish so a
    /// later connect is not blocked on `teardown_in_flight` forever (WEB-66).
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_disconnect_finishes_when_audiocontext_close_hangs() {
        use crate::audio::engine::AUDIO_CONTEXT_CLOSE_TIMEOUT_MS;
        use wasm_bindgen::closure::Closure;

        let params = AudioParams::default();
        let session = new_session(&params);
        session
            .connect_with_test_transport(true, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("mock connect should succeed");

        let ctx = session
            .audio_engine
            .borrow()
            .as_ref()
            .expect("connected session must own an AudioEngine")
            .context();
        let original_close = js_sys::Reflect::get(ctx.as_ref(), &"close".into())
            .expect("AudioContext must expose close");
        let hanging = js_sys::Promise::new(&mut |_resolve, _reject| {});
        let hanging_for_stub = hanging.clone();
        let stub = Closure::wrap(Box::new(move || -> js_sys::Promise {
            hanging_for_stub.clone()
        }) as Box<dyn FnMut() -> js_sys::Promise>);
        js_sys::Reflect::set(ctx.as_ref(), &"close".into(), stub.as_ref().unchecked_ref())
            .expect("installing the hanging close stub should succeed");

        let started = js_sys::Date::now();
        session.disconnect().await;
        let elapsed = js_sys::Date::now() - started;
        assert_eq!(session.get_state(), "idle");
        let min_ms = f64::from(AUDIO_CONTEXT_CLOSE_TIMEOUT_MS) - 50.0;
        let max_ms = f64::from(AUDIO_CONTEXT_CLOSE_TIMEOUT_MS) + 1500.0;
        assert!(
            elapsed >= min_ms && elapsed < max_ms,
            "disconnect should finish around the {AUDIO_CONTEXT_CLOSE_TIMEOUT_MS}ms close \
             timeout, got {elapsed}ms"
        );

        // The close never settled, so the worklet is not proven idle by it; its
        // own `"stopped"` acknowledgement is what frees the buffers.
        assert!(
            wait_until(|| session.retired_count() == 0, 1000, 5).await,
            "the worklet's acknowledgement must free the buffers after a close that timed out"
        );

        // Restore the real close and finish tearing down so this test does not
        // leave a live context for the rest of the suite.
        js_sys::Reflect::set(ctx.as_ref(), &"close".into(), &original_close)
            .expect("restoring AudioContext.close should succeed");
        drop(stub);
        if let Ok(promise) = ctx.close() {
            let _ = JsFuture::from(promise).await;
        }
    }

    /// A disconnect while the transport is still dialing cancels the connect:
    /// the connect rejects, closes the transport it created, and never reaches
    /// `connected` — not even once the dial would have completed (the zombie
    /// connection this guards against).
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_disconnect_cancels_a_dialing_connect() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);

        let (dial_gate, finish_dial) = gate();
        let transport = MockTransport::new().with_connect_gate(dial_gate);
        let probe = transport.probe();
        let connect = spawn_connect(&session, false, transport, CONNECT_TIMEOUT_MS);
        assert!(wait_until(|| probe.connects.get() == 1, 1000, 1).await, "the dial never started");

        session.disconnect().await;
        let (name, message) = rejection(settled(&connect, "cancelled connect").await);
        assert_eq!(name, "AbortError", "unexpected rejection: {message}");
        assert_eq!(probe.closes.get(), 1, "the cancelled connect must close its transport");
        assert!(!session.has_transport());

        let _ = finish_dial.call0(&JsValue::NULL);
        crate::test_support::sleep_ms(20).await;
        assert_eq!(session.get_state(), "idle");
        assert_log(&log, &["connecting", "disconnecting", "idle"], "around a cancelled dial");
    }

    /// A disconnect that races a dial which has already resolved `Ok` cancels
    /// at the post-dial `is_current(epoch)` check — not via the abort promise
    /// (still-gated dial) and not at `finish_connect` (audio already starting).
    /// That branch must close the transport that *did* connect (WEB-73).
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_disconnect_cancels_after_a_successful_dial() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);
        let _ = crate::test_support::take_start_engine_calls();

        let (dial_gate, finish_dial) = gate();
        let transport = MockTransport::new().with_connect_gate(dial_gate);
        let probe = transport.probe();
        let dial_succeeded = Rc::new(std::cell::Cell::new(false));
        let dial_flag = dial_succeeded.clone();
        probe.on_connected(move || dial_flag.set(true));

        let connect = spawn_connect(&session, false, transport, CONNECT_TIMEOUT_MS);
        assert!(wait_until(|| probe.connects.get() == 1, 1000, 1).await, "the dial never started");

        // Open the dial, then disconnect in the same turn (no yield between):
        // `finish_dial` schedules the dial's `Ok`; `disconnect` then bumps the
        // epoch and fires the abort. After yielding, `unless_aborted`
        // prefers a ready dial over a ready abort, so bring_up hits the
        // post-Ok `is_current` check, rejects, and `end_connection` closes the
        // transport that connected — without ever starting audio.
        let _ = finish_dial.call0(&JsValue::NULL);
        session.disconnect().await;

        let (name, message) = rejection(settled(&connect, "cancelled connect").await);
        assert_eq!(name, "AbortError", "unexpected rejection: {message}");
        assert!(
            dial_succeeded.get(),
            "the dial must have resolved Ok before the post-dial cancel check"
        );
        assert_eq!(
            crate::test_support::take_start_engine_calls(),
            0,
            "a network-only connect must never start audio"
        );
        assert_eq!(probe.closes.get(), 1, "the cancelled connect must close its transport");
        assert!(!session.has_transport());
        assert_eq!(session.get_state(), "idle");
        assert_log(
            &log,
            &["connecting", "disconnecting", "idle"],
            "around a cancelled post-dial",
        );
    }

    /// `setOutputDevice` is not gated on the network: while a `connectNetwork`
    /// dial is in flight over audio that is already running, the live engine
    /// is rerouted (PR 121 review). The Starting rejection is covered by
    /// `session_stop_audio_cancels_a_pending_start`.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_set_output_device_applies_while_dialing_over_running_audio() {
        let params = AudioParams::default();
        let session = new_session(&params);

        start_audio(&session).await.expect("audio should start");
        let (dial_gate, finish_dial) = gate();
        let transport = MockTransport::new().with_connect_gate(dial_gate);
        let probe = transport.probe();
        let connect = spawn_connect(&session, false, transport, CONNECT_TIMEOUT_MS);
        assert!(wait_until(|| probe.connects.get() == 1, 3000, 5).await, "the dial never started");
        assert_eq!(session.get_state(), "connecting");

        // `None` selects the default sink, so no real device id is needed.
        session
            .set_output_device(None)
            .await
            .expect("setOutputDevice while dialing over running audio must apply");
        assert_eq!(session.stored_output_device_id(), None);

        let _ = finish_dial.call0(&JsValue::NULL);
        settled(&connect, "connect after setOutputDevice")
            .await
            .expect("connect must succeed after the dial gate opens");
        assert_eq!(session.get_state(), "connected");

        session.disconnect().await;
        session.stop_audio().await;
    }

    /// A disconnect while capture is stuck (gated mid-bring-up, as when the
    /// mic-permission prompt is left unanswered) must resolve promptly: the
    /// connect rejects with `AbortError`, the session returns to `idle`, and a
    /// later connect is admitted — without waiting for the gated capture to
    /// finish (WEB-69).
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_disconnect_during_gated_capture_resolves_promptly() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);

        let (capture_gate, finish_capture) = gate();
        session.set_capture_gate(capture_gate);

        let transport = MockTransport::new();
        let probe = transport.probe();
        let connect = spawn_connect(&session, true, transport, CONNECT_TIMEOUT_MS);
        assert!(
            wait_until(|| session.is_capture_waiting(), 2000, 5).await,
            "bring-up never reached the capture gate"
        );

        let started = js_sys::Date::now();
        session.disconnect().await;
        let elapsed = js_sys::Date::now() - started;
        assert!(
            elapsed < 500.0,
            "disconnect must not wait for a stuck capture step, got {elapsed}ms"
        );

        let (name, message) = rejection(settled(&connect, "cancelled connect").await);
        assert_eq!(name, "AbortError", "unexpected rejection: {message}");
        assert_eq!(probe.connects.get(), 0, "audio comes first: the cancelled connect must never dial");
        assert!(!session.has_transport());
        assert!(!session.has_engine());
        assert_eq!(session.get_audio_state(), "stopped");
        assert_eq!(session.get_state(), "idle");
        assert_log(&log, &["connecting", "disconnecting", "idle"], "around a gated capture abort");

        // A later connect is admitted while the abandoned bring-up is still
        // blocked on the gate.
        session
            .connect_with_test_transport(true, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("connect after aborting a gated capture should succeed");
        assert_eq!(session.get_state(), "connected");

        // Release the abandoned bring-up so it can observe cancel and close
        // its AudioContext without leaking across the suite.
        let _ = finish_capture.call0(&JsValue::NULL);
        crate::test_support::sleep_ms(50).await;

        session.disconnect().await;
        assert_eq!(session.get_state(), "idle");
    }

    /// Audio starts before the dial, so a disconnect that lands while the hub
    /// is being dialed finds an engine the connect started: it must be stopped
    /// again, with the transport closed, and nothing published.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_disconnect_during_the_dial_stops_the_audio_the_connect_started() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);

        let (dial_gate, _never_opened) = gate();
        let transport = MockTransport::new().with_connect_gate(dial_gate);
        let probe = transport.probe();
        let connect = spawn_connect(&session, true, transport, CONNECT_TIMEOUT_MS);
        assert!(wait_until(|| probe.connects.get() == 1, 3000, 5).await, "the dial never started");
        assert_eq!(session.get_audio_state(), "running", "audio must be up before the hub is dialed");

        session.disconnect().await;
        let (name, message) = rejection(settled(&connect, "cancelled connect").await);
        assert_eq!(name, "AbortError", "unexpected rejection: {message}");
        assert_eq!(probe.closes.get(), 1, "the cancelled connect must close its transport");
        assert!(!session.has_transport());
        assert!(!session.has_engine(), "the cancelled connect must stop the engine it started");
        assert_eq!(session.get_audio_state(), "stopped");
        assert_eq!(session.get_state(), "idle");
        assert_log(&log, &["connecting", "disconnecting", "idle"], "around a cancelled dial");
    }

    /// Regression (PR 121 review): `disconnectNetwork()` cancelling a dial
    /// cancels the connect but leaves the audio it started running, as it does
    /// for a connect that completed; the audio is then the caller's to stop.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_disconnect_network_during_the_dial_leaves_the_connect_audio_running() {
        let params = AudioParams::default();
        let session = new_session(&params);

        let (dial_gate, _never_opened) = gate();
        let transport = MockTransport::new().with_connect_gate(dial_gate);
        let probe = transport.probe();
        let connect = spawn_connect(&session, true, transport, CONNECT_TIMEOUT_MS);
        assert!(wait_until(|| probe.connects.get() == 1, 3000, 5).await, "the dial never started");

        session.disconnect_network().await;
        let (name, message) = rejection(settled(&connect, "cancelled connect").await);
        assert_eq!(name, "AbortError", "unexpected rejection: {message}");
        assert_eq!(probe.closes.get(), 1, "the cancelled connect must close its transport");
        assert_eq!(session.get_state(), "idle");
        assert_eq!(session.get_audio_state(), "running", "disconnectNetwork stopped the connect's audio");
        session.stop_audio().await;
    }

    /// Regression (PR 121 review): `stopAudio` during the dial takes the
    /// connect's audio out of its hands, so audio the caller starts afterwards
    /// is theirs, and neither the connect completing nor `disconnect()` stops it.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_audio_restarted_during_the_dial_is_not_owned_by_the_connect() {
        let params = AudioParams::default();
        let session = new_session(&params);

        let (dial_gate, open_dial) = gate();
        let transport = MockTransport::new().with_connect_gate(dial_gate);
        let probe = transport.probe();
        let connect = spawn_connect(&session, true, transport, CONNECT_TIMEOUT_MS);
        assert!(wait_until(|| probe.connects.get() == 1, 3000, 5).await, "the dial never started");

        session.stop_audio().await;
        start_audio(&session).await.expect("the caller restarts audio");
        let _ = open_dial.call0(&JsValue::NULL);
        settled(&connect, "connect").await.expect("the connect completes");

        session.disconnect().await;
        assert_eq!(session.get_audio_state(), "running", "disconnect stopped audio the caller started");
        session.stop_audio().await;
    }

    /// Regression (PR 121 review): a `connect()` issued while a `startAudio()`
    /// is in flight adopts that start instead of failing with "already
    /// starting", and the audio stays the caller's: `disconnect()` leaves it
    /// running.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_connect_adopts_an_audio_start_in_flight() {
        let params = AudioParams::default();
        let session = new_session(&params);

        let (capture_gate, finish_capture) = gate();
        session.set_capture_gate(capture_gate);
        let pending = {
            let session = session.clone();
            spawn(async move { start_audio(&session).await })
        };
        assert!(wait_until(|| session.is_capture_waiting(), 3000, 5).await, "bring-up never reached the gate");

        let connect = spawn_connect(&session, true, MockTransport::new(), CONNECT_TIMEOUT_MS);
        crate::test_support::sleep_ms(50).await;
        assert!(connect.borrow().is_none(), "the connect must wait for the start, not fail or race it");

        let _ = finish_capture.call0(&JsValue::NULL);
        settled(&pending, "start").await.expect("the start completes");
        settled(&connect, "connect").await.expect("the connect adopts the start");

        session.disconnect().await;
        assert_eq!(session.get_audio_state(), "running", "disconnect stopped audio the caller started");
        session.stop_audio().await;
    }

    /// Regression (PR 121 review): a `disconnect()` that arrives while a
    /// `disconnectNetwork()` teardown is in flight was still asked to stop the
    /// audio the connect started, so that audio stops.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_disconnect_joining_a_network_disconnect_still_stops_connect_audio() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (close_gate, finish_close) = gate();
        session
            .connect_with_test_transport(true, Box::new(MockTransport::new().with_close_gate(close_gate)), CONNECT_TIMEOUT_MS)
            .await
            .expect("connect should succeed");
        assert_eq!(session.get_audio_state(), "running");

        let network = {
            let session = session.clone();
            spawn(async move { session.disconnect_network().await })
        };
        crate::test_support::sleep_ms(30).await;
        let full = {
            let session = session.clone();
            spawn(async move { session.disconnect().await })
        };
        crate::test_support::sleep_ms(30).await;
        let _ = finish_close.call0(&JsValue::NULL);
        settled(&network, "disconnectNetwork").await;
        settled(&full, "disconnect").await;

        assert_eq!(session.get_state(), "idle");
        assert_eq!(session.get_audio_state(), "stopped", "disconnect() was asked to stop the connect's audio");
    }

    /// A transport failure reported after the dial succeeded but before the
    /// connection is published (while audio is starting) must fail the
    /// connect rather than be dropped: otherwise the session would report
    /// `connected` over a dead transport. The connect releases what it created
    /// and rejects, leaving the session `idle`.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_connect_fails_when_the_transport_fails_during_bring_up() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);

        let transport = MockTransport::new();
        let probe = transport.probe();
        let reporter = probe.clone();
        probe.on_connected(move || reporter.report(TransportState::Failed));

        let result = session.connect_with_test_transport(true, Box::new(transport), CONNECT_TIMEOUT_MS).await;
        let (name, message) = rejection(result);
        assert_eq!(name, "Error", "a failed bring-up is a failure, not a cancellation: {message}");
        assert_eq!(probe.closes.get(), 1, "the failed connect must close its transport");
        assert!(!session.has_transport());
        assert!(!session.has_engine(), "the failed connect must not publish its engine");
        assert_eq!(session.get_state(), "idle");
        assert_log(&log, &["connecting", "idle"], "around a failed bring-up");
    }

    /// A dial that outlasts the connect timeout is abandoned: the connect
    /// rejects with a timeout, closes the transport, and the session is
    /// reconnectable.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_connect_times_out_a_stalled_dial() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);

        let (dial_gate, _never_opened) = gate();
        let transport = MockTransport::new().with_connect_gate(dial_gate);
        let probe = transport.probe();
        let result = session.connect_with_test_transport(true, Box::new(transport), 20).await;

        let (name, message) = rejection(result);
        assert_eq!(name, "TimeoutError", "unexpected rejection: {message}");
        assert_eq!(probe.closes.get(), 1, "a timed-out connect must close its transport");
        assert_eq!(session.get_state(), "idle");
        assert_log(&log, &["connecting", "idle"], "around a timed-out dial");
    }

    /// Connect is admitted only from `idle`. A second connect while one is
    /// dialing, and a connect while a failed connection is still held (state
    /// `error`), both reject without ever dialing their transport. Once
    /// `disconnect()` clears the error, connecting works again, and a late
    /// failure report from the torn-down transport cannot fail the new
    /// connection.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_connect_is_admitted_only_from_idle() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);

        // Connect while connecting.
        let (dial_gate, finish_dial) = gate();
        let first = MockTransport::new().with_connect_gate(dial_gate);
        let first_probe = first.probe();
        let first_connect = spawn_connect(&session, true, first, CONNECT_TIMEOUT_MS);
        assert!(wait_until(|| first_probe.connects.get() == 1, 3000, 5).await, "the dial never started");

        let overlapping = MockTransport::new();
        let overlapping_probe = overlapping.probe();
        let (name, message) = rejection(
            session.connect_with_test_transport(true, Box::new(overlapping), CONNECT_TIMEOUT_MS).await,
        );
        assert_eq!(name, "Error", "a rejected connect is a failure, not a cancellation");
        assert!(message.contains("already in progress"), "unexpected rejection: {message}");
        assert_eq!(overlapping_probe.connects.get(), 0, "an overlapping connect must not dial");

        let _ = finish_dial.call0(&JsValue::NULL);
        settled(&first_connect, "first connect").await.expect("the first connect should succeed");
        assert_eq!(session.get_state(), "connected");

        // Connect while in error.
        first_probe.report(TransportState::Failed);
        assert_eq!(session.get_state(), "error");
        let from_error = MockTransport::new();
        let from_error_probe = from_error.probe();
        let (_, message) = rejection(
            session.connect_with_test_transport(true, Box::new(from_error), CONNECT_TIMEOUT_MS).await,
        );
        assert!(message.contains("disconnect()"), "unexpected rejection: {message}");
        assert_eq!(from_error_probe.connects.get(), 0, "a connect from error must not dial");
        assert_eq!(session.get_state(), "error");

        // Recovery, and isolation from the old transport.
        session.disconnect().await;
        session
            .connect_with_test_transport(true, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("connect after clearing the error should succeed");
        first_probe.report(TransportState::Failed);
        assert_eq!(session.get_state(), "connected", "a stale failure report must be ignored");

        session.disconnect().await;
        assert_log(
            &log,
            &[
                "connecting", "connected", "error", "disconnecting", "idle",
                "connecting", "connected", "disconnecting", "idle",
            ],
            "across rejected connects",
        );
    }

    /// Teardown is serialized inside the session: while a disconnect is
    /// closing the transport, a second disconnect waits on it rather than
    /// starting another teardown, and a connect waits for it to finish
    /// before dialing (so it can never race the regulator reset).
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_teardown_coalesces_and_holds_off_the_next_connect() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);

        let (close_gate, finish_close) = gate();
        let transport = MockTransport::new().with_close_gate(close_gate);
        let probe = transport.probe();
        session
            .connect_with_test_transport(true, Box::new(transport), CONNECT_TIMEOUT_MS)
            .await
            .expect("mock connect should succeed");

        let first_disconnect = spawn_disconnect(&session);
        assert!(wait_until(|| probe.closes.get() == 1, 1000, 1).await, "teardown never reached close");
        let second_disconnect = spawn_disconnect(&session);
        let next = MockTransport::new();
        let next_probe = next.probe();
        let reconnect = spawn_connect(&session, true, next, CONNECT_TIMEOUT_MS);

        crate::test_support::sleep_ms(20).await;
        assert_eq!(session.get_state(), "disconnecting");
        assert_eq!(probe.closes.get(), 1, "a second disconnect must not start another teardown");
        assert!(second_disconnect.borrow().is_none(), "the second disconnect must wait for the first");
        assert_eq!(next_probe.connects.get(), 0, "the next connect must wait for teardown");

        let _ = finish_close.call0(&JsValue::NULL);
        settled(&first_disconnect, "first disconnect").await;
        settled(&second_disconnect, "second disconnect").await;
        settled(&reconnect, "reconnect").await.expect("the held-off connect should succeed");
        assert_eq!(probe.closes.get(), 1);
        assert_eq!(session.get_state(), "connected");

        session.disconnect().await;
        assert_log(
            &log,
            &[
                "connecting", "connected", "disconnecting", "idle",
                "connecting", "connected", "disconnecting", "idle",
            ],
            "across a coalesced teardown",
        );
    }

    /// A connect with an empty host must reject rather than report a false
    /// `connected`, and must leave the session `idle` (reconnectable) without
    /// any disconnect.
    ///
    /// Uses the default WebRTC transport: building the signaling `WebSocket`
    /// against the resulting invalid `wss://:PORT/webrtc` URL fails without any
    /// live server.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_connect_invalid_host_fails_back_to_idle() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);

        // Default transport is WebRTC; an empty host yields an invalid
        // signaling URL that fails fast, with no live hub involved.
        let result = session
            .connect(
                String::new(),
                None,
                None,
                false,
                false,
                false,
                None,
            )
            .await;

        assert!(
            result.is_err(),
            "connect to an empty host must return Err, got {result:?}"
        );
        assert_eq!(session.get_state(), "idle");
        assert!(!session.has_transport(), "a failed connect must not publish a transport");
        assert_log(&log, &["connecting", "idle"], "around a failed connect");
    }

    /// With no engine yet created (fresh session, never connected), the
    /// AudioContext-backed controls must take their no-engine branch:
    /// `is_audio_suspended` returns `false` and `resume_audio` resolves `Ok`
    /// without touching any `AudioContext`.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_audio_controls_no_engine_branch() {
        let params = AudioParams::default();
        let session = new_session(&params);

        assert!(
            !session.is_audio_suspended(),
            "with no engine, is_audio_suspended must report false"
        );
        session
            .resume_audio()
            .await
            .expect("resume_audio with no engine must resolve Ok");
    }

    /// Dropping a connected session must not emit `"error"` when the deferred
    /// teardown closes a WebRTC-typed transport. `MockTransport` reports
    /// `TransportType::WebRTC` and notifies `"closed"` from `close`, matching
    /// the real WebRTC path that would otherwise reach `failure_bridge` after
    /// free (WEB-68).
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_drop_does_not_emit_error_from_webrtc_close() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);

        let (close_gate, finish_close) = gate();
        let transport = MockTransport::new().with_close_gate(close_gate);
        let probe = transport.probe();
        session
            .connect_with_test_transport(true, Box::new(transport), CONNECT_TIMEOUT_MS)
            .await
            .expect("mock connect should succeed");
        assert_log(&log, &["connecting", "connected"], "after connect");

        // Release the sole `Rc` so `Drop` runs and spawns the deferred close.
        drop(session);
        assert!(
            wait_until(|| probe.closes.get() == 1, 2000, 5).await,
            "Drop's teardown task must reach transport.close"
        );
        // `"closed"` was already reported synchronously at the start of close
        // (before the gate). Opening the gate lets the task finish freeing
        // buffers / stopping the engine.
        let _ = finish_close.call0(&JsValue::NULL);
        crate::test_support::sleep_ms(50).await;

        assert!(
            !log.borrow().iter().any(|s| s == "error"),
            "Drop must not emit error from WebRTC close teardown, got {:?}",
            *log.borrow()
        );
        assert_log(&log, &["connecting", "connected"], "after Drop teardown");
    }

    /// A reconnect must start from a fresh regulator: the second peer's channel
    /// count and fpp are adopted, not refused against the first peer's. This
    /// catches a regulator carried across connections, which is the bug the
    /// old `Regulator::reset` existed to prevent.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_reconnect_adopts_the_second_peers_shape() {
        let params = AudioParams::default();
        let session = new_session(&params);

        session
            .connect_with_test_transport(true, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("first connect should succeed");
        assert_eq!(session.peer_shape(), Some((2, 128, false)), "before any packet: the construction geometry");
        session.push_peer_packet(0, 2, 128);
        assert_eq!(session.peer_shape(), Some((2, 128, true)));
        session.disconnect().await;
        assert_eq!(session.peer_shape(), None, "no connection, no regulator");

        session
            .connect_with_test_transport(true, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("reconnect should succeed");
        assert_eq!(session.peer_shape(), Some((2, 128, false)), "a reconnect starts from a fresh regulator");
        session.push_peer_packet(500, 1, 256);
        assert_eq!(
            session.peer_shape(),
            Some((1, 256, true)),
            "the second peer's channel count and fpp must be adopted"
        );
        session.disconnect().await;
    }

    /// Retired buffers are freed only when the replaced node's `"stopped"`
    /// acknowledgement arrives. A set no acknowledgement ever came for (as a
    /// close that timed out leaves) survives a *different* context's fulfilled
    /// close, which proves nothing about its worklet.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn retired_buffers_are_freed_by_the_stopped_acknowledgement() {
        let params = AudioParams::default();
        let session = new_session(&params);

        let (stopped, acknowledge) = gate();
        session.retire_until_stopped(StreamBuffers::new(1, 2, 128), vec![stopped], None);
        crate::test_support::sleep_ms(20).await;
        assert_eq!(session.retired_count(), 1, "buffers must stay alive until the node acknowledges");

        let _ = acknowledge.call0(&JsValue::NULL);
        assert!(
            wait_until(|| session.retired_count() == 0, 1000, 5).await,
            "the acknowledgement must free the retired buffers"
        );

        // Never acknowledged (a suspended context delivers no port messages):
        // kept, even across another connection's fulfilled close; a late
        // acknowledgement then frees it.
        let (never, late) = gate();
        session.retire_until_stopped(StreamBuffers::new(1, 2, 128), vec![never], None);
        session
            .connect_with_test_transport(true, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("connect should succeed");
        assert_eq!(session.retired_count(), 1);
        session.disconnect().await;
        assert_eq!(
            session.retired_count(),
            1,
            "a fulfilled close frees only its own connection's buffers, not an earlier set"
        );
        let _ = late.call0(&JsValue::NULL);
        assert!(wait_until(|| session.retired_count() == 0, 1000, 5).await);
    }
    /// The bug the interval clock exists for: a connection made with no audio
    /// has no worklet to wake the audio-callback loop, so without a clock of its
    /// own a WebRTC transport would never be ticked (it could not even
    /// receive). Audio then starting hands ticking to the audio callback, and
    /// stopping hands it back, with the connection up throughout.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_network_without_audio_still_ticks_the_transport() {
        let params = AudioParams::default();
        let session = new_session(&params);

        let transport = MockTransport::new();
        let probe = transport.probe();
        session
            .connect_with_test_transport(false, Box::new(transport), CONNECT_TIMEOUT_MS)
            .await
            .expect("a network-only connect should succeed");
        assert_eq!(session.get_state(), "connected");
        assert_eq!(session.get_audio_state(), "stopped");
        assert!(!session.has_engine(), "connect() must not start audio");
        assert_eq!(session.tick_kind(), TickKind::Interval);
        assert!(
            wait_until(|| probe.ticks.get() > 0, 2000, 5).await,
            "a connection with no audio must still be ticked"
        );

        // Audio joins: the node is attached to the connection and the audio
        // callback takes over ticking.
        start_audio(&session).await.expect("audio should start on a live connection");
        assert_eq!(session.engine_attached(), session.connection_id());
        assert!(session.engine_attached().is_some());
        assert_eq!(session.tick_kind(), TickKind::Atomics);

        // Audio leaves: the connection stays up and falls back to the interval.
        session.stop_audio().await;
        assert_eq!(session.get_state(), "connected", "stopping audio must not touch the connection");
        assert_eq!(session.engine_attached(), None);
        assert_eq!(session.tick_kind(), TickKind::Interval);
        let ticks = probe.ticks.get();
        assert!(wait_until(|| probe.ticks.get() > ticks, 2000, 5).await, "ticking must continue without audio");

        session.disconnect().await;
        assert_eq!(session.tick_kind(), TickKind::None);
        assert_eq!(session.retired_count(), 0, "no engine was reading the buffers, so they are freed at once");
    }

    /// Audio needs no network: it runs alone, a connection made later is
    /// attached to it, and a network-only `disconnect()` leaves it running,
    /// with the connection's buffers kept until the replaced node says it has
    /// let go of them.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_audio_outlives_the_connection() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);

        start_audio(&session).await.expect("audio should start with no connection");
        assert_eq!(session.get_audio_state(), "running");
        assert_eq!(session.get_state(), "idle", "audio alone is not a connection");
        assert!(session.has_engine());
        assert_eq!(session.engine_attached(), None);
        assert_eq!(session.connection_id(), None);
        assert!(start_audio(&session).await.is_err(), "a second start must be refused");

        session
            .connect_with_test_transport(false, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("connecting with audio running should succeed");
        assert_eq!(session.engine_attached(), session.connection_id());
        assert!(session.engine_attached().is_some(), "the running engine must be attached to the connection");
        assert_eq!(session.tick_kind(), TickKind::Atomics);

        session.disconnect().await;
        assert_eq!(session.get_state(), "idle");
        assert_eq!(session.get_audio_state(), "running", "audio the caller started survives disconnect()");
        assert!(session.has_engine());
        assert_eq!(session.engine_attached(), None, "the node must let go of the ended connection's buffers");
        assert_eq!(session.tick_kind(), TickKind::None);
        assert!(
            wait_until(|| session.retired_count() == 0, 2000, 5).await,
            "the replaced node's acknowledgement must free the connection's buffers"
        );
        assert_log(&log, &["connecting", "connected", "disconnecting", "idle"], "around a network-only cycle");

        // A second connection attaches to the same engine.
        session
            .connect_with_test_transport(false, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("reconnecting with audio running should succeed");
        assert_eq!(session.engine_attached(), session.connection_id());
        session.disconnect().await;

        session.stop_audio().await;
        assert_eq!(session.get_audio_state(), "stopped");
        assert!(!session.has_engine());
        assert_eq!(session.retired_count(), 0);
    }

    /// A disconnect-then-reconnect with audio running adopts the second peer's
    /// channel count and fpp: each connection's regulator is its own, so a
    /// stale one carried across connections (what `Regulator::reset` used to
    /// guard against) cannot refuse the second peer.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_reconnect_with_audio_running_adopts_the_second_peers_shape() {
        let params = AudioParams::default();
        let session = new_session(&params);
        start_audio(&session).await.expect("audio should start");

        session
            .connect_with_test_transport(false, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("first connect should succeed");
        session.push_peer_packet(0, 2, 128);
        assert_eq!(session.peer_shape(), Some((2, 128, true)));
        session.disconnect().await;

        session
            .connect_with_test_transport(false, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("reconnect should succeed");
        assert_eq!(session.peer_shape(), Some((2, 128, false)), "a reconnect starts from a fresh regulator");
        session.push_peer_packet(500, 1, 256);
        assert_eq!(session.peer_shape(), Some((1, 256, true)));

        session.disconnect().await;
        session.stop_audio().await;
    }

    /// `connect()` owns the audio it started and `disconnect()` stops
    /// it, so the demo (which only knows those two calls) releases the mic. Audio
    /// the caller already had running is not the connect's to stop.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_connect_owns_only_the_audio_it_started() {
        let params = AudioParams::default();
        let session = new_session(&params);

        session
            .connect_with_test_transport(true, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("connect should succeed");
        assert_eq!(session.get_audio_state(), "running");
        session.disconnect().await;
        assert_eq!(session.get_audio_state(), "stopped", "the connect started this audio, so it stops with it");
        assert!(!session.has_engine());

        start_audio(&session).await.expect("audio should start");
        session
            .connect_with_test_transport(true, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("connect with audio already running should succeed");
        session.disconnect().await;
        assert_eq!(session.get_audio_state(), "running", "audio the caller started is not the connect's to stop");

        // `disconnectNetwork()` ends the connection but leaves even a connect's
        // audio running, and hands it to the caller: a later `disconnect()` is
        // a no-op for audio.
        session
            .connect_with_test_transport(true, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("connect should succeed");
        session.disconnect_network().await;
        assert_eq!(session.get_state(), "idle");
        assert_eq!(session.get_audio_state(), "running", "disconnectNetwork must leave audio running");
        session.disconnect().await;
        assert_eq!(session.get_audio_state(), "running", "the audio is the caller's after disconnectNetwork");
        session.stop_audio().await;

        // An explicit stop takes audio out of a connect's hands: the
        // connection stays up and a later disconnect leaves audio alone.
        session
            .connect_with_test_transport(false, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("network connect should succeed");
        session.stop_audio().await;
        assert_eq!(session.get_state(), "connected");
        session.disconnect().await;
        assert_eq!(session.get_audio_state(), "stopped");
    }

    /// `stopAudio()` cancels a start stuck on a permission prompt promptly: the
    /// start rejects with `AbortError`, nothing is left running, and audio can
    /// start again while the abandoned bring-up is still blocked.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_stop_audio_cancels_a_pending_start() {
        let params = AudioParams::default();
        let session = new_session(&params);

        let (capture_gate, finish_capture) = gate();
        session.set_capture_gate(capture_gate);
        let pending = {
            let session = session.clone();
            spawn(async move { start_audio(&session).await })
        };
        assert!(
            wait_until(|| session.is_capture_waiting(), 3000, 5).await,
            "bring-up never reached the capture gate"
        );
        assert_eq!(session.get_audio_state(), "stopped", "audio is not running until the start completes");
        assert!(
            session.set_output_device(None).await.is_err(),
            "the output device cannot change while audio is starting"
        );

        let started = js_sys::Date::now();
        session.stop_audio().await;
        assert!(js_sys::Date::now() - started < 500.0, "stopAudio must not wait for a stuck capture step");
        let (name, message) = rejection(settled(&pending, "cancelled start").await);
        assert_eq!(name, "AbortError", "unexpected rejection: {message}");
        assert!(!session.has_engine());
        assert_eq!(session.get_audio_state(), "stopped");

        start_audio(&session).await.expect("audio can start again after a cancelled start");
        let _ = finish_capture.call0(&JsValue::NULL);
        crate::test_support::sleep_ms(50).await;
        session.stop_audio().await;
    }

    /// Regression (PR 121 review): a start that has been handed a connection's
    /// buffers but has not published its engine yet is still a reader of them,
    /// so ending that connection retires them against the starting engine
    /// instead of freeing them under the node about to be built. The engine's
    /// close then frees them.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_disconnect_during_an_audio_start_keeps_the_buffers_for_the_engine() {
        let params = AudioParams::default();
        let session = new_session(&params);
        session
            .connect_with_test_transport(false, Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("network connect should succeed");

        let (capture_gate, finish_capture) = gate();
        session.set_capture_gate(capture_gate);
        let pending = {
            let session = session.clone();
            spawn(async move { start_audio(&session).await })
        };
        assert!(
            wait_until(|| session.is_capture_waiting(), 3000, 5).await,
            "bring-up never reached the capture gate"
        );

        session.disconnect_network().await;
        assert_eq!(
            session.retired_count(),
            1,
            "the starting engine was handed these buffers: they must not be freed before it closes"
        );

        let _ = finish_capture.call0(&JsValue::NULL);
        settled(&pending, "start").await.expect("the start completes");
        assert_eq!(session.engine_attached(), None, "the engine must end up reading no connection");
        session.stop_audio().await;
        assert_eq!(session.retired_count(), 0, "the engine's fulfilled close frees what was retired against it");
    }
}
