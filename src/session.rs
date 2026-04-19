//! WebTrip Session Manager
//!
//! High-level session management that coordinates all components:
//! - AudioEngine (capture/playback via AudioWorklet)
//! - Transport (abstraction for network layer - WebRTC, WebTransport, or Mock)
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
use crate::audio::regulator::Regulator;
use crate::audio::params::AudioParams;
use crate::audio::ring_buffer::RingBuffer;
use crate::audio::transport::{Transport, TransportType, AudioBufferConfig};
use crate::audio::webrtc::{TransportConfig as WebRtcConfig, WebRtcTransport};
use crate::audio::webtransport::{WebTransportImpl, is_webtransport_available};
use crate::audio::mock_transport::MockTransport;
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
}

#[wasm_bindgen]
impl SessionStats {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
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

    // Configuration
    sample_rate: u32,
    buffer_size: usize,
    channels: u8,

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
        let channels = 2; // Default to stereo

        // Sync channels to AudioParams so processor knows to duplicate mono to stereo
        if !audio_params_ptr.is_null() {
            unsafe {
                (*audio_params_ptr).set_output_channels(channels as u32);
            }
        }

        // Create owned buffers
        let local_to_network_buffer = Box::new(RingBuffer::new());
        let mut network_to_local_buffer = Box::new(Regulator::new());

        // Configure regulator with auto-adaptive tolerance and headroom (-500.0)
        network_to_local_buffer.configure(channels as usize, buffer_size, sample_rate, -500.0);

