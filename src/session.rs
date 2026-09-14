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
//! 1. connect_to_studio(server_host, port)
//!    └─> Stores audio capture parameters for later
//!    └─> Creates WebRtcTransport
//!    └─> WebRtcTransport.connect_to_hub() handles all signaling internally:
//!        - Creates HubSignaling (WebSocket)
//!        - Sends protocol handshake {"protocol":"webrtc",...}
//!        - Creates SDP offer
//!        - Handles SDP answer from server
//!        - Exchanges ICE candidates
//!        - Establishes data channel
//!
//! 2. Connection callback fires when ready
//!    └─> Transport moved to trait object (Box<dyn Transport>)
//!    └─> Audio capture starts
//!    └─> Network loop begins
//!
//! 3. Audio streaming
//!    └─> tick() loop sends/receives packets via Transport trait
//! ```

use wasm_bindgen::prelude::*;

use crate::audio::engine::AudioEngine;
use crate::audio::protocol::MAX_CHANNELS;
use crate::audio::regulator::{Regulator, RegulatorStats};
use crate::audio::params::AudioParams;
use crate::audio::ring_buffer::RingBuffer;
use crate::audio::shared_ptr::SharedPtr;
use crate::audio::transport::{Transport, TransportType, AudioBufferConfig};
use crate::audio::webrtc::{TransportConfig as WebRtcConfig, WebRtcTransport};
use crate::audio::webtransport::{WebTransportImpl, is_webtransport_available};
use crate::audio::audio_callback_loop::AudioCallbackLoop;
use wasm_bindgen::closure::Closure;

/// Default signaling port (same as JackTrip TCP port)
pub const DEFAULT_SIGNALING_PORT: u16 = 4464;

/// Session state
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Disconnected - no audio or network activity
    Idle,
    /// Connecting to hub server (signaling)
    Connecting,
    /// WebRTC negotiation in progress
    Negotiating,
    /// Fully connected and transmitting audio
    Connected,
    /// Error state
    Error,
}

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
    /// Number of packets skipped (late/reordered)
    pub regulator_skipped: u64,
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

/// Returns `true` when the state transition `from → to` is permitted.
///
/// Legal transitions:
/// ```text
/// Idle        → Connecting
/// Connecting  → Negotiating
/// Connecting  → Connected   (transport with no separate negotiation step)
/// Negotiating → Connected
/// Idle        → Connected   (disconnect raced and reset to Idle while
///                            connect_to_studio was awaiting transport.connect();
///                            the connect path must still reach Connected so the
///                            UI gets notified and transport/capture are live)
/// Connected   → Connecting  (reconnect: transport fires error directly to JS
///                            callback, bypassing set_state, so self.state can
///                            still be Connected when the user retries)
/// *           → Error       (failure from any state)
/// Error       → Idle        (reset after error)
/// Connected   → Idle        (disconnect)
/// Connecting  → Idle        (cancel connection)
/// Negotiating → Idle        (cancel negotiation)
/// ```
pub(crate) fn is_valid_state_transition(from: SessionState, to: SessionState) -> bool {
    use SessionState::*;
    matches!(
        (from, to),
        (Idle, Connecting)
            | (Connecting, Negotiating)
            | (Connecting, Connected)
            | (Negotiating, Connected)
            | (Idle, Connected) // disconnect raced connect; see doc comment above
            | (Connected, Connecting) // reconnect after transport-level error
            | (_, Error)
            | (Error, Idle)
            | (Connected, Idle)
            | (Connecting, Idle)
            | (Negotiating, Idle)
    )
}

