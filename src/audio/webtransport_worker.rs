//! WebTransport Worker Module
//!
//! This module provides the worker-side implementation for WebTransport audio transport.
//! It runs in a dedicated Web Worker thread, handling all network I/O independently
//! from the main thread and AudioWorklet.
//!
//! ## Architecture
//!
//! ```text
//! Main Thread                    Worker Thread (this module)
//! ┌────────────┐                 ┌─────────────────────────┐
//! │  Session   │ ───postMessage──>│  WebTransportWorker     │
//! │            │                 │    ├─ send_loop()       │
//! │            │                 │    └─ receive_loop()    │
//! └────────────┘                 └─────────────────────────┘
//!       │                                    │
//!       │  SharedArrayBuffer                 │
//!       ▼                                    ▼
//! ┌─────────────────────────────────────────────────────────┐
//! │           RingBuffer          │        Regulator        │
//! │     (AudioWorklet writes)     │   (AudioWorklet reads)  │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Thread Safety
//!
//! - RingBuffer and Regulator use atomics for lock-free access
//! - Worker accesses these via raw pointers (valid because WASM memory is SharedArrayBuffer)
//! - No locks are needed - atomics provide the necessary synchronization

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use js_sys::{Object, Reflect, Uint8Array};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};

use crate::audio::ring_buffer::RingBuffer;
use crate::audio::regulator::Regulator;
use crate::audio::protocol::{AudioPacket, HEADER_SIZE};

/// Post a message to the main thread from the worker
fn post_message_to_main(msg: &JsValue) {
    // Get the global worker scope
    let global = js_sys::global().unchecked_into::<web_sys::DedicatedWorkerGlobalScope>();
    let _ = global.post_message(msg);
}

/// Worker state shared between message handler and transport loops
struct WorkerState {
    /// Raw pointer to RingBuffer (send path: AudioWorklet -> Network)
    ring_buffer_ptr: *mut RingBuffer,
    /// Raw pointer to Regulator (receive path: Network -> AudioWorklet)
    regulator_ptr: *mut Regulator,
    /// Audio buffer configuration
    buffer_size: usize,
    channels: u8,
    /// Sequence number for outgoing packets
    sequence_number: AtomicU16,
    /// Timestamp for outgoing packets
    timestamp: AtomicU64,
    /// Flag to signal loops to stop
    running: AtomicBool,
    /// Reusable buffers for packet processing (avoid allocations in hot path)
    audio_buffer: RefCell<Vec<f32>>,
    packet_buffer: RefCell<Vec<u8>>,
    samples_buffer: RefCell<Vec<f32>>,
    /// Cached Int32Array for Atomics.wait() operations
    has_data_int32_array: RefCell<Option<js_sys::Int32Array>>,
}

// Safety: Worker is single-threaded in WASM, and these pointers are to SharedArrayBuffer
// which is designed for cross-thread access with atomics
unsafe impl Send for WorkerState {}
unsafe impl Sync for WorkerState {}

impl WorkerState {
    fn new() -> Self {
        Self {
            ring_buffer_ptr: std::ptr::null_mut(),
            regulator_ptr: std::ptr::null_mut(),
            buffer_size: 128,
            channels: 2,
            sequence_number: AtomicU16::new(0),
            timestamp: AtomicU64::new(0),
            running: AtomicBool::new(false),
            audio_buffer: RefCell::new(Vec::new()),
            packet_buffer: RefCell::new(Vec::new()),
            samples_buffer: RefCell::new(Vec::new()),
            has_data_int32_array: RefCell::new(None),
        }
    }

    fn configure(&mut self, ring_ptr: usize, reg_ptr: usize, buffer_size: usize, channels: u8) {
        self.ring_buffer_ptr = ring_ptr as *mut RingBuffer;
        self.regulator_ptr = reg_ptr as *mut Regulator;
        self.buffer_size = buffer_size;
        self.channels = channels;
        
        // Pre-allocate buffers
        let samples_per_packet = buffer_size * channels as usize;
        *self.audio_buffer.borrow_mut() = vec![0.0f32; samples_per_packet];
        
        // Max packet size: header + samples as 16-bit
        let max_packet_size = HEADER_SIZE + samples_per_packet * 2;
        *self.packet_buffer.borrow_mut() = vec![0u8; max_packet_size];
        *self.samples_buffer.borrow_mut() = Vec::with_capacity(samples_per_packet);
        
        // Set up Int32Array for Atomics.wait() on the ring buffer's has_data flag
        if !self.ring_buffer_ptr.is_null() {
            let ring_buffer = unsafe { &*self.ring_buffer_ptr };
            let flag_ptr = ring_buffer.get_has_data_flag_ptr();
            
            // Create Int32Array view of the has_data flag
            // Safety: The flag is an AtomicU32 at a valid memory location in SharedArrayBuffer
            let int32_array = unsafe {
                js_sys::Int32Array::view_mut_raw(flag_ptr as *mut i32, 1)
            };
            
            *self.has_data_int32_array.borrow_mut() = Some(int32_array);
        }
    }