        Ok(WebTripSession {
            audio_params_ptr,
            audio_engine: None,
            local_to_network_buffer,
            network_to_local_buffer,
            transport: None,
            transport_type: TransportType::WebRTC, // Default to WebRTC
            state: SessionState::Idle,
            sample_rate,
            buffer_size,
            channels,
            on_state_change: None,
            pending_capture_params: None,
            output_device_id: None,
            audio_callback_loop: None,
            tick_callback_closure: None,
        })
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
        SessionStats {
            packets_sent: 0, // TODO: Get from transport if needed
            packets_received: reg_stats.packets_received,
            
            // Ring buffer (send path)
            send_buffer_available: self.local_to_network_buffer.available(),
            ring_buffer_writes: self.local_to_network_buffer.writes(),
            ring_buffer_samples_written: self.local_to_network_buffer.samples_written(),
            ring_buffer_overruns: self.local_to_network_buffer.overruns(),
            
            // Regulator (receive path with Burg PLC)
            regulator_tolerance_ms: reg_stats.tolerance_ms,
            regulator_headroom_ms: reg_stats.headroom_ms,
            regulator_max_latency_ms: reg_stats.max_latency_ms,
            regulator_depth: self.network_to_local_buffer.depth(),
            regulator_latency_ms: self.network_to_local_buffer.latency_ms(),
            regulator_initialized: self.network_to_local_buffer.is_initialized(),
            regulator_packets_played: reg_stats.packets_played,
            regulator_plc_count: reg_stats.glitches,
            regulator_skipped: reg_stats.skipped,
            regulator_last_seq: reg_stats.last_seq_received,
        }
    }

    /// Get the ring buffer flag pointer for event-driven wake-up
    /// 
    /// Used with Atomics.waitAsync() in JavaScript for zero-CPU idle behavior.
    /// Returns the memory address of the atomic flag that signals data availability.
    #[wasm_bindgen(js_name = getRingBufferFlagPtr)]
    pub fn get_ring_buffer_flag_ptr(&self) -> usize {
        self.local_to_network_buffer.get_has_data_flag_ptr()
    }

    /// Set the number of audio channels (1 for mono, 2 for stereo)
    /// Must be called before connecting
    #[wasm_bindgen(js_name = setChannels)]
    pub fn set_channels(&mut self, channels: u8) {
        if channels >= 1 && channels <= 8 {
            self.channels = channels;
            
            // Sync to AudioParams so processor knows to duplicate mono to stereo
            if !self.audio_params_ptr.is_null() {
                unsafe {
                    (*self.audio_params_ptr).set_output_channels(channels as u32);
                }
            }
            // Reconfigure regulator with new channel count
            self.network_to_local_buffer.configure(channels as usize, self.buffer_size, self.sample_rate, -1.0);
        }
    }

    /// Get the current channel count
    #[wasm_bindgen(js_name = getChannels)]
    pub fn get_channels(&self) -> u8 {
        self.channels
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
        if let Some(ref engine) = self.audio_engine {
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
        // Get raw pointers to owned buffers
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
                    auto_gain_control,
                    echo_cancellation,
                    noise_suppression,
                )
                .await?;
        }

        // Apply stored output device selection now that audio engine is ready
        if let Some(ref output_device) = self.output_device_id {
            if let Some(ref engine) = self.audio_engine {
                engine.set_output_device(Some(output_device.clone())).await?;
            }
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
                // WebRTC and Mock need the audio callback loop for tick()
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
            local_to_network_ptr: &mut *self.local_to_network_buffer as *mut RingBuffer,
            network_to_local_ptr: &mut *self.network_to_local_buffer as *mut Regulator,
            buffer_size: self.buffer_size,
            channels: self.channels,
        };

        // Create the appropriate transport based on type
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
                        // Map transport states to session states
                        let session_state = match state.as_str() {
                            "failed" | "disconnected" | "closed" => "error",
                            "connected" => "connected",
                            // Session already emits "connecting" before transport connect starts.
                            // Ignore transport-level "connecting" to avoid stale callbacks
                            // regressing the UI back to Connecting after a successful connect.
                            _ => return,
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
                    client_name.as_deref().unwrap_or(""),
                ).await?;
                
                Box::new(webrtc_transport)
            }
            TransportType::Mock => {
                let mut mock_transport = MockTransport::new();
                
                // Configure audio buffers (Mock needs these for its internal tick loop)
                mock_transport.set_audio_buffers(buffer_config);
                
                // Enable sine wave generation for mock transport
                mock_transport.enable_sine_wave();
                
                // Connect (mock transport connects instantly)
                // Use explicit trait method syntax to avoid ambiguity
                Transport::connect(
                    &mut mock_transport,
                    &server_host,
                    port,
                    client_name.as_deref().unwrap_or(""),
                ).await?;
                
                Box::new(mock_transport)
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
                        // Map transport states to session states.
                        //
                        // Note: the WebTransport worker only posts "disconnected" after a
                        // graceful, user-initiated close (i.e. once it has finished
                        // flushing the JackTrip exit packet). Unexpected connection
                        // failures are reported separately as {type:"error"}, which the
                        // WebTransport main-thread handler forwards as "failed". So
                        // "disconnected" must NOT be treated as an error — doing so
                        // causes the UI to flip from "Not Connected" to "Connection
                        // Error" ~20 ms after the user clicks Disconnect.
                        let session_state = match state.as_str() {
                            "failed" => "error",
                            "connected" => "connected",
                            "connecting" => "connecting",
                            _ => return,
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
                    client_name.as_deref().unwrap_or(""),
                ).await?;
                
                Box::new(webtransport)
            }
        };

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


    /// Disconnect from the hub server
    pub fn disconnect(&mut self) {
        // Close transport
        if let Some(ref mut transport) = self.transport {
            transport.close();
        }
        self.transport = None;

        // Stop audio capture when disconnecting (this will also stop the audio callback loop)
        self.stop_capture();

        // Clear any pending capture parameters
        self.pending_capture_params = None;

        // Reset network-to-local buffer
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


    fn set_state(&mut self, state: SessionState) {
        if self.state != state {
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
}

impl Drop for WebTripSession {
    fn drop(&mut self) {
        // Critical: Stop in correct order to ensure buffer pointers aren't used after drop
        self.disconnect();
        self.stop_capture();
    }
}