/// Map a transport-level state string onto the session state string reported to
/// the app, or `None` when the transport state must not be forwarded at all.
///
/// Both `connect_to_studio` transport arms install a state-change callback that
/// performs this translation; the mapping lives here so it is written once and
/// is testable without a live transport.
///
/// The two transports deliberately differ on `"disconnected"`:
///
/// * **WebRTC** reports `"disconnected"`/`"closed"` only when the peer
///   connection or data channel drops, which from the session's point of view is
///   a failure → `"error"`.
/// * **WebTransport** posts `"disconnected"` from the worker after a *graceful*,
///   user-initiated close (once the JackTrip exit packets have been flushed);
///   genuine failures arrive separately as `{type:"error"}` and reach here as
///   `"failed"`. Treating `"disconnected"` as an error there flips the UI from
///   "Not Connected" to "Connection Error" ~20 ms after the user clicks
///   Disconnect, so it must be dropped instead.
///
/// Transport-level `"connecting"` is dropped for WebRTC because the session
/// already emits `"connecting"` before the transport connect starts, and a late
/// transport-level `"connecting"` would regress the UI after a successful
/// connect.
pub(crate) fn session_state_for_transport_state(
    transport_type: TransportType,
    transport_state: &str,
) -> Option<&'static str> {
    match (transport_type, transport_state) {
        (_, "connected") => Some("connected"),
        (_, "failed") => Some("error"),
        (TransportType::WebRTC, "disconnected" | "closed") => Some("error"),
        (TransportType::WebTransport, "connecting") => Some("connecting"),
        _ => None,
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
        regulator_skipped: reg_stats.skipped,
        regulator_last_seq: reg_stats.last_seq_received,
        regulator_packets_rejected: reg_stats.packets_rejected,
    }
}

/// WebTrip Session - coordinates all audio and network components
#[wasm_bindgen]
pub struct WebTripSession {
    // Audio components
    audio_params_ptr: *const AudioParams,
    audio_engine: Option<AudioEngine>,

    // Owned buffers (shared with AudioWorklet and Transport via pointers)
    local_to_network_buffer: Box<RingBuffer>,
    network_to_local_buffer: Box<Regulator>,

    // Network components
    transport: Option<Box<dyn Transport>>,
    transport_type: TransportType,

    // State
    state: SessionState,

    // Configuration. The four channel counts are independent, set only
    // while Idle, and fixed for the life of a connection.
    buffer_size: usize,
    /// Channels sent over the wire (payload width of every outbound packet)
    send_channels: u8,
    /// Channels asked back from the peer (advertised in every outbound header)
    receive_channels: u8,
    /// Input device channels to capture (the first N, clamped to the device)
    input_channels: u8,
    /// Output device channels to play to (the first N, clamped to the device)
    output_channels: u8,

    // Callbacks
    on_state_change: Option<js_sys::Function>,

    // Pending audio capture parameters (to start after connection)
    pending_capture_params: Option<PendingCaptureParams>,
    
    // Selected output device ID (stored until audio engine is ready)
    output_device_id: Option<String>,
    
    // Audio callback loop (triggers transport tick on each audio callback)
    audio_callback_loop: Option<AudioCallbackLoop>,
    tick_callback_closure: Option<Closure<dyn FnMut()>>,
}

/// Parameters for starting audio capture after connection
struct PendingCaptureParams {
    device_id: Option<String>,
    auto_gain_control: bool,
    echo_cancellation: bool,
    noise_suppression: bool,
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
        let mut network_to_local_buffer = Box::new(Regulator::new());

        // Configure regulator with auto-adaptive tolerance and headroom (-500.0).
        // The peer's actual count is still adopted from its first packet.
        network_to_local_buffer.configure(receive_channels as usize, buffer_size, sample_rate, -500.0);

