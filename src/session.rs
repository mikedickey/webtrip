//! WebTrip Session Manager
//!
//! High-level session management that coordinates all components:
//! - AudioEngine (capture/playback via AudioWorklet)
//! - Transport (abstraction for network layer - WebRTC or WebTransport)
//! - RingBuffer (send path: worklet -> network)
//! - JitterBuffer (receive path: network -> worklet)
//!
//! ## Connection Flow
//!
//! ```text
//! 1. connect_to_studio(server_host, port, ...)
//!    └─> Waits out any disconnect still tearing down
//!    └─> Admitted only from Idle (see `crate::lifecycle`)
//!    └─> Dials the hub over the selected transport; WebRTC signaling
//!        (WebSocket, SDP offer/answer, ICE, data channel) happens inside
//!        WebRtcTransport.connect_to_hub()
//!
//! 2. Once the transport is connected
//!    └─> AudioEngine created and capture started
//!    └─> Transport and engine published to the session → Connected
//!    └─> Network loop begins
//!
//! 3. Audio streaming
//!    └─> tick() loop sends/receives packets via Transport trait
//! ```
//!
//! `disconnect()` may be called at any point: it cancels a connect in flight,
//! waits for that connect to release what it created, then tears down the
//! published connection. Every state change is reported through
//! `set_on_state_change` and readable with `getState()`.

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::mem::ManuallyDrop;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::Poll;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::audio::audio_callback_loop::AudioCallbackLoop;
use crate::audio::engine::{resume_context, route_output_sink, AudioEngine};
use crate::audio::params::AudioParams;
use crate::audio::protocol::MAX_CHANNELS;
use crate::audio::regulator::{Regulator, RegulatorStats};
use crate::audio::ring_buffer::RingBuffer;
use crate::audio::shared_ptr::SharedPtr;
use crate::audio::transport::{AudioBufferConfig, Transport, TransportType};
use crate::audio::webrtc::{TransportConfig as WebRtcConfig, WebRtcTransport};
use crate::audio::webtransport::{is_webtransport_available, WebTransportImpl};
use crate::lifecycle::{ConnectRejected, DisconnectStart, Lifecycle};

pub use crate::lifecycle::SessionState;

/// Default signaling port (same as JackTrip TCP port)
pub const DEFAULT_SIGNALING_PORT: u16 = 4464;

/// How long `connect_to_studio` waits for the transport to connect before
/// giving up and rejecting.
pub const CONNECT_TIMEOUT_MS: i32 = 45_000;

/// Session statistics
#[wasm_bindgen]
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
    /// that changed mid-stream, or a size mismatch)
    pub regulator_packets_rejected: u64,
}

#[wasm_bindgen]
impl SessionStats {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }
}


// ============================================================================
// Pure decision helpers (no browser APIs — fully testable with `cargo test`)
// ============================================================================

/// Returns `true` when `channels` is within the supported range [1, `MAX_CHANNELS`].
pub(crate) fn is_valid_channel_count(channels: u8) -> bool {
    channels >= 1 && channels <= MAX_CHANNELS
}

/// Whether a transport-level state string reports that the connection failed.
///
/// Only failures cross from the transport into the session: the session
/// drives `connecting`/`connected` itself, so a transport's own (possibly
/// late) reports of those are dropped.
///
/// The two transports deliberately differ on `"disconnected"`:
///
/// * **WebRTC** reports `"disconnected"`/`"closed"` only when the peer
///   connection or data channel drops, which from the session's point of view
///   is a failure.
/// * **WebTransport** posts `"disconnected"` from the worker after a
///   *graceful*, user-initiated close (once the JackTrip exit packets have been
///   flushed); genuine failures arrive separately as `"failed"`. Treating
///   `"disconnected"` as a failure there flips the UI from "Not Connected" to
///   "Connection Error" ~20 ms after the user clicks Disconnect.
pub(crate) fn transport_state_is_failure(transport_type: TransportType, transport_state: &str) -> bool {
    match (transport_type, transport_state) {
        (_, "failed") => true,
        (TransportType::WebRTC, "disconnected" | "closed") => true,
        _ => false,
    }
}