    fn start(&self) {
        self.running.store(true, Ordering::SeqCst);
    }

    fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

thread_local! {
    static WORKER_STATE: RefCell<WorkerState> = RefCell::new(WorkerState::new());
}

/// Statistics for the WebTransport worker
#[wasm_bindgen]
#[derive(Debug, Clone, Default)]
pub struct WebTransportWorkerStats {
    pub packets_sent: u64,
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub send_errors: u64,
    pub receive_errors: u64,
}

// Global stats (accessed from async loops)
thread_local! {
    static STATS: RefCell<WebTransportWorkerStats> = RefCell::new(WebTransportWorkerStats::default());
}

/// Initialize the worker with buffer pointers
/// 
/// Called from JavaScript after the worker receives the 'init' message with
/// WASM memory and buffer pointers.
#[wasm_bindgen(js_name = workerInit)]
pub fn worker_init(
    ring_buffer_ptr: usize,
    regulator_ptr: usize,
    buffer_size: usize,
    channels: u8,
) {
    WORKER_STATE.with(|state| {
        state.borrow_mut().configure(ring_buffer_ptr, regulator_ptr, buffer_size, channels);
    });
}

/// Connect to a WebTransport server and start send/receive loops
/// 
/// This is an async function that:
/// 1. Establishes a WebTransport connection using browser's native API
/// 2. Starts parallel send and receive loops
/// 3. Returns when connection is closed or error occurs
#[wasm_bindgen(js_name = workerConnect)]
pub async fn worker_connect(server_url: String) -> Result<(), JsValue> {
    // Mark as running
    WORKER_STATE.with(|state| {
        state.borrow().start();
    });

    // Create WebTransport using browser's native API
    let transport = web_sys::WebTransport::new(&server_url)
        .map_err(|e| {
            web_sys::console::error_1(&format!("[WebTransport Worker] ❌ Failed to create WebTransport: {:?}", e).into());
            JsValue::from_str(&format!("Failed to create WebTransport: {:?}", e))
        })?;
    
    // Wait for connection to be ready
    JsFuture::from(transport.ready())
        .await
        .map_err(|e| {
            web_sys::console::error_1(&format!(
                "[WebTransport Worker] ❌ Connection failed: {:?}",
                e
            ).into());
            web_sys::console::error_1(&"[WebTransport Worker] 💡 Check: Is server listening on UDP 4464? Valid TLS cert? HTTP/3 enabled?".into());
            JsValue::from_str(&format!("WebTransport connection failed: {:?}", e))
        })?;
    
    // Wrap transport in Rc<RefCell> for shared ownership
    let transport = Rc::new(RefCell::new(transport));

    // Clone for each loop
    let transport_send = transport.clone();
    let transport_recv = transport.clone();

    // Start send and receive loops as concurrent tasks    
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = send_loop(transport_send).await {
            web_sys::console::error_1(&format!("[WebTransport Worker] Send loop error: {:?}", e).into());
        }
    });
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = receive_loop(transport_recv).await {
            web_sys::console::error_1(&format!("[WebTransport Worker] Receive loop error: {:?}", e).into());
        }
    });
    
    Ok(())
}

/// Disconnect from the server and stop all loops
#[wasm_bindgen(js_name = workerDisconnect)]
pub fn worker_disconnect() {
    WORKER_STATE.with(|state| {
        state.borrow().stop();
    });
}

/// Get current worker statistics
#[wasm_bindgen(js_name = workerGetStats)]
pub fn worker_get_stats() -> WebTransportWorkerStats {
    STATS.with(|stats| stats.borrow().clone())
}

