//! JackTrip Session Manager
//!
//! High-level session management that coordinates all components:
//! - AudioEngine (capture/playback via AudioWorklet)
//! - AudioClient (WebRTC connection and packet handling)
//! - RingBuffer (send path: worklet -> network)
//! - JitterBuffer (receive path: network -> worklet)
//!
//! This keeps all audio/network logic in Rust, with TypeScript
//! only handling UI interactions.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::audio::engine::AudioEngine;
use crate::audio::params::AudioParams;
use crate::audio::client::{AudioClient, JackTripConfig};
use crate::audio::jitter_buffer::LockFreeJitterBuffer;
use crate::audio::ring_buffer::RingBuffer;

/// Session state
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// No audio or network activity
    Idle,
    /// Audio capture active, no network connection
    LocalOnly,
    /// Connecting to remote peer
    Connecting,
    /// Connected, buffering audio
    Buffering,
    /// Fully connected and streaming
    Streaming,
    /// Error state
    Error,
}

/// Session statistics
#[wasm_bindgen]
#[derive(Debug, Clone, Default)]
pub struct SessionStats {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub jitter_depth: u32,
    pub jitter_latency_ms: f32,
    pub send_buffer_available: u32,
}

#[wasm_bindgen]
impl SessionStats {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self::default()
    }
}

/// JackTrip Session - coordinates all audio and network components
#[wasm_bindgen]
pub struct JackTripSession {
    // Audio components
    audio_params_ptr: *const AudioParams,
    audio_engine: Option<AudioEngine>,
    
    // Owned buffers (shared with AudioWorklet via pointers)
    local_to_network_buffer: Box<RingBuffer>,
    network_to_local_buffer: Box<LockFreeJitterBuffer>,
    
    // Network components
    client: Option<AudioClient>,
    
    // State
    state: SessionState,
    sequence_number: u64,
    
    // Configuration
    sample_rate: u32,
    buffer_size: usize,
    
    // Buffers for network processing
    audio_to_send_buffer: Vec<f32>,
    
    // Callbacks
    on_state_change: Option<js_sys::Function>,
    on_signaling: Option<js_sys::Function>,
    
    // Network loop handle
    interval_handle: Option<i32>,
}

#[wasm_bindgen]
impl JackTripSession {
    /// Create a new session
    #[wasm_bindgen(constructor)]
    pub fn new(audio_params_ptr: *const AudioParams) -> Result<JackTripSession, JsValue> {
        let buffer_size = 128;
        let sample_rate = 48000;
        
        // Create owned buffers
        let local_to_network_buffer = Box::new(RingBuffer::new());
        let network_to_local_buffer = Box::new(LockFreeJitterBuffer::new());
        
        // Configure jitter buffer
        network_to_local_buffer.configure(buffer_size as u32, 4, 2);
        
        Ok(JackTripSession {
            audio_params_ptr,
            audio_engine: None,
            local_to_network_buffer,
            network_to_local_buffer,
            client: None,
            state: SessionState::Idle,
            sequence_number: 0,
            sample_rate,
            buffer_size,
            audio_to_send_buffer: vec![0.0; buffer_size],
            on_state_change: None,
            on_signaling: None,
            interval_handle: None,
        })
    }

    /// Set callback for state changes
    pub fn set_on_state_change(&mut self, callback: js_sys::Function) {
        self.on_state_change = Some(callback);
    }

    /// Set callback for signaling messages (SDP, ICE candidates)
    pub fn set_on_signaling(&mut self, callback: js_sys::Function) {
        self.on_signaling = Some(callback.clone());
        if let Some(ref mut client) = self.client {
            client.set_on_signaling(callback);
        }
    }

    /// Get current state
    pub fn state(&self) -> SessionState {
        self.state
    }

    /// Get current statistics
    pub fn get_stats(&self) -> SessionStats {
        let mut stats = SessionStats::default();
        
        if let Some(ref client) = self.client {
            let client_stats = client.get_stats();
            stats.packets_sent = client_stats.packets_sent;
            stats.packets_received = client_stats.packets_received;
        }
        
        stats.jitter_depth = self.network_to_local_buffer.depth();
        stats.jitter_latency_ms = self.network_to_local_buffer.latency_ms(self.sample_rate);
        stats.send_buffer_available = self.local_to_network_buffer.available();
        
        stats
    }