        Ok(WebTripSession {
            audio_params_ptr,
            audio_engine: None,
            local_to_network_buffer,
            network_to_local_buffer,
            transport: None,
            transport_type: TransportType::WebRTC, // Default to WebRTC
            state: SessionState::Idle,
            buffer_size,
            send_channels,
            receive_channels,
            input_channels: 2,
            output_channels: 2,
            on_state_change: None,
            pending_capture_params: None,
            output_device_id: None,
            audio_callback_loop: None,
            tick_callback_closure: None,
        })
    }

    /// Connect the session over a caller-supplied, already-constructed transport,
    /// bypassing the real per-type transport construction in `connect_to_studio`.
    ///
    /// Unit-test only. Mirrors `connect_to_studio`'s pre-transport steps (mark
    /// `Connecting`, register default capture params) and then reuses the shared
    /// [`finalize_connection`] tail, so a test can drive the full connect →
    /// `AudioEngine`/worklet/callback-loop → `Connected` sequence against a mock
    /// transport with no live hub. Pair with [`disconnect`] for the teardown half.
    ///
    /// [`finalize_connection`]: Self::finalize_connection
    /// [`disconnect`]: Self::disconnect
    #[cfg(all(test, target_arch = "wasm32"))]
    pub(crate) async fn connect_with_test_transport(
        &mut self,
        mut transport: Box<dyn Transport>,
    ) -> Result<(), JsValue> {
        self.pending_capture_params = Some(PendingCaptureParams {
            device_id: None,
            auto_gain_control: false,
            echo_cancellation: false,
            noise_suppression: false,
        });
        self.set_state(SessionState::Connecting);

        Transport::connect(transport.as_mut(), "test-host", DEFAULT_SIGNALING_PORT, "").await?;

        self.finalize_connection(transport).await
    }

    /// Set callback for state changes
    pub fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.on_state_change = Some(callback);
    }

    /// Get current state
    pub fn state(&self) -> SessionState {
        self.state
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
    pub fn set_send_channels(&mut self, channels: u8) {
        if !self.can_set_channel_count("send", channels) {
            return;
        }
        self.send_channels = channels;
        if !self.audio_params_ptr.is_null() {
            unsafe {
                (*self.audio_params_ptr).set_send_channels(channels as u32);
            }
        }
    }

    /// Get the configured send channel count
    #[wasm_bindgen(js_name = getSendChannels)]
    pub fn get_send_channels(&self) -> u8 {
        self.send_channels
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
    pub fn set_receive_channels(&mut self, channels: u8) {
        if self.can_set_channel_count("receive", channels) {
            self.receive_channels = channels;
        }
    }

    /// Get the configured receive channel count
    #[wasm_bindgen(js_name = getReceiveChannels)]
    pub fn get_receive_channels(&self) -> u8 {
        self.receive_channels
    }

    /// Set how many input device channels to capture (default 2): the
    /// device's first N, clamped at capture time to what the browser grants.
    /// Idle-only (see [`set_send_channels`](Self::set_send_channels)).
    #[wasm_bindgen(js_name = setInputChannels)]
    pub fn set_input_channels(&mut self, channels: u8) {
        if self.can_set_channel_count("input", channels) {
            self.input_channels = channels;
        }
    }

    /// Get the configured input device channel count
    #[wasm_bindgen(js_name = getInputChannels)]
    pub fn get_input_channels(&self) -> u8 {
        self.input_channels
    }

    /// Set how many output device channels to play to (default 2): the
    /// device's first N, clamped at capture time to the destination's
    /// `maxChannelCount`. Idle-only (see
    /// [`set_send_channels`](Self::set_send_channels)).
    #[wasm_bindgen(js_name = setOutputChannels)]
    pub fn set_output_channels(&mut self, channels: u8) {
        if self.can_set_channel_count("output", channels) {
            self.output_channels = channels;
        }
    }

    /// Get the configured output device channel count
    #[wasm_bindgen(js_name = getOutputChannels)]
    pub fn get_output_channels(&self) -> u8 {
        self.output_channels
    }

    /// Set the transport type to use for connections
    /// Must be called before connecting
    #[wasm_bindgen(js_name = setTransportType)]
    pub fn set_transport_type(&mut self, transport_type: TransportType) {
        if self.state == SessionState::Idle {
            self.transport_type = transport_type;
            web_sys::console::log_1(&format!("🚀 Transport type set to: {}", transport_type.name()).into());
        } else {
            web_sys::console::warn_1(&"⚠️ Cannot change transport type while connected".into());
        }
    }

    /// Get the current transport type
    #[wasm_bindgen(js_name = getTransportType)]
    pub fn get_transport_type(&self) -> TransportType {
        self.transport_type
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
    pub async fn set_output_device(&mut self, device_id: Option<String>) -> Result<(), JsValue> {
        // Store the desired output device
        self.output_device_id = device_id.clone();
        
        // If audio engine exists, apply immediately
        if let Some(ref mut engine) = self.audio_engine {
            engine.set_output_device(device_id).await?;
        }
        Ok(())
    }

    /// Start audio capture (internal use only)
    async fn start_capture(
        &mut self,
        device_id: Option<String>,
        auto_gain_control: bool,
        echo_cancellation: bool,
        noise_suppression: bool,
    ) -> Result<(), JsValue> {
        // Get raw pointers to owned buffers. `AudioEngine` is a `#[wasm_bindgen]`
        // boundary type, so it takes raw pointers here and wraps them in
        // `SharedPtr` internally when it builds the `AudioProcessor`.
        let local_to_network_ptr = &mut *self.local_to_network_buffer as *mut RingBuffer;
        let network_to_local_ptr = &mut *self.network_to_local_buffer as *mut Regulator;

        // Create audio engine with network support
        let engine = AudioEngine::create_with_network(
            self.audio_params_ptr,
            local_to_network_ptr,
            network_to_local_ptr,
        )
        .await?;

        self.audio_engine = Some(engine);

        // Start capture
        if let Some(ref mut engine) = self.audio_engine {
            engine
                .start_capture(
                    device_id,
                    self.output_device_id.clone(),
                    auto_gain_control,
                    echo_cancellation,
                    noise_suppression,
                    self.input_channels as u32,
                    self.output_channels as u32,
                )
                .await?;
        }

        // Start audio callback loop ONLY for transports that need main-thread tick()
        // WebTransport handles all network I/O in a dedicated Worker thread
        match self.transport_type {
            TransportType::WebTransport => {
                // WebTransport worker handles send/receive loops - no tick() needed!
                // Just enable streaming on ring buffer for the worker to read
                self.local_to_network_buffer.set_streaming(true);
            }
            _ => {
                // WebRTC needs the main-thread audio callback loop for tick()
                self.start_audio_callback_loop();
            }
        }

        Ok(())
    }

    /// Stop audio capture (internal use only)
    fn stop_capture(&mut self) {
        // Stop audio callback loop first (no more ticks)
        self.stop_audio_callback_loop();
        
        // IMPORTANT: Stop audio engine before dropping
        // This ensures AudioWorklet stops using buffer pointers
        if let Some(ref mut engine) = self.audio_engine {
            engine.stop_capture();
        }
        self.audio_engine = None;

        self.set_state(SessionState::Idle);
    }


    // ========== Studio Connection Methods ==========

    /// Connect to a JackTrip hub server
    ///
    /// This is the main entry point for connecting to a studio. It:
    /// 1. Establishes a WebSocket connection for signaling
    /// 2. Creates a WebRTC peer connection
    /// 3. Exchanges SDP offer/answer
    /// 4. Establishes the data channel for audio transmission
    /// 5. Starts audio capture (after connection is established)
    ///
    /// Note: Audio capture is deferred until after the server connection
    /// completes to avoid capturing audio before a connection is ready.
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
        &mut self,
        server_host: String,
        port: Option<u16>,
        device_id: Option<String>,
        auto_gain_control: bool,
        echo_cancellation: bool,
        noise_suppression: bool,
        client_name: Option<String>,
    ) -> Result<(), JsValue> {
        // Store audio capture parameters to start after connection
        self.pending_capture_params = Some(PendingCaptureParams {
            device_id,
            auto_gain_control,
            echo_cancellation,
            noise_suppression,
        });

        let port = port.unwrap_or(DEFAULT_SIGNALING_PORT);

        self.set_state(SessionState::Connecting);

        // Create buffer configuration for transports that need it
        let buffer_config = AudioBufferConfig {
            local_to_network: SharedPtr::new(&mut *self.local_to_network_buffer),
            network_to_local: SharedPtr::new(&mut *self.network_to_local_buffer),
            buffer_size: self.buffer_size,
            send_channels: self.send_channels,
            receive_channels: self.receive_channels,
        };

        // Create the appropriate transport based on type
        let client_name_str = client_name.as_deref().unwrap_or("");
        use crate::audio::transport::Transport;
        let transport: Box<dyn Transport> = match self.transport_type {
            TransportType::WebRTC => {
                let mut webrtc_transport = WebRtcTransport::new(Some(WebRtcConfig::low_latency()))?;
                
                // Configure audio buffers (WebRTC needs these for its internal tick loop)
                webrtc_transport.set_audio_buffers(buffer_config);
                
                // Set up state change callback BEFORE connecting
                // This ensures the data channel and signaling handlers can use it
                if let Some(ref callback) = self.on_state_change {
                    let callback_clone = callback.clone();
                    let state_change_cb = Closure::wrap(Box::new(move |state: String| {
                        // Map transport states to session states (see
                        // `session_state_for_transport_state` for why WebRTC's
                        // mapping differs from WebTransport's).
                        let Some(session_state) =
                            session_state_for_transport_state(TransportType::WebRTC, &state)
                        else {
                            return;
                        };

                        // Notify the app
                        let _ = callback_clone.call1(&JsValue::NULL, &JsValue::from_str(session_state));
                    }) as Box<dyn FnMut(String)>);
                    
                    let js_func: js_sys::Function = state_change_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
                    webrtc_transport.set_on_state_change(js_func);
                    state_change_cb.forget(); // Keep callback alive
                }
                
                // Connect to hub using the unified Transport trait method
                // This will also start the internal tick loop for WebRTC
                webrtc_transport.connect(
                    &server_host,
                    port,
                    client_name_str,
                ).await?;
                
                Box::new(webrtc_transport)
            }
            TransportType::WebTransport => {
                // Check if WebTransport is available in this browser
                if !is_webtransport_available() {
                    return Err("WebTransport is not supported in this browser. Please use Chrome 97+ or Edge 97+.".into());
                }

                let mut webtransport = WebTransportImpl::new()?;
                
                // Configure audio buffers
                webtransport.set_audio_buffers(buffer_config);
                
                // Set up state change callback BEFORE connecting
                // This ensures the worker's message handler can use it
                if let Some(ref callback) = self.on_state_change {
                    let callback_clone = callback.clone();
                    let state_change_cb = Closure::wrap(Box::new(move |state: String| {
                        // Map transport states to session states. Note in
                        // particular that the worker's graceful "disconnected"
                        // must NOT become an error here — see
                        // `session_state_for_transport_state`.
                        let Some(session_state) =
                            session_state_for_transport_state(TransportType::WebTransport, &state)
                        else {
                            return;
                        };

                        // Notify the app
                        let _ = callback_clone.call1(&JsValue::NULL, &JsValue::from_str(session_state));
                    }) as Box<dyn FnMut(String)>);
                    
                    let js_func: js_sys::Function = state_change_cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
                    webtransport.set_on_state_change(js_func);
                    state_change_cb.forget(); // Keep callback alive
                }
                
                // Connect via worker thread
                // WebTransport uses a dedicated Worker for network I/O
                Transport::connect(
                    &mut webtransport,
                    &server_host,
                    port,
                    client_name_str,
                ).await?;
                
                Box::new(webtransport)
            }
        };

        self.finalize_connection(transport).await
    }

    /// Wire an already-connected transport into the session and bring audio up.
    ///
    /// Stores the transport, advances the session to `Connected`, and starts the
    /// pending audio capture (which creates the `AudioEngine`, bootstraps the
    /// worklet, and — for main-thread transports — starts the `Atomics.waitAsync`
    /// audio-callback loop). This is the transport-agnostic tail shared by
    /// `connect_to_studio` and the unit-test connect path, so both drive the same
    /// bring-up sequence.
    async fn finalize_connection(&mut self, transport: Box<dyn Transport>) -> Result<(), JsValue> {
        // Store the connected transport
        self.transport = Some(transport);

        // Connection established - update state
        self.set_state(SessionState::Connected);

        // Start audio capture if pending (this will also start the audio callback loop)
        if let Some(params) = self.pending_capture_params.take() {
            web_sys::console::log_1(&"🎙️ Starting audio capture after connection established".into());
            self.start_capture(
                params.device_id,
                params.auto_gain_control,
                params.echo_cancellation,
                params.noise_suppression,
            ).await?;
        }

        Ok(())
    }


    /// Disconnect from the hub server.
    ///
    /// This awaits full transport teardown before resetting the
    /// network-to-local buffer. That ordering is load-bearing: the
    /// `Regulator`'s sequence-number state is shared via a raw pointer with
    /// the transport (for WebTransport, the worker thread writes to it), so
    /// resetting before the transport has quiesced creates a race in which a
    /// late `regulator.push()` from the old connection pins `last_seq_in`
    /// high and causes the *next* connection's packets to be rejected by the
    /// wrap-distance check. Awaiting `transport.close()` establishes the
    /// happens-before ordering we need: for WebTransport the future only
    /// resolves after `worker.terminate()`; for WebRTC the teardown is
    /// fully synchronous.
    pub async fn disconnect(&mut self) {
        if let Some(mut transport) = self.transport.take() {
            transport.close().await;
            // `transport` is dropped at end of scope; its `Drop` impl is a
            // no-op on already-closed state.
        }

        // Stop audio capture when disconnecting (this will also stop the audio callback loop)
        self.stop_capture();

        // Clear any pending capture parameters
        self.pending_capture_params = None;

        // Now that no further writes to the Regulator are possible, reset it
        // so the next connection starts from a clean slate.
        self.network_to_local_buffer.reset();

        self.set_state(SessionState::Idle);
    }
    
    /// Start the audio callback loop
    fn start_audio_callback_loop(&mut self) {
        if self.audio_callback_loop.is_some() {
            return; // Already started
        }
        
        // Enable streaming on ring buffer
        self.local_to_network_buffer.set_streaming(true);
        
        // Get ring buffer flag pointer
        let flag_ptr = self.local_to_network_buffer.get_has_data_flag_ptr();
        
        // Create audio callback loop
        let mut callback_loop = AudioCallbackLoop::new();
        
        // Create tick callback that calls transport.tick()
        let session_ptr = self as *mut WebTripSession;
        let tick_closure = Closure::wrap(Box::new(move || {
            unsafe {
                if !session_ptr.is_null() {
                    if let Some(ref mut transport) = (*session_ptr).transport {
                        transport.tick();
                    }
                }
            }
        }) as Box<dyn FnMut()>);
        let tick_fn = tick_closure.as_ref().unchecked_ref::<js_sys::Function>().clone();
        
        // Try Atomics.waitAsync
        let memory = wasm_bindgen::memory();
        
        match callback_loop.start_with_atomics(&memory, flag_ptr, tick_fn) {
            Ok(true) => {
                self.audio_callback_loop = Some(callback_loop);
                self.tick_callback_closure = Some(tick_closure);
                web_sys::console::debug_1(&"✅ Audio callback loop started".into());
            }
            Ok(false) | Err(_) => {
                web_sys::console::warn_1(&"⚠️ Atomics.waitAsync not available".into());
            }
        }
    }
    
    /// Stop the audio callback loop
    fn stop_audio_callback_loop(&mut self) {
        if let Some(mut callback_loop) = self.audio_callback_loop.take() {
            callback_loop.stop();
        }
        self.tick_callback_closure = None;
        
        // Disable streaming on ring buffer
        self.local_to_network_buffer.set_streaming(false);
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
        if let Some(ref engine) = self.audio_engine {
            engine.resume_ctx().await?;
        }
        Ok(())
    }

    /// Check if connected to hub server
    #[wasm_bindgen(js_name = isConnected)]
    pub fn is_connected(&self) -> bool {
        self.transport
            .as_ref()
            .map(|t| t.is_connected())
            .unwrap_or(false)
    }

    // ========== Private Methods ==========

    /// Shared guard for the channel-count setters: a count may change only
    /// while `Idle` and only within [1, `MAX_CHANNELS`]. Logs the reason
    /// (`which` names the setting) when it may not.
    fn can_set_channel_count(&self, which: &str, channels: u8) -> bool {
        if self.state != SessionState::Idle {
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

    fn set_state(&mut self, state: SessionState) {
        if self.state == state {
            return;
        }
        if !is_valid_state_transition(self.state, state) {
            web_sys::console::warn_1(
                &format!("⚠️ Invalid state transition: {:?} → {:?}", self.state, state).into(),
            );
            return;
        }
        self.state = state;

        if let Some(ref callback) = self.on_state_change {
            let state_str = match state {
                SessionState::Idle => "idle",
                SessionState::Connecting => "connecting",
                SessionState::Negotiating => "negotiating",
                SessionState::Connected => "connected",
                SessionState::Error => "error",
            };
            let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(state_str));
        }
    }
}

impl Drop for WebTripSession {
    fn drop(&mut self) {
        // `disconnect()` is now async and can't be awaited from Drop. Instead,
        // best-effort teardown: drop the transport (its own Drop impl fires
        // off the shutdown message and schedules the fallback terminate) and
        // stop audio capture. The Regulator doesn't need to be reset in Drop
        // because the whole session is being deallocated.
        self.transport = None;
        self.stop_capture();
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
    // State-transition validation
    // -----------------------------------------------------------------------

    #[test]
    fn legal_state_transitions_accepted() {
        let legal: &[(SessionState, SessionState)] = &[
            (SessionState::Idle, SessionState::Connecting),
            (SessionState::Connecting, SessionState::Negotiating),
            (SessionState::Connecting, SessionState::Connected),
            (SessionState::Negotiating, SessionState::Connected),
            // disconnect() raced connect_to_studio() and reset to Idle; the
            // connect path must still reach Connected so the UI is notified.
            (SessionState::Idle, SessionState::Connected),
            // Reconnect: transport fires error directly to JS, self.state stays
            // Connected; the next connect_to_studio must be able to proceed.
            (SessionState::Connected, SessionState::Connecting),
            // Error from every state
            (SessionState::Idle, SessionState::Error),
            (SessionState::Connecting, SessionState::Error),
            (SessionState::Negotiating, SessionState::Error),
            (SessionState::Connected, SessionState::Error),
            // Reset / disconnect paths
            (SessionState::Error, SessionState::Idle),
            (SessionState::Connected, SessionState::Idle),
            (SessionState::Connecting, SessionState::Idle),
            (SessionState::Negotiating, SessionState::Idle),
        ];
        for &(from, to) in legal {
            assert!(
                is_valid_state_transition(from, to),
                "{from:?} → {to:?} should be a legal transition"
            );
        }
    }

    #[test]
    fn illegal_state_transitions_rejected() {
        let illegal: &[(SessionState, SessionState)] = &[
            (SessionState::Idle, SessionState::Negotiating),
            (SessionState::Negotiating, SessionState::Connecting),
            (SessionState::Connected, SessionState::Negotiating),
            (SessionState::Error, SessionState::Connecting),
            (SessionState::Error, SessionState::Connected),
            (SessionState::Error, SessionState::Negotiating),
        ];
        for &(from, to) in illegal {
            assert!(
                !is_valid_state_transition(from, to),
                "{from:?} → {to:?} should be an illegal transition"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Transport-state → session-state mapping
    // -----------------------------------------------------------------------

    /// Full matrix over both transports and every state string either of them
    /// can emit. The two rows that matter most:
    ///
    /// * WebTransport `"disconnected"` must map to `None` (regression: a
    ///   graceful user-initiated disconnect flipped the UI to "Connection
    ///   Error" ~20 ms after the exit packets were flushed).
    /// * WebRTC `"disconnected"`/`"closed"` must map to `"error"` (the peer
    ///   connection really did drop).
    #[test]
    fn transport_state_maps_to_session_state_per_transport() {
        use TransportType::{WebRTC, WebTransport};

        let cases: &[(TransportType, &str, Option<&str>)] = &[
            // Shared across transports.
            (WebRTC, "connected", Some("connected")),
            (WebTransport, "connected", Some("connected")),
            (WebRTC, "failed", Some("error")),
            (WebTransport, "failed", Some("error")),
            // Divergent: a dropped peer connection vs. a graceful worker close.
            (WebRTC, "disconnected", Some("error")),
            (WebRTC, "closed", Some("error")),
            (WebTransport, "disconnected", None),
            (WebTransport, "closed", None),
            // Divergent: WebRTC drops the late transport-level "connecting"
            // (the session already emitted it); WebTransport forwards it.
            (WebRTC, "connecting", None),
            (WebTransport, "connecting", Some("connecting")),
            // Anything unrecognized is dropped rather than guessed at.
            (WebRTC, "negotiating", None),
            (WebTransport, "negotiating", None),
            (WebRTC, "", None),
            (WebTransport, "", None),
        ];

        for &(transport, state, expected) in cases {
            assert_eq!(
                session_state_for_transport_state(transport, state),
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
            skipped: 1,
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
        assert_eq!(s.regulator_skipped, 1);
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
    // Real-browser coverage of the async connect/disconnect state machine and
    // the `AudioContext`-dependent audio controls, run in headless Chrome via
    // `npm run test:wasm`. The per-binary browser opt-in
    // (`wasm_bindgen_test_configure!(run_in_browser)`) lives once in
    // `crate::test_support`.
    //
    // The pure decision logic (`is_valid_channel_count`,
    // `is_valid_state_transition`, `build_session_stats`) is covered by the
    // native tests above (WEB-18) and is not re-covered here.
    //
    // The *successful* connect lifecycle is driven over `test_support::
    // MockTransport` via `connect_with_test_transport`, which reuses the same
    // `finalize_connection` → `AudioEngine` → `disconnect` path as
    // `connect_to_studio` without a live hub. The real WebRTC/WebTransport arms'
    // network handshakes (SDP/ICE, QUIC) still require a live JackTrip hub and are
    // covered by the integration tests; here they are only exercised on their
    // failure paths.

    #[cfg(target_arch = "wasm32")]
    use crate::test_support::{recording_state_callback, wait_until, MockTransport};
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
        session: &mut WebTripSession,
    ) -> (Rc<RefCell<Vec<String>>>, Closure<dyn FnMut(String)>) {
        let (log, closure) = recording_state_callback();
        session.set_on_state_change(closure.as_ref().unchecked_ref::<js_sys::Function>().clone());
        (log, closure)
    }

    /// Drive a full connect → audio-controls → disconnect cycle over the
    /// server-free [`MockTransport`], via `connect_with_test_transport`.
    ///
    /// Asserts the `Idle → Connecting → Connected` progression (both via
    /// `state()` and the order of the `on_state_change` callbacks), exercises
    /// the `AudioContext`-backed `is_audio_suspended`/`resume_audio` on the
    /// now-live engine, then tears down via `disconnect()` and asserts the
    /// session returns to `Idle`. Along the way it pins the channel-count
    /// setters' contract: applied while `Idle`, ignored while connected, and
    /// retained across disconnect (nothing narrows them). This runs the real async/await connect path,
    /// `AudioEngine::create`, worklet bootstrap, and the `Atomics.waitAsync`
    /// audio-callback loop in the browser — only the network transport is mocked.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_mock_connect_disconnect_lifecycle() {
        // `params` must outlive `session` (the session holds a raw pointer to
        // it); declaring it first means it is dropped last.
        let params = AudioParams::default();
        let mut session = WebTripSession::new(&params as *const AudioParams)
            .expect("session construction should succeed");

        let (log, _cb) = attach_state_log(&mut session);

        assert_eq!(session.state(), SessionState::Idle);
        assert!(!session.is_connected());

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

        session
            .connect_with_test_transport(Box::new(MockTransport::new()))
            .await
            .expect("mock connect should succeed");

        assert_eq!(session.state(), SessionState::Connected);
        assert!(session.is_connected());

        // The callback must have observed Idle → Connecting → Connected, in
        // order, driven by `set_state`.
        {
            let states = log.borrow();
            let connecting = states.iter().position(|s| s == "connecting");
            let connected = states.iter().position(|s| s == "connected");
            assert!(
                connecting.is_some(),
                "expected a 'connecting' callback, got {states:?}"
            );
            assert!(
                connected.is_some(),
                "expected a 'connected' callback, got {states:?}"
            );
            assert!(
                connecting < connected,
                "'connecting' must precede 'connected', got {states:?}"
            );
        }

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
        // Wait on the actual teardown transition rather than a fixed delay:
        // poll until the session reports `Idle` (or bail after a generous
        // budget) so the assertion is not timing-dependent.
        let reached_idle = wait_until(
            || session.state() == SessionState::Idle,
            /* timeout_ms */ 1000,
            /* interval_ms */ 5,
        )
        .await;
        assert!(
            reached_idle,
            "session must return to Idle after disconnect, got {:?}",
            session.state()
        );
        assert!(!session.is_connected());
        assert_eq!(
            log.borrow().last().map(String::as_str),
            Some("idle"),
            "the final emitted state after disconnect must be 'idle', got {:?}",
            log.borrow()
        );
        assert_eq!(
            channel_config(&session),
            configured,
            "disconnect must retain the configured channel counts"
        );
    }

    /// A connect with an empty host must surface an `Err` rather than report a
    /// false `Connected`.
    ///
    /// Uses the default WebRTC transport: building the signaling `WebSocket`
    /// against the resulting invalid `wss://:PORT/webrtc` URL fails without any
    /// live server, so `connect_to_studio` returns `Err`. We assert the session
    /// never reaches `Connected` and reports `is_connected() == false`. (The
    /// session does advance to `Connecting` before the transport is built — the
    /// state machine has no pre-async host validation — so we assert "did not
    /// reach Connected", not literal state equality.)
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_connect_invalid_host_fails_without_connecting() {
        let params = AudioParams::default();
        let mut session = WebTripSession::new(&params as *const AudioParams)
            .expect("session construction should succeed");

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
        assert_ne!(
            session.state(),
            SessionState::Connected,
            "a failed connect must not leave the session reporting Connected"
        );
        assert!(
            !session.is_connected(),
            "a failed connect must not report an open transport"
        );
    }

    /// With no engine yet created (fresh session, never connected), the
    /// AudioContext-backed controls must take their no-engine branch:
    /// `is_audio_suspended` returns `false` and `resume_audio` resolves `Ok`
    /// without touching any `AudioContext`.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn session_audio_controls_no_engine_branch() {
        let params = AudioParams::default();
        let session = WebTripSession::new(&params as *const AudioParams)
            .expect("session construction should succeed");

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