/// Construct a [`SessionStats`] from its raw component values.
///
/// This is the single authoritative mapping point; [`WebTripSession::get_stats`]
/// delegates here so the aggregation logic can be tested without constructing the
/// full session (which requires a live `AudioEngine` and `Transport`).
pub(crate) fn build_session_stats(
    reg_stats: RegulatorStats,
    regulator_depth: u32,
    regulator_latency_ms: f32,
    regulator_initialized: bool,
    send_buffer_available: u32,
    ring_buffer_writes: u64,
    ring_buffer_samples_written: u64,
    ring_buffer_overruns: u64,
) -> SessionStats {
    SessionStats {
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

// ============================================================================
// Callback-boundary plumbing
// ============================================================================

/// The published transport, shared with the audio-callback tick closure.
type TransportSlot = Rc<RefCell<Option<Box<dyn Transport>>>>;

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

/// An `Error` with the given `name`. A connect rejects with the DOM's
/// conventional names for an aborted (`"AbortError"`) or timed-out
/// (`"TimeoutError"`) operation, so a caller can tell a cancellation it caused
/// from a failure (a cross-language contract, relied on by the demo).
fn named_error(name: &str, message: &str) -> JsValue {
    let error = js_sys::Error::new(message);
    error.set_name(name);
    error.into()
}

fn cancelled_error() -> JsValue {
    named_error("AbortError", "Connection cancelled by disconnect()")
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
        let (promise, resolve, _reject) = crate::audio::make_promise();
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

/// A promise that resolves with an error value when either the returned
/// function is called with one (cancellation) or `timeout_ms` elapses, plus the
/// timer handle to clear once it is no longer needed.
fn abort_signal(timeout_ms: i32) -> Result<(js_sys::Promise, js_sys::Function, i32), JsValue> {
    let (promise, abort, _reject) = crate::audio::make_promise();
    let window = web_sys::window().ok_or_else(|| js_error("No window available"))?;
    let timed_out = named_error("TimeoutError", &format!("Connection timed out after {timeout_ms} ms"));
    let timer = window.set_timeout_with_callback_and_timeout_and_arguments_1(&abort, timeout_ms, &timed_out)?;
    Ok((promise, abort, timer))
}

/// Await `dial` unless `abort` settles first, in which case the dial future is
/// dropped (abandoning it; the caller closes the transport) and the abort's
/// value becomes the error.
async fn dial_unless_aborted(
    mut dial: Pin<Box<dyn Future<Output = Result<(), JsValue>> + '_>>,
    abort: js_sys::Promise,
) -> Result<(), JsValue> {
    let mut abort = JsFuture::from(abort);
    std::future::poll_fn(|cx| {
        if let Poll::Ready(result) = dial.as_mut().poll(cx) {
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
                return Err(js_error(
                    "WebTransport is not supported in this browser. Please use Chrome 97+ or Edge 97+.",
                ));
            }
            Ok(Box::new(WebTransportImpl::new()?))
        }
    }
}

/// Audio capture options for a connect, applied once the transport is up.
struct CaptureParams {
    device_id: Option<String>,
    auto_gain_control: bool,
    echo_cancellation: bool,
    noise_suppression: bool,
}

/// WebTrip Session - coordinates all audio and network components
///
/// Every method takes `&self`, so JavaScript may call any of them (notably
/// `disconnect()` and `getState()`) while a `connectToStudio()` promise is
/// pending: wasm-bindgen holds an async method's borrow until its promise
/// settles, and an `&mut self` borrow would make every such call throw.
///
/// The lifecycle invariants (connect only from `Idle`, a disconnect cancels a
/// connect in flight, one teardown at a time) live in the atomic
/// [`Lifecycle`]. The remaining interior mutability (`Cell`/`RefCell`/`Rc`)
/// holds browser objects and the JS callbacks wired to them. It is confined to
/// the agent that created the session: the session holds `js_sys` values and
/// raw pointers, so it is neither `Send` nor `Sync`, and its callbacks run on
/// that same agent's event loop. No `RefCell` borrow is held across an
/// `.await`.
#[wasm_bindgen]
pub struct WebTripSession {
    // Audio components
    audio_params_ptr: *const AudioParams,
    audio_engine: RefCell<Option<AudioEngine>>,

    // Owned buffers (shared with AudioWorklet and Transport via pointers).
    // `ManuallyDrop` because `Drop` must hand them to a deferred task that
    // frees them only once both consumers have quiesced (see `Drop`).
    local_to_network_buffer: ManuallyDrop<Box<RingBuffer>>,
    network_to_local_buffer: ManuallyDrop<Box<Regulator>>,

    // Network components
    transport: TransportSlot,
    transport_type: Cell<TransportType>,

    lifecycle: Arc<Lifecycle>,
    /// Settles when the connect in flight has finished (successfully or after
    /// releasing everything it created)
    connect_in_flight: RefCell<Option<js_sys::Promise>>,
    /// Aborts the in-flight connect's transport dial
    abort_dial: RefCell<Option<js_sys::Function>>,
    /// Settles when the disconnect in flight has returned the session to `Idle`
    teardown_in_flight: RefCell<Option<js_sys::Promise>>,

    // Configuration. The four channel counts are independent, set only
    // while Idle, and fixed for the life of a connection.
    buffer_size: usize,
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

    // Audio callback loop (triggers transport tick on each audio callback)
    audio_callback_loop: RefCell<Option<(AudioCallbackLoop, Closure<dyn FnMut()>)>>,
}

#[wasm_bindgen]
impl WebTripSession {
    /// Create a new session
    #[wasm_bindgen(constructor)]
    pub fn new(audio_params_ptr: *const AudioParams) -> Result<WebTripSession, JsValue> {
        let buffer_size = 128;
        let sample_rate = 48000;
        let send_channels = 1;
        let receive_channels = 2;

        // Sync the send width to AudioParams: the render thread conforms
        // captured audio to it (see `conform_interleaved_channels`).
        if !audio_params_ptr.is_null() {
            unsafe {
                (*audio_params_ptr).set_send_channels(send_channels as u32);
            }
        }

        // Create owned buffers
        let local_to_network_buffer = Box::new(RingBuffer::new());
        // Regulator with auto-adaptive tolerance and headroom (-500.0).
        // The peer's actual count is still adopted from its first packet.
        let network_to_local_buffer = Box::new(Regulator::with_params(
            receive_channels as usize,
            buffer_size,
            sample_rate,
            -500.0,
        ));

        Ok(WebTripSession {
            audio_params_ptr,
            audio_engine: RefCell::new(None),
            local_to_network_buffer: ManuallyDrop::new(local_to_network_buffer),
            network_to_local_buffer: ManuallyDrop::new(network_to_local_buffer),
            transport: Rc::new(RefCell::new(None)),
            transport_type: Cell::new(TransportType::WebRTC), // Default to WebRTC
            lifecycle: Arc::new(Lifecycle::new()),
            connect_in_flight: RefCell::new(None),
            abort_dial: RefCell::new(None),
            teardown_in_flight: RefCell::new(None),
            buffer_size,
            send_channels: Cell::new(send_channels),
            receive_channels: Cell::new(receive_channels),
            input_channels: Cell::new(2),
            output_channels: Cell::new(2),
            on_state_change: Rc::new(RefCell::new(None)),
            output_device_id: RefCell::new(None),
            audio_callback_loop: RefCell::new(None),
        })
    }

    /// Set callback for state changes. It receives the same strings as
    /// [`get_state`](Self::get_state).
    pub fn set_on_state_change(&self, callback: js_sys::Function) {
        *self.on_state_change.borrow_mut() = Some(callback);
    }

    /// The current lifecycle state: `"idle"`, `"connecting"`, `"connected"`,
    /// `"error"`, or `"disconnecting"`.
    #[wasm_bindgen(js_name = getState)]
    pub fn get_state(&self) -> String {
        self.state().as_str().to_string()
    }

    /// Get current statistics
    pub fn get_stats(&self) -> SessionStats {
        let reg_stats = self.network_to_local_buffer.stats();
        build_session_stats(
            reg_stats,
            self.network_to_local_buffer.depth(),
            self.network_to_local_buffer.latency_ms(),
            self.network_to_local_buffer.is_initialized(),
            self.local_to_network_buffer.available(),
            self.local_to_network_buffer.writes(),
            self.local_to_network_buffer.samples_written(),
            self.local_to_network_buffer.overruns(),
        )
    }

    /// Get the ring buffer flag pointer for event-driven wake-up
    ///
    /// Used with Atomics.waitAsync() in JavaScript for zero-CPU idle behavior.
    /// Returns the memory address of the atomic flag that signals data availability.
    #[wasm_bindgen(js_name = getRingBufferFlagPtr)]
    pub fn get_ring_buffer_flag_ptr(&self) -> usize {
        self.local_to_network_buffer.get_has_data_flag_ptr()
    }

    /// Set how many channels this client sends over the wire (default 1).
    ///
    /// Every channel-count setter is rejected outside `Idle` (and for counts
    /// outside [1, `MAX_CHANNELS`]): the transports and the render thread
    /// snapshot these at connect time, so a mid-connection change would
    /// desync producer and consumer on the channel-agnostic `RingBuffer`.
    ///
    /// This is each outbound packet's payload width, independent of the
    /// input device: captured audio is conformed to it on the render thread
    /// (`conform_interleaved_channels`), e.g. 2 input channels averaged into
    /// 1 sent.
    #[wasm_bindgen(js_name = setSendChannels)]
    pub fn set_send_channels(&self, channels: u8) {
        if !self.can_set_channel_count("send", channels) {
            return;
        }
        self.send_channels.set(channels);
        if !self.audio_params_ptr.is_null() {
            unsafe {
                (*self.audio_params_ptr).set_send_channels(channels as u32);
            }
        }
    }

    /// Get the configured send channel count
    #[wasm_bindgen(js_name = getSendChannels)]
    pub fn get_send_channels(&self) -> u8 {
        self.send_channels.get()
    }

    /// Set how many channels this client asks the peer to send back
    /// (default 2), advertised in every outbound packet's header. Idle-only
    /// (see [`set_send_channels`](Self::set_send_channels)).
    ///
    /// The peer decides what it actually sends: the regulator adopts the
    /// count from its first packet
    /// ([`Regulator::push`](crate::audio::regulator::Regulator::push)) and
    /// `map_to_output` maps it onto the output device.
    #[wasm_bindgen(js_name = setReceiveChannels)]
    pub fn set_receive_channels(&self, channels: u8) {
        if self.can_set_channel_count("receive", channels) {
            self.receive_channels.set(channels);
        }
    }

    /// Get the configured receive channel count
    #[wasm_bindgen(js_name = getReceiveChannels)]
    pub fn get_receive_channels(&self) -> u8 {
        self.receive_channels.get()
    }

    /// Set how many input device channels to capture (default 2): the
    /// device's first N, clamped at capture time to what the browser grants.
    /// Idle-only (see [`set_send_channels`](Self::set_send_channels)).
    #[wasm_bindgen(js_name = setInputChannels)]
    pub fn set_input_channels(&self, channels: u8) {
        if self.can_set_channel_count("input", channels) {
            self.input_channels.set(channels);
        }
    }

    /// Get the configured input device channel count
    #[wasm_bindgen(js_name = getInputChannels)]
    pub fn get_input_channels(&self) -> u8 {
        self.input_channels.get()
    }

    /// Set how many output device channels to play to (default 2): the
    /// device's first N, clamped at capture time to the destination's
    /// `maxChannelCount`. Idle-only (see
    /// [`set_send_channels`](Self::set_send_channels)).
    #[wasm_bindgen(js_name = setOutputChannels)]
    pub fn set_output_channels(&self, channels: u8) {
        if self.can_set_channel_count("output", channels) {
            self.output_channels.set(channels);
        }
    }

    /// Get the configured output device channel count
    #[wasm_bindgen(js_name = getOutputChannels)]
    pub fn get_output_channels(&self) -> u8 {
        self.output_channels.get()
    }

    /// Set the transport type to use for connections
    /// Must be called before connecting
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

    /// Check if WebTransport is available in this browser
    ///
    /// WebTransport is supported in Chrome 97+, Edge 97+.
    /// Safari and Firefox do not yet support WebTransport.
    #[wasm_bindgen(js_name = isWebTransportAvailable)]
    pub fn is_webtransport_available_check() -> bool {
        is_webtransport_available()
    }

    /// Set the output audio device for playback
    ///
    /// This can be called at any time to change the output device,
    /// even while audio is playing. If the audio engine hasn't been
    /// created yet, the device will be stored and applied when the
    /// engine is initialized.
    ///
    /// # Arguments
    /// * `device_id` - The device ID from the output device selector, or empty string/None for default
    #[wasm_bindgen(js_name = setOutputDevice)]
    pub async fn set_output_device(&self, device_id: Option<String>) -> Result<(), JsValue> {
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

    // ========== Studio Connection Methods ==========

    /// Connect to a JackTrip hub server
    ///
    /// This is the main entry point for connecting to a studio. It:
    /// 1. Waits for any disconnect still tearing down, then rejects unless the
    ///    session is `Idle` (call `disconnect()` first after an `"error"`)
    /// 2. Connects the selected transport (for WebRTC: WebSocket signaling,
    ///    SDP offer/answer, ICE, data channel), giving up after
    ///    `CONNECT_TIMEOUT_MS`
    /// 3. Starts audio capture, so no audio is captured before a connection
    ///    is ready
    ///
    /// A `disconnect()` while this is pending makes it release whatever it has
    /// created and reject with an `Error` named `"AbortError"`; it never
    /// reaches `"connected"` afterwards. A timed-out dial rejects with
    /// `"TimeoutError"`. Any failed connect leaves the session `"idle"`.
    ///
    /// # Arguments
    /// * `server_host` - The hub server hostname (from studio.server_host)
    /// * `port` - The signaling port (default 4464)
    /// * `device_id` - Optional input device ID
    /// * `auto_gain_control` - Enable AGC
    /// * `echo_cancellation` - Enable echo cancellation
    /// * `noise_suppression` - Enable noise suppression
    /// * `client_name` - Optional client name to send in connection request
    #[wasm_bindgen(js_name = connectToStudio)]
    pub async fn connect_to_studio(
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
            CaptureParams {
                device_id,
                auto_gain_control,
                echo_cancellation,
                noise_suppression,
            },
            CONNECT_TIMEOUT_MS,
        )
        .await
    }

    /// Disconnect from the hub server, resolving once the session is `Idle`.
    ///
    /// Safe to call at any time and any number of times: from `Idle` it is a
    /// no-op, and a call while another disconnect is running waits for that
    /// one instead of starting a second teardown. A connect in flight is
    /// cancelled first, and its own cleanup is awaited before anything else
    /// is torn down.
    ///
    /// Teardown awaits full transport shutdown before resetting the
    /// network-to-local buffer. That ordering is load-bearing: the
    /// `Regulator`'s sequence-number state is shared via a raw pointer with
    /// the transport (for WebTransport, the worker thread writes to it), so
    /// resetting before the transport has quiesced creates a race in which a
    /// late `regulator.push()` from the old connection pins `last_seq_in`
    /// high and causes the *next* connection's packets to be rejected by the
    /// wrap-distance check. Awaiting `transport.close()` establishes the
    /// happens-before ordering we need: for WebTransport the future only
    /// resolves after `worker.terminate()`; for WebRTC the teardown is
    /// fully synchronous. A connect issued meanwhile waits for this to finish.
    pub async fn disconnect(&self) {
        match self.lifecycle.begin_disconnect() {
            DisconnectStart::AlreadyIdle => return,
            DisconnectStart::AlreadyDisconnecting => {
                await_in_flight(&self.teardown_in_flight).await;
                return;
            }
            DisconnectStart::Started => {}
        }
        // Published before anything can re-enter (the state callback below),
        // so a connect issued from it finds the teardown to wait on.
        let teardown = InFlight::publish(&self.teardown_in_flight);
        emit_state(&self.on_state_change, SessionState::Disconnecting);

        // Abort a dial still in progress, then let that connect release what
        // it created (it may own a live transport or engine not yet published).
        let abort = self.abort_dial.borrow_mut().take();
        if let Some(abort) = abort {
            let _ = abort.call1(&JsValue::NULL, &cancelled_error());
        }
        await_in_flight(&self.connect_in_flight).await;

        self.stop_network_loop();
        let transport = self.transport.borrow_mut().take();
        let engine = self.audio_engine.borrow_mut().take();
        self.release(transport, engine).await;

        self.lifecycle.finish_disconnect();
        drop(teardown);
        emit_state(&self.on_state_change, SessionState::Idle);
    }

    /// Return true when the underlying AudioContext is still suspended.
    ///
    /// On iOS Safari the context may stay suspended after `connectToStudio` completes
    /// because `resume()` requires a direct user-gesture activation.  The TypeScript
    /// layer can call this after a successful connect to decide whether to show a
    /// "Tap to enable audio" prompt.
    #[wasm_bindgen(js_name = isAudioSuspended)]
    pub fn is_audio_suspended(&self) -> bool {
        self.audio_engine
            .borrow()
            .as_ref()
            .map(|e| e.is_suspended())
            .unwrap_or(false)
    }

    /// Explicitly resume the AudioContext.
    ///
    /// Call this from inside a synchronous user-gesture handler (e.g. a button `onclick`)
    /// so that iOS Safari grants the audio-output activation.
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
    /// The current lifecycle state.
    pub fn state(&self) -> SessionState {
        self.lifecycle.state()
    }

    /// Connect the session over a caller-supplied, already-constructed transport,
    /// bypassing the real per-type transport construction in `connect_to_studio`.
    ///
    /// Unit-test only. Runs the same [`connect_with`] path as
    /// `connect_to_studio` (admission, dial, `AudioEngine`/worklet/callback-loop
    /// bring-up, `Connected`) against a mock transport with no live hub. Pair
    /// with [`disconnect`] for the teardown half.
    ///
    /// [`connect_with`]: Self::connect_with
    /// [`disconnect`]: Self::disconnect
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) async fn connect_with_test_transport(
        &self,
        transport: Box<dyn Transport>,
        timeout_ms: i32,
    ) -> Result<(), JsValue> {
        let capture = CaptureParams {
            device_id: None,
            auto_gain_control: false,
            echo_cancellation: false,
            noise_suppression: false,
        };
        self.connect_with(|| Ok(transport), "test-host", DEFAULT_SIGNALING_PORT, "", capture, timeout_ms)
            .await
    }

    /// Whether a connected transport is published (test-only observer).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn has_transport(&self) -> bool {
        self.transport.borrow().is_some()
    }

    /// Whether an audio engine is published (test-only observer).
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) fn has_engine(&self) -> bool {
        self.audio_engine.borrow().is_some()
    }

    /// Admit, dial, and bring up a connection; the transport-agnostic body of
    /// `connect_to_studio`. On failure (including cancellation) everything the
    /// attempt created is released before this returns.
    async fn connect_with(
        &self,
        new_transport: impl FnOnce() -> Result<Box<dyn Transport>, JsValue>,
        server: &str,
        port: u16,
        client_name: &str,
        capture: CaptureParams,
        timeout_ms: i32,
    ) -> Result<(), JsValue> {
        let epoch = self.admit_connect().await?;
        // Published before anything can re-enter (the state callback below),
        // so a disconnect issued from it waits for this connect to settle.
        let _connect = InFlight::publish(&self.connect_in_flight);
        emit_state(&self.on_state_change, SessionState::Connecting);

        let result = match abort_signal(timeout_ms) {
            Ok((abort, abort_fn, timer)) => {
                *self.abort_dial.borrow_mut() = Some(abort_fn);
                let result = self
                    .bring_up(epoch, new_transport, server, port, client_name, capture, abort)
                    .await;
                self.abort_dial.borrow_mut().take();
                if let Some(window) = web_sys::window() {
                    window.clear_timeout_with_handle(timer);
                }
                result
            }
            Err(e) => Err(e),
        };

        if result.is_err() && self.lifecycle.abort_connect(epoch) {
            emit_state(&self.on_state_change, SessionState::Idle);
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

    /// Dial the transport, start capture, and publish both, checking after
    /// every await that no disconnect has overtaken this connect (`epoch`).
    /// Anything created is released before an `Err` is returned.
    #[allow(clippy::too_many_arguments)]
    async fn bring_up(
        &self,
        epoch: u64,
        new_transport: impl FnOnce() -> Result<Box<dyn Transport>, JsValue>,
        server: &str,
        port: u16,
        client_name: &str,
        capture: CaptureParams,
        abort: js_sys::Promise,
    ) -> Result<(), JsValue> {
        let mut transport = new_transport()?;
        let transport_type = transport.transport_type();
        transport.set_audio_buffers(self.audio_buffer_config());
        let failed_while_connecting = Rc::new(Cell::new(false));
        transport.set_on_state_change(self.failure_bridge(
            epoch,
            transport_type,
            Rc::clone(&failed_while_connecting),
        ));

        let dialed = dial_unless_aborted(transport.connect(server, port, client_name), abort).await;
        let dialed = dialed.and_then(|()| {
            if self.lifecycle.is_current(epoch) { Ok(()) } else { Err(cancelled_error()) }
        });
        if let Err(e) = dialed {
            self.release(Some(transport), None).await;
            return Err(e);
        }

        let engine = match self.start_engine(capture).await {
            Ok(engine) => engine,
            Err(e) => {
                self.release(Some(transport), None).await;
                return Err(e);
            }
        };

        // A failure reported before `Connected` cannot fail the connection
        // (there is none yet), so it fails the connect instead.
        if failed_while_connecting.get() {
            self.release(Some(transport), Some(engine)).await;
            return Err(js_error("The connection failed while it was being set up"));
        }
        if !self.lifecycle.finish_connect(epoch) {
            self.release(Some(transport), Some(engine)).await;
            return Err(cancelled_error());
        }
        // Publish and start the network loop with no await in between, so the
        // `Connected` state is never observable without them.
        *self.transport.borrow_mut() = Some(transport);
        *self.audio_engine.borrow_mut() = Some(engine);
        self.start_network_loop(transport_type);
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
    ) -> js_sys::Function {
        let lifecycle = Arc::clone(&self.lifecycle);
        let on_state_change = Rc::clone(&self.on_state_change);
        Closure::<dyn FnMut(String)>::new(move |state: String| {
            if !transport_state_is_failure(transport_type, &state) {
                return;
            }
            if lifecycle.fail(epoch) {
                emit_state(&on_state_change, SessionState::Error);
            } else {
                failed_while_connecting.set(true);
            }
        })
        // The transport keeps the function for its lifetime; leak the closure
        // so it outlives the transport.
        .into_js_value()
        .unchecked_into()
    }

    /// Quiesce and drop a connection's transport and engine, then reset the
    /// regulator for the next connection. The caller must already have
    /// stopped the network loop if it was started.
    async fn release(&self, transport: Option<Box<dyn Transport>>, engine: Option<AudioEngine>) {
        if let Some(mut transport) = transport {
            transport.close().await;
            // `transport` is dropped at end of scope; its `Drop` impl is a
            // no-op on already-closed state.
        }
        // Awaits worklet quiescence (via `AudioEngine::stop_capture`'s
        // `AudioContext.suspend`), as `Regulator::reset` requires (WEB-53 Bug 7).
        if let Some(mut engine) = engine {
            engine.stop_capture().await;
        }
        // Now that no further writes to the Regulator are possible, reset it
        // so the next connection starts from a clean slate.
        self.network_to_local_buffer.reset();
    }

    /// Raw pointers to the owned buffers, for consumers that cross agents.
    /// Both are only ever dereferenced as shared references (`SharedPtr::as_ref`);
    /// their mutable state is atomics and `UnsafeCell`.
    fn buffer_ptrs(&self) -> (*mut RingBuffer, *mut Regulator) {
        (
            std::ptr::from_ref::<RingBuffer>(&self.local_to_network_buffer).cast_mut(),
            std::ptr::from_ref::<Regulator>(&self.network_to_local_buffer).cast_mut(),
        )
    }

    /// The session's audio buffers and channel counts, as handed to a
    /// transport before it connects.
    fn audio_buffer_config(&self) -> AudioBufferConfig {
        let (local_to_network, network_to_local) = self.buffer_ptrs();
        AudioBufferConfig {
            local_to_network: SharedPtr::new(local_to_network),
            network_to_local: SharedPtr::new(network_to_local),
            buffer_size: self.buffer_size,
            send_channels: self.send_channels.get(),
            receive_channels: self.receive_channels.get(),
        }
    }

    /// Create the `AudioEngine` and start capture, stopping the engine again
    /// if capture fails to start.
    async fn start_engine(&self, capture: CaptureParams) -> Result<AudioEngine, JsValue> {
        // `AudioEngine` is a `#[wasm_bindgen]` boundary type, so it takes raw
        // pointers here and wraps them in `SharedPtr` internally when it
        // builds the `AudioProcessor`.
        let (local_to_network, network_to_local) = self.buffer_ptrs();
        let mut engine =
            AudioEngine::create_with_network(self.audio_params_ptr, local_to_network, network_to_local).await?;

        web_sys::console::log_1(&"🎙️ Starting audio capture after connection established".into());
        let output_device_id = self.output_device_id.borrow().clone();
        let started = engine
            .start_capture(
                capture.device_id,
                output_device_id,
                capture.auto_gain_control,
                capture.echo_cancellation,
                capture.noise_suppression,
                self.input_channels.get() as u32,
                self.output_channels.get() as u32,
            )
            .await;
        if let Err(e) = started {
            engine.stop_capture().await;
            return Err(e);
        }
        Ok(engine)
    }

    /// Start moving audio between the buffers and the published transport.
    fn start_network_loop(&self, transport_type: TransportType) {
        match transport_type {
            TransportType::WebTransport => {
                // WebTransport worker handles send/receive loops - no tick() needed!
                // Just enable streaming on ring buffer for the worker to read
                self.local_to_network_buffer.set_streaming(true);
            }
            TransportType::WebRTC => {
                // WebRTC needs the main-thread audio callback loop for tick()
                self.start_audio_callback_loop();
            }
        }
    }

    /// Stop the network loop started by [`start_network_loop`](Self::start_network_loop).
    fn stop_network_loop(&self) {
        if let Some((mut callback_loop, _tick)) = self.audio_callback_loop.borrow_mut().take() {
            callback_loop.stop();
        }
        self.local_to_network_buffer.set_streaming(false);
    }

    /// Start the audio callback loop
    fn start_audio_callback_loop(&self) {
        if self.audio_callback_loop.borrow().is_some() {
            return; // Already started
        }

        // Enable streaming on ring buffer
        self.local_to_network_buffer.set_streaming(true);

        // Get ring buffer flag pointer
        let flag_ptr = self.local_to_network_buffer.get_has_data_flag_ptr();

        // Create audio callback loop
        let mut callback_loop = AudioCallbackLoop::new();

        // Tick the published transport. The slot is never borrowed across an
        // await, so the borrow only fails if a tick re-enters itself; skip it then.
        let transport = Rc::clone(&self.transport);
        let tick_closure = Closure::<dyn FnMut()>::new(move || {
            if let Ok(mut slot) = transport.try_borrow_mut() {
                if let Some(transport) = slot.as_mut() {
                    transport.tick();
                }
            }
        });
        let tick_fn = tick_closure.as_ref().unchecked_ref::<js_sys::Function>().clone();

        // Try Atomics.waitAsync
        let memory = wasm_bindgen::memory();

        match callback_loop.start_with_atomics(&memory, flag_ptr, tick_fn) {
            Ok(true) => {
                *self.audio_callback_loop.borrow_mut() = Some((callback_loop, tick_closure));
                web_sys::console::debug_1(&"✅ Audio callback loop started".into());
            }
            Ok(false) | Err(_) => {
                web_sys::console::warn_1(&"⚠️ Atomics.waitAsync not available".into());
            }
        }
    }

    /// Shared guard for the channel-count setters: a count may change only
    /// while `Idle` and only within [1, `MAX_CHANNELS`]. Logs the reason
    /// (`which` names the setting) when it may not.
    fn can_set_channel_count(&self, which: &str, channels: u8) -> bool {
        if self.state() != SessionState::Idle {
            web_sys::console::warn_1(
                &format!("⚠️ Cannot change {which} channels while connected").into(),
            );
            return false;
        }
        if !is_valid_channel_count(channels) {
            web_sys::console::warn_1(
                &format!("⚠️ Invalid {which} channel count: {channels}").into(),
            );
            return false;
        }
        true
    }
}

impl Drop for WebTripSession {
    fn drop(&mut self) {
        // Drop can't await, but the worklet render thread and the WebTransport
        // worker both hold raw pointers into the buffers, and neither stops
        // synchronously (`"stop"` and `{type:"disconnect"}` are just posted
        // messages). Freeing the buffers here would let a pending `process()`
        // or worker `push()` dereference freed storage. Instead, move the
        // buffers into a task that runs the same quiescing teardown as
        // `disconnect()` and frees them only afterwards. No reset needed: the
        // buffers are being deallocated. (No connect can be in flight: it
        // borrows the session for its whole duration.)
        self.stop_network_loop();
        let transport = self.transport.borrow_mut().take();
        let engine = self.audio_engine.get_mut().take();
        // SAFETY: `Drop::drop` runs once and nothing reads the fields after.
        let buffers = unsafe {
            (
                ManuallyDrop::take(&mut self.local_to_network_buffer),
                ManuallyDrop::take(&mut self.network_to_local_buffer),
            )
        };
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(mut transport) = transport {
                transport.close().await;
            }
            if let Some(mut engine) = engine {
                engine.stop_capture().await;
            }
            drop(buffers);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::regulator::RegulatorStats;

    // -----------------------------------------------------------------------
    // Channel-count validation
    // -----------------------------------------------------------------------

    #[test]
    fn channel_count_validation_boundaries() {
        assert!(!is_valid_channel_count(0));
        assert!(is_valid_channel_count(1));
        assert!(is_valid_channel_count(8));
        assert!(!is_valid_channel_count(9));
    }

    // -----------------------------------------------------------------------
    // Transport-state → session failure mapping
    // -----------------------------------------------------------------------

    /// Full matrix over both transports and every state string either of them
    /// can emit. The rows that matter most:
    ///
    /// * WebTransport `"disconnected"` must not be a failure (regression: a
    ///   graceful user-initiated disconnect flipped the UI to "Connection
    ///   Error" ~20 ms after the exit packets were flushed).
    /// * WebRTC `"disconnected"`/`"closed"` must be a failure (the peer
    ///   connection really did drop).
    /// * `"connected"`/`"connecting"` are never forwarded: the session drives
    ///   those itself, and a late transport report must not regress the UI.
    #[test]
    fn transport_failure_reports_per_transport() {
        use TransportType::{WebRTC, WebTransport};

        let cases: &[(TransportType, &str, bool)] = &[
            (WebRTC, "failed", true),
            (WebTransport, "failed", true),
            // Divergent: a dropped peer connection vs. a graceful worker close.
            (WebRTC, "disconnected", true),
            (WebRTC, "closed", true),
            (WebTransport, "disconnected", false),
            (WebTransport, "closed", false),
            // Progress reports belong to the session, not the transport.
            (WebRTC, "connected", false),
            (WebTransport, "connected", false),
            (WebRTC, "connecting", false),
            (WebTransport, "connecting", false),
            // Anything unrecognized is dropped rather than guessed at.
            (WebRTC, "", false),
            (WebTransport, "", false),
        ];

        for &(transport, state, expected) in cases {
            assert_eq!(
                transport_state_is_failure(transport, state),
                expected,
                "{transport:?} transport state {state:?} mapped incorrectly"
            );
        }
    }

    // -----------------------------------------------------------------------
    // SessionStats aggregation
    // -----------------------------------------------------------------------

    #[test]
    fn build_session_stats_maps_all_fields() {
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

        let s = build_session_stats(
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
    // `crate::lifecycle`; these tests cover the session's use of it: that the
    // resources follow the state (a cancelled connect releases its transport
    // and never publishes, teardown is serialized and coalesced) and that the
    // state-change callback reports it in order.
    //
    // Connects run over `test_support::MockTransport` via
    // `connect_with_test_transport`, which reuses `connect_to_studio`'s whole
    // `connect_with` path without a live hub. The real WebRTC/WebTransport
    // arms' network handshakes (SDP/ICE, QUIC) still require a live JackTrip
    // hub and are covered by the integration tests; here they are only
    // exercised on their failure paths.

    #[cfg(target_arch = "wasm32")]
    use crate::test_support::{gate, recording_state_callback, wait_until, MockTransport};
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

    /// A session over `params`, which must outlive it (the session holds a raw
    /// pointer to it): declare `params` first so it is dropped last.
    #[cfg(target_arch = "wasm32")]
    fn new_session(params: &AudioParams) -> Rc<WebTripSession> {
        Rc::new(WebTripSession::new(params as *const AudioParams).expect("session construction should succeed"))
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

    /// Spawn a mock connect on `session` (see [`spawn`]).
    #[cfg(target_arch = "wasm32")]
    fn spawn_connect(
        session: &Rc<WebTripSession>,
        transport: MockTransport,
        timeout_ms: i32,
    ) -> Rc<RefCell<Option<Result<(), JsValue>>>> {
        let session = session.clone();
        spawn(async move { session.connect_with_test_transport(Box::new(transport), timeout_ms).await })
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
    /// and closed once by teardown. Along the way it pins the channel-count
    /// setters' contract: applied while `Idle`, ignored while connected, and
    /// retained across disconnect (nothing narrows them). This runs the real
    /// async/await connect path, `AudioEngine::create`, worklet bootstrap, and
    /// the `Atomics.waitAsync` audio-callback loop in the browser — only the
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
        assert_eq!(
            params.get_send_channels(),
            u32::from(configured.0),
            "setSendChannels must reach the render thread's AudioParams"
        );

        let transport = MockTransport::new();
        let probe = transport.probe();
        session
            .connect_with_test_transport(Box::new(transport), CONNECT_TIMEOUT_MS)
            .await
            .expect("mock connect should succeed");

        assert_eq!(session.get_state(), "connected");
        assert!(session.has_transport());
        assert_log(&log, &["connecting", "connected"], "after connect");

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
        assert_eq!(params.get_send_channels(), u32::from(configured.0));

        session.disconnect().await;
        assert_eq!(session.get_state(), "idle");
        assert!(!session.has_transport());
        assert_eq!(probe.closes.get(), 1, "teardown must close the transport once");
        assert_log(&log, &["connecting", "connected", "disconnecting", "idle"], "after disconnect");
        assert_eq!(
            channel_config(&session),
            configured,
            "disconnect must retain the configured channel counts"
        );
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
        let connect = spawn_connect(&session, transport, CONNECT_TIMEOUT_MS);
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

    /// A disconnect that lands after the dial succeeded but while audio is
    /// still starting cancels at the last checkpoint: the connect releases the
    /// transport and engine it holds, rejects, and never publishes either.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_disconnect_cancels_a_connect_starting_audio() {
        let params = AudioParams::default();
        let session = new_session(&params);
        let (log, _cb) = attach_state_log(&session);

        let transport = MockTransport::new();
        let probe = transport.probe();
        // Queued as the dial resolves, so it runs once the connect has moved
        // on and is awaiting the audio engine.
        let disconnect = Rc::new(RefCell::new(None));
        let (session_for_hook, disconnect_slot) = (session.clone(), disconnect.clone());
        probe.on_connected(move || {
            *disconnect_slot.borrow_mut() = Some(spawn_disconnect(&session_for_hook));
        });

        let result = session.connect_with_test_transport(Box::new(transport), CONNECT_TIMEOUT_MS).await;
        let (name, message) = rejection(result);
        assert_eq!(name, "AbortError", "unexpected rejection: {message}");
        let disconnect = disconnect.borrow_mut().take().expect("the hook ran");
        settled(&disconnect, "disconnect").await;

        assert_eq!(probe.closes.get(), 1, "the cancelled connect must close its transport");
        assert!(!session.has_transport());
        assert!(!session.has_engine(), "the cancelled connect must not publish its engine");
        assert_eq!(session.get_state(), "idle");
        assert_log(&log, &["connecting", "disconnecting", "idle"], "around a cancelled bring-up");
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
        probe.on_connected(move || reporter.report("failed"));

        let result = session.connect_with_test_transport(Box::new(transport), CONNECT_TIMEOUT_MS).await;
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
        let result = session.connect_with_test_transport(Box::new(transport), 20).await;

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
        let first_connect = spawn_connect(&session, first, CONNECT_TIMEOUT_MS);
        assert!(wait_until(|| first_probe.connects.get() == 1, 1000, 1).await);

        let overlapping = MockTransport::new();
        let overlapping_probe = overlapping.probe();
        let (name, message) = rejection(
            session.connect_with_test_transport(Box::new(overlapping), CONNECT_TIMEOUT_MS).await,
        );
        assert_eq!(name, "Error", "a rejected connect is a failure, not a cancellation");
        assert!(message.contains("already in progress"), "unexpected rejection: {message}");
        assert_eq!(overlapping_probe.connects.get(), 0, "an overlapping connect must not dial");

        let _ = finish_dial.call0(&JsValue::NULL);
        settled(&first_connect, "first connect").await.expect("the first connect should succeed");
        assert_eq!(session.get_state(), "connected");

        // Connect while in error.
        first_probe.report("failed");
        assert_eq!(session.get_state(), "error");
        let from_error = MockTransport::new();
        let from_error_probe = from_error.probe();
        let (_, message) = rejection(
            session.connect_with_test_transport(Box::new(from_error), CONNECT_TIMEOUT_MS).await,
        );
        assert!(message.contains("disconnect()"), "unexpected rejection: {message}");
        assert_eq!(from_error_probe.connects.get(), 0, "a connect from error must not dial");
        assert_eq!(session.get_state(), "error");

        // Recovery, and isolation from the old transport.
        session.disconnect().await;
        session
            .connect_with_test_transport(Box::new(MockTransport::new()), CONNECT_TIMEOUT_MS)
            .await
            .expect("connect after clearing the error should succeed");
        first_probe.report("failed");
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
            .connect_with_test_transport(Box::new(transport), CONNECT_TIMEOUT_MS)
            .await
            .expect("mock connect should succeed");

        let first_disconnect = spawn_disconnect(&session);
        assert!(wait_until(|| probe.closes.get() == 1, 1000, 1).await, "teardown never reached close");
        let second_disconnect = spawn_disconnect(&session);
        let next = MockTransport::new();
        let next_probe = next.probe();
        let reconnect = spawn_connect(&session, next, CONNECT_TIMEOUT_MS);

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
            .connect_to_studio(
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
}