    /// Start audio capture
    #[wasm_bindgen(js_name = startCapture)]
    pub async fn start_capture(
        &mut self,
        device_id: Option<String>,
        auto_gain_control: bool,
        echo_cancellation: bool,
        noise_suppression: bool,
    ) -> Result<(), JsValue> {
        // Get raw pointers to owned buffers
        let local_to_network_ptr = &mut *self.local_to_network_buffer as *mut RingBuffer;
        let network_to_local_ptr = &*self.network_to_local_buffer as *const LockFreeJitterBuffer;
        
        // Create audio engine with network support
        let engine = AudioEngine::create_with_network(
            self.audio_params_ptr,
            local_to_network_ptr,
            network_to_local_ptr,
        ).await?;
        
        self.audio_engine = Some(engine);
        
        // Start capture
        if let Some(ref mut engine) = self.audio_engine {
            engine.start_capture(
                device_id,
                auto_gain_control,
                echo_cancellation,
                noise_suppression,
            ).await?;
        }
        
        self.set_state(SessionState::LocalOnly);
        Ok(())
    }

    /// Stop audio capture
    #[wasm_bindgen(js_name = stopCapture)]
    pub fn stop_capture(&mut self) {
        self.stop_network_loop();
        
        // IMPORTANT: Stop audio engine before dropping
        // This ensures AudioWorklet stops using buffer pointers
        if let Some(ref mut engine) = self.audio_engine {
            engine.stop_capture();
        }
        self.audio_engine = None;
        // Now it's safe - buffers are still owned but no longer accessed by worklet
        
        self.set_state(SessionState::Idle);
    }

    /// Check if audio is being captured
    #[wasm_bindgen(js_name = isCapturing)]
    pub fn is_capturing(&self) -> bool {
        self.audio_engine.as_ref().map(|e| e.is_capturing()).unwrap_or(false)
    }

    // ========== Connection Methods ==========

    /// Create an offer to initiate connection
    #[wasm_bindgen(js_name = createOffer)]
    pub async fn create_offer(&mut self) -> Result<String, JsValue> {
        self.ensure_client()?;
        self.set_state(SessionState::Connecting);
        
        let offer = self.client.as_mut().unwrap().connect().await?;
        Ok(offer)
    }

    /// Handle an incoming offer
    #[wasm_bindgen(js_name = handleOffer)]
    pub async fn handle_offer(&mut self, offer_sdp: String) -> Result<String, JsValue> {
        self.ensure_client()?;
        self.set_state(SessionState::Connecting);
        
        let answer = self.client.as_mut().unwrap().handle_offer(&offer_sdp).await?;
        Ok(answer)
    }

    /// Handle an incoming answer
    #[wasm_bindgen(js_name = handleAnswer)]
    pub async fn handle_answer(&mut self, answer_sdp: String) -> Result<(), JsValue> {
        if let Some(ref mut client) = self.client {
            client.handle_answer(&answer_sdp).await?;
            self.set_state(SessionState::Buffering);
            self.start_network_loop();
        }
        Ok(())
    }

    /// Add an ICE candidate
    #[wasm_bindgen(js_name = addIceCandidate)]
    pub async fn add_ice_candidate(&mut self, candidate: String) -> Result<(), JsValue> {
        if let Some(ref mut client) = self.client {
            client.add_ice_candidate(&candidate).await?;
        }
        Ok(())
    }

    /// Disconnect from remote peer
    pub fn disconnect(&mut self) {
        self.stop_network_loop();
        
        if let Some(ref mut client) = self.client {
            client.disconnect();
        }
        self.client = None;
        
        // Reset network-to-local buffer
        self.network_to_local_buffer.reset();
        
        self.sequence_number = 0;
        
        if self.is_capturing() {
            self.set_state(SessionState::LocalOnly);
        } else {
            self.set_state(SessionState::Idle);
        }
    }