/// Send loop: reads from RingBuffer, sends QUIC datagrams
/// 
/// This loop runs continuously while connected, reading audio packets from
/// the RingBuffer and sending them as unreliable QUIC datagrams via the browser's API.
/// 
/// ## Event-Driven Wake-Up
/// 
/// When no data is available, the loop uses `Atomics.wait()` to sleep efficiently:
/// - AudioWorklet writes to RingBuffer and calls `Atomics.notify()`
/// - This worker wakes up immediately (microsecond precision)
/// - Zero CPU usage while idle
/// - No setTimeout imprecision or 4ms minimum delays
async fn send_loop(transport: Rc<RefCell<web_sys::WebTransport>>) -> Result<(), JsValue> {
    // Get the writable datagram stream
    let datagrams = transport.borrow().datagrams();
    let writable = datagrams.writable();
    
    // get_writer() returns Result<WritableStreamDefaultWriter, JsValue>
    let writer = writable.get_writer()
        .map_err(|e| {
            web_sys::console::error_1(&format!("[WebTransport Worker] ❌ Failed to get datagram writer: {:?}", e).into());
            e
        })?;

    loop {
        // Check if we should stop
        let running = WORKER_STATE.with(|state| state.borrow().is_running());
        if !running {
            break;
        }

        // Try to read from ring buffer
        let packet_data = WORKER_STATE.with(|state| {
            let state = state.borrow();
            
            if state.ring_buffer_ptr.is_null() {
                return None;
            }

            // Safety: pointer is valid and RingBuffer uses atomics
            let ring_buffer = unsafe { &mut *state.ring_buffer_ptr };
            
            let samples_needed = (state.buffer_size * state.channels as usize) as u32;
            if ring_buffer.available() < samples_needed {
                return None;
            }

            // Read audio samples
            let mut audio_buffer = state.audio_buffer.borrow_mut();
            if !ring_buffer.read(&mut audio_buffer) {
                return None;
            }

            // Serialize packet
            let seq = state.sequence_number.fetch_add(1, Ordering::Relaxed);
            let ts = state.timestamp.fetch_add(state.buffer_size as u64, Ordering::Relaxed);
            
            let mut packet_buffer = state.packet_buffer.borrow_mut();
            match AudioPacket::serialize_samples_into(
                seq,
                ts,
                &audio_buffer,
                state.channels,
                &mut packet_buffer,
            ) {
                Ok(bytes_written) => {
                    // Convert to Uint8Array for browser API
                    let array = Uint8Array::new_with_length(bytes_written as u32);
                    array.copy_from(&packet_buffer[..bytes_written]);
                    Some((array, bytes_written))
                }
                Err(e) => {
                    web_sys::console::error_1(&format!(
                        "[WebTransport Worker] Serialize error: {:?}",
                        e
                    ).into());
                    None
                }
            }
        });

        if let Some((data, data_len)) = packet_data {
            // Send as unreliable datagram using browser API
            match JsFuture::from(writer.write_with_chunk(&data)).await {
                Ok(_) => {
                    STATS.with(|stats| {
                        let mut s = stats.borrow_mut();
                        s.packets_sent += 1;
                        s.bytes_sent += data_len as u64;
                    });
                }
                Err(e) => {
                    STATS.with(|stats| {
                        stats.borrow_mut().send_errors += 1;
                    });
                    
                    // Check if this is a connection error
                    let error_str = format!("{:?}", e);
                    if error_str.contains("Connection lost") || error_str.contains("WebTransportError") {
                        web_sys::console::error_1(&format!(
                            "[WebTransport Worker] ❌ Connection lost in send loop: {:?}",
                            e
                        ).into());
                        
                        // Stop the worker
                        WORKER_STATE.with(|state| {
                            state.borrow().stop();
                        });
                        
                        // Notify main thread
                        let error_obj = Object::new();
                        let _ = Reflect::set(&error_obj, &"type".into(), &"error".into());
                        let _ = Reflect::set(&error_obj, &"error".into(), &"Connection lost".into());
                        post_message_to_main(&error_obj.into());
                        
                        break;
                    }
                    
                    // For other errors, just log but don't fail - datagrams can be lost
                    web_sys::console::warn_1(&format!(
                        "[WebTransport Worker] Datagram send error: {:?}",
                        e
                    ).into());
                }
            }
        } else {
            // No data available - wait for notification from AudioWorklet
            // The AudioWorklet calls Atomics.notify() after writing to the ring buffer
            let int32_array = WORKER_STATE.with(|state| {
                let state = state.borrow();
                // Clone the Int32Array so we can use it outside the closure
                let result = state.has_data_int32_array.borrow().clone();
                result
            });
            
            // Wait for Atomics.notify() from the AudioWorklet
            // This is event-driven with microsecond precision and zero CPU usage when idle
            if let Some(array) = int32_array {
                // Wait with a short timeout to allow responsive shutdown
                // wait() params: array, index, expected_value, timeout_ms
                // Returns: "ok" (notified), "not-equal" (value changed), "timed-out"
                let _result = js_sys::Atomics::wait_with_timeout(
                    &array,
                    0,    // index in the Int32Array
                    0,    // expected value (wait if flag is 0, wake when it becomes 1)
                    1.0   // 1ms timeout for responsive shutdown (zero CPU while waiting)
                );
                // Note: We don't need to check the result - if we're notified or timeout,
                // we'll loop back and try to read. The ring buffer's read() will clear
                // the flag back to 0 if there's no more data.
            }
        }
    }

    Ok(())
}