    /// Check if connected to remote peer
    #[wasm_bindgen(js_name = isConnected)]
    pub fn is_connected(&self) -> bool {
        self.client.as_ref().map(|c| c.is_connected()).unwrap_or(false)
    }

    /// Process one network tick (called by interval)
    /// This is public so JS can call it, but typically managed internally
    pub fn tick(&mut self) {
        if self.client.is_none() {
            return;
        }
        
        let client = self.client.as_mut().unwrap();
        
        if !client.is_connected() {
            return;
        }

        // === SEND PATH: local audio → network ===
        // Read audio from worklet's ring buffer and send to network
        if self.local_to_network_buffer.available() >= self.buffer_size as u32 {
            if self.local_to_network_buffer.read(&mut self.audio_to_send_buffer) {
                let _ = client.send_audio(&self.audio_to_send_buffer);
            }
        }

        // === RECEIVE PATH: network → local audio ===
        // Receive audio from network and push to jitter buffer
        if let Ok(samples) = client.receive_audio() {
            if !samples.is_empty() {
                self.network_to_local_buffer.push(self.sequence_number, &samples);
                self.sequence_number += 1;
                
                // Update state based on jitter buffer
                if self.network_to_local_buffer.is_playing() && self.state == SessionState::Buffering {
                    self.set_state(SessionState::Streaming);
                }
            }
        }
    }

    // ========== Private Methods ==========

    fn ensure_client(&mut self) -> Result<(), JsValue> {
        if self.client.is_none() {
            let config = JackTripConfig::low_latency();
            let mut client = AudioClient::new(Some(config))?;
            
            // Forward signaling callback
            if let Some(ref callback) = self.on_signaling {
                client.set_on_signaling(callback.clone());
            }
            
            self.client = Some(client);
        }
        Ok(())
    }

    fn set_state(&mut self, state: SessionState) {
        if self.state != state {
            self.state = state;
            
            if let Some(ref callback) = self.on_state_change {
                let state_str = match state {
                    SessionState::Idle => "idle",
                    SessionState::LocalOnly => "local",
                    SessionState::Connecting => "connecting",
                    SessionState::Buffering => "buffering",
                    SessionState::Streaming => "streaming",
                    SessionState::Error => "error",
                };
                let _ = callback.call1(&JsValue::NULL, &JsValue::from_str(state_str));
            }
        }
    }

    fn start_network_loop(&mut self) {
        if self.interval_handle.is_some() {
            return;
        }
        
        // Enable streaming on the local-to-network buffer
        self.local_to_network_buffer.set_streaming(true);
        
        // Reset sequence number
        self.sequence_number = 0;
        
        // Reset network-to-local buffer
        self.network_to_local_buffer.reset();
        
        // Set up interval using web-sys
        let window = web_sys::window().expect("no global window");
        
        // We need to create a closure that calls tick()
        // Since we can't capture self, we use a raw pointer approach
        // This is safe because the interval is cleared before the session is dropped
        let session_ptr = self as *mut JackTripSession;
        
        let closure = Closure::wrap(Box::new(move || {
            unsafe {
                if !session_ptr.is_null() {
                    (*session_ptr).tick();
                }
            }
        }) as Box<dyn FnMut()>);
        
        let handle = window.set_interval_with_callback_and_timeout_and_arguments_0(
            closure.as_ref().unchecked_ref(),
            5, // 5ms interval for low latency
        ).expect("failed to set interval");
        
        closure.forget(); // Leak the closure so it stays alive
        
        self.interval_handle = Some(handle);
    }

    fn stop_network_loop(&mut self) {
        if let Some(handle) = self.interval_handle.take() {
            if let Some(window) = web_sys::window() {
                window.clear_interval_with_handle(handle);
            }
        }
        
        // Disable streaming on the local-to-network buffer
        self.local_to_network_buffer.set_streaming(false);
    }
}

impl Drop for JackTripSession {
    fn drop(&mut self) {
        // Critical: Stop in correct order to ensure buffer pointers aren't used after drop
        self.stop_network_loop();
        self.disconnect();
        self.stop_capture();  // Stops AudioWorklet, ensuring no more buffer access
        // Now buffers can be safely dropped
    }
}