/// Receive loop: receives QUIC datagrams, writes to Regulator
/// 
/// This loop runs continuously while connected, receiving audio packets as
/// unreliable QUIC datagrams via the browser's API and writing them to the Regulator.
async fn receive_loop(transport: Rc<RefCell<web_sys::WebTransport>>) -> Result<(), JsValue> {
    // Get the readable datagram stream
    let datagrams = transport.borrow().datagrams();
    let readable = datagrams.readable();
    
    // get_reader() returns Object, cast to ReadableStreamDefaultReader
    let reader: web_sys::ReadableStreamDefaultReader = readable.get_reader().unchecked_into();

    loop {
        // Check if we should stop
        let running = WORKER_STATE.with(|state| state.borrow().is_running());
        if !running {
            break;
        }

        // Read next datagram (event-driven, not polling!)
        let read_result = JsFuture::from(reader.read()).await;
        
        match read_result {
            Ok(result) => {
                // Check if stream is done
                let done = Reflect::get(&result, &"done".into())
                    .ok()
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                
                if done {
                    break;
                }

                // Get the value (Uint8Array)
                let value = Reflect::get(&result, &"value".into())?;
                let array = value.dyn_into::<Uint8Array>()
                    .map_err(|e| {
                        web_sys::console::error_1(&format!("[WebTransport Worker] ❌ Expected Uint8Array, got: {:?}", e).into());
                        JsValue::from_str("Expected Uint8Array")
                    })?;
                
                let data_len = array.length() as usize;

                STATS.with(|stats| {
                    let mut s = stats.borrow_mut();
                    s.packets_received += 1;
                    s.bytes_received += data_len as u64;
                });

                // Convert Uint8Array to Vec<u8>
                let mut data = vec![0u8; data_len];
                array.copy_to(&mut data);

                // Deserialize and push to regulator
                WORKER_STATE.with(|state| {
                    let state = state.borrow();
                    
                    if state.regulator_ptr.is_null() {
                        return;
                    }

                    // Safety: pointer is valid and Regulator uses atomics
                    let regulator = unsafe { &mut *state.regulator_ptr };

                    // Deserialize packet
                    let mut samples = state.samples_buffer.borrow_mut();
                    match AudioPacket::deserialize_into(&data, &mut samples) {
                        Ok(header) => {
                            regulator.push(header.sequence_number, &samples);                            
                        }
                        Err(e) => {
                            STATS.with(|stats| {
                                let mut s = stats.borrow_mut();
                                s.receive_errors += 1;
                                
                                // If we're getting too many errors in a row, the connection may be dead
                                // Log but continue - single corrupted packets are expected in network conditions
                                if s.receive_errors > 10 && s.packets_received > 0 {
                                    let error_rate = s.receive_errors as f64 / s.packets_received as f64;
                                    if error_rate > 0.5 {
                                        web_sys::console::warn_1(&format!(
                                            "[WebTransport Worker] ⚠️ High error rate ({:.1}%), deserialization error: {:?}",
                                            error_rate * 100.0, e
                                        ).into());
                                    }
                                } else {
                                    web_sys::console::warn_1(&format!(
                                        "[WebTransport Worker] ⚠️ Deserialize error: {:?}",
                                        e
                                    ).into());
                                }
                            });
                        }
                    }
                });
            }
            Err(e) => {
                // Check if this is a normal close or an error
                let running = WORKER_STATE.with(|state| state.borrow().is_running());
                if !running {
                    // Normal shutdown
                    break;
                }
                
                web_sys::console::error_1(&format!(
                    "[WebTransport Worker] ❌ Datagram receive error: {:?}",
                    e
                ).into());
                STATS.with(|stats| {
                    stats.borrow_mut().receive_errors += 1;
                });
                
                // Stop the worker
                WORKER_STATE.with(|state| {
                    state.borrow().stop();
                });
                
                // Notify main thread
                let error_obj = Object::new();
                let _ = Reflect::set(&error_obj, &"type".into(), &"error".into());
                let _ = Reflect::set(&error_obj, &"error".into(), &"Connection lost".into());
                post_message_to_main(&error_obj.into());
                
                // For connection errors, break the loop
                break;
            }
        }
    }

    Ok(())
}

/// Entry point for the worker when loaded as a module
/// 
/// Sets up message handling for communication with the main thread.
#[wasm_bindgen(start)]
pub fn worker_main() {
    // Set up panic hook for better error messages
    console_error_panic_hook::set_once();
    
    // Note: Message handling is set up from JavaScript glue code
    // This function just ensures the WASM module is ready
}

/// Handle incoming message from main thread
/// 
/// Message types:
/// - { type: "init", ringBufferPtr, regulatorPtr, bufferSize, channels }
/// - { type: "connect", serverUrl }
/// - { type: "disconnect" }
/// - { type: "getStats" }
#[wasm_bindgen(js_name = handleWorkerMessage)]
pub fn handle_worker_message(msg: JsValue) -> js_sys::Promise {
    let msg_type = Reflect::get(&msg, &"type".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default();

    match msg_type.as_str() {
        "init" => {
            let ring_ptr = Reflect::get(&msg, &"ringBufferPtr".into())
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as usize;
            let reg_ptr = Reflect::get(&msg, &"regulatorPtr".into())
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as usize;
            let buffer_size = Reflect::get(&msg, &"bufferSize".into())
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(128.0) as usize;
            let channels = Reflect::get(&msg, &"channels".into())
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(2.0) as u8;

            worker_init(ring_ptr, reg_ptr, buffer_size, channels);
            
            // Post ready message to main thread
            post_message_to_main(&JsValue::from_str("ready"));

            js_sys::Promise::resolve(&JsValue::from_str("ready"))
        }
        "connect" => {
            let server_url = Reflect::get(&msg, &"serverUrl".into())
                .ok()
                .and_then(|v| v.as_string())
                .unwrap_or_default();

            // Return a promise that resolves when connected
            wasm_bindgen_futures::future_to_promise(async move {
                worker_connect(server_url).await?;
                
                // Post connected message to main thread
                post_message_to_main(&JsValue::from_str("connected"));
                
                Ok(JsValue::from_str("connected"))
            })
        }
        "disconnect" => {
            worker_disconnect();
            
            // Post disconnected message to main thread
            post_message_to_main(&JsValue::from_str("disconnected"));
            
            js_sys::Promise::resolve(&JsValue::from_str("disconnected"))
        }
        "getStats" => {
            let stats = worker_get_stats();
            let obj = Object::new();
            let _ = Reflect::set(&obj, &"type".into(), &"stats".into());
            let _ = Reflect::set(&obj, &"packetsSent".into(), &JsValue::from_f64(stats.packets_sent as f64));
            let _ = Reflect::set(&obj, &"packetsReceived".into(), &JsValue::from_f64(stats.packets_received as f64));
            let _ = Reflect::set(&obj, &"bytesSent".into(), &JsValue::from_f64(stats.bytes_sent as f64));
            let _ = Reflect::set(&obj, &"bytesReceived".into(), &JsValue::from_f64(stats.bytes_received as f64));
            let _ = Reflect::set(&obj, &"sendErrors".into(), &JsValue::from_f64(stats.send_errors as f64));
            let _ = Reflect::set(&obj, &"receiveErrors".into(), &JsValue::from_f64(stats.receive_errors as f64));
            
            // Post stats to main thread
            post_message_to_main(&obj);
            
            js_sys::Promise::resolve(&obj)
        }
        _ => {
            let error_msg = format!("Unknown message type: {}", msg_type);
            web_sys::console::warn_1(&format!("[WebTransport Worker] {}", error_msg).into());
            
            // Post error to main thread
            let error_obj = Object::new();
            let _ = Reflect::set(&error_obj, &"type".into(), &"error".into());
            let _ = Reflect::set(&error_obj, &"error".into(), &JsValue::from_str(&error_msg));
            post_message_to_main(&error_obj);
            
            js_sys::Promise::reject(&JsValue::from_str(&error_msg))
        }
    }
}
