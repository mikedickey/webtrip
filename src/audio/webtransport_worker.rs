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
use crate::audio::protocol::{AudioPacket, make_exit_packet, HEADER_SIZE, ProtocolError, PacketHeader};

/// Number of interleaved samples in one outgoing audio packet for the given
/// buffer size (samples per channel) and channel count.
///
/// This is the threshold the send loop compares the ring buffer's fill level
/// against. Extracted as a pure function so the framing math is the single
/// source of truth and testable natively.
fn samples_per_packet(buffer_size: usize, channels: u8) -> u32 {
    (buffer_size * channels as usize) as u32
}

/// Whether the send loop should assemble and send a packet now, or sleep until
/// more audio is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SendDecision {
    /// The ring buffer holds at least a full packet's worth of samples.
    Process,
    /// Not enough samples buffered yet — wait for the AudioWorklet to produce more.
    Wait,
}

/// Decide whether the send loop should process or wait.
///
/// Processes as soon as the ring buffer holds at least one full packet
/// (`available >= needed`), otherwise waits. Extracted so the "process vs wait"
/// decision can be tested without a ring buffer or transport.
fn send_decision(available_samples: u32, samples_needed: u32) -> SendDecision {
    if available_samples >= samples_needed {
        SendDecision::Process
    } else {
        SendDecision::Wait
    }
}

/// Deserialize a received QUIC datagram into audio samples, returning the
/// packet's sequence number.
///
/// Reuses [`AudioPacket::deserialize_into`] (the single source of truth for the
/// JackTrip wire format) to fill `samples` and extract the sequence number that
/// the receive loop hands to the regulator. Returns the same [`ProtocolError`]
/// as the underlying parser for malformed or short datagrams.
fn deserialize_datagram(data: &[u8], samples: &mut Vec<f32>) -> Result<u16, ProtocolError> {
    let header: PacketHeader = AudioPacket::deserialize_into(data, samples)?;
    Ok(header.sequence_number)
}

/// Post a message to the main thread from the worker
fn post_message_to_main(msg: &JsValue) {
    // Get the global worker scope
    let global = js_sys::global().unchecked_into::<web_sys::DedicatedWorkerGlobalScope>();
    let _ = global.post_message(msg);
}

fn post_error_to_main(msg: &str) {
    let obj = Object::new();
    let _ = Reflect::set(&obj, &"type".into(), &"error".into());
    let _ = Reflect::set(&obj, &"error".into(), &msg.into());
    post_message_to_main(&obj.into());
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
///
/// Sets `running = false` and then wakes any thread currently sleeping in
/// `Atomics.waitAsync` on the ring-buffer's has_data flag, so the send loop
/// notices the shutdown immediately (µs-scale) without relying on a polling
/// timeout.
#[wasm_bindgen(js_name = workerDisconnect)]
pub fn worker_disconnect() {
    // Clone out the Int32Array first so we don't hold the RefCell borrow across
    // the notify call (which also avoids lifetime issues with the inner borrow).
    let array = WORKER_STATE.with(|state| {
        let state = state.borrow();
        state.stop();
        let cloned = state.has_data_int32_array.borrow().clone();
        cloned
    });
    // Wake any Atomics.waitAsync sleeper on the has_data flag. The value of the
    // flag is unchanged; we're only sending a wake-up signal so the send loop's
    // awaited Promise resolves and it can observe running=false.
    if let Some(array) = array {
        let _ = js_sys::Atomics::notify(&array, 0);
    }
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
/// When no data is available, the loop sleeps via `Atomics.waitAsync()`:
/// - AudioWorklet writes to RingBuffer and calls `Atomics.notify()`
/// - This worker wakes up immediately (microsecond precision)
/// - Zero CPU usage while idle
/// - The async wait properly yields the worker's JS event loop between
///   iterations, so control messages (e.g. `{type:"disconnect"}`) can be
///   dispatched promptly. `worker_disconnect()` additionally calls
///   `Atomics.notify` on the same flag to unblock the wait on shutdown
///   even if the AudioWorklet has stopped producing new data.
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

    // Tracks whether the loop exited due to a graceful worker_disconnect() call (as opposed
    // to a connection error). Only a graceful exit warrants sending an exit packet.
    let mut graceful_exit = false;

    loop {
        // Check if we should stop
        let running = WORKER_STATE.with(|state| state.borrow().is_running());
        if !running {
            graceful_exit = true;
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
            
            let samples_needed = samples_per_packet(state.buffer_size, state.channels);
            if send_decision(ring_buffer.available(), samples_needed) == SendDecision::Wait {
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
                        post_error_to_main("Connection lost");
                        
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
            // No data available. Sleep until the AudioWorklet signals new data
            // via `Atomics.notify`, or `worker_disconnect()` wakes us.
            //
            // We use `Atomics.waitAsync` (the async counterpart of `Atomics.wait`)
            // to get the best of both worlds:
            //   - µs-scale wake-up on `Atomics.notify` (same as sync `Atomics.wait`)
            //   - Zero idle CPU usage
            //   - The `.await` on the returned Promise properly returns `Pending`
            //     from `poll()`, handing control back to the JS event loop so
            //     queued macrotasks (like `onmessage` with a disconnect request)
            //     can be delivered. A plain sync `Atomics.wait` would not yield,
            //     starving the worker's event loop.
            //
            // `wait_async(array, 0, 0)` means: "sleep while array[0] == 0". When
            // the AudioWorklet writes new data it sets the flag to 1 and calls
            // `Atomics.notify`; `worker_disconnect()` also calls `Atomics.notify`
            // to wake us immediately on shutdown.
            let int32_array = WORKER_STATE.with(|state| {
                state.borrow().has_data_int32_array.borrow().clone()
            });
            if let Some(array) = int32_array {
                match js_sys::Atomics::wait_async(&array, 0, 0) {
                    Ok(result) => {
                        // Result shape: { async: bool, value: Promise | String }.
                        // If async=false, the value already changed — no wait needed.
                        let is_async = Reflect::get(&result, &"async".into())
                            .ok()
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        if is_async {
                            if let Ok(promise_val) = Reflect::get(&result, &"value".into()) {
                                if let Ok(promise) = promise_val.dyn_into::<js_sys::Promise>() {
                                    let _ = JsFuture::from(promise).await;
                                }
                            }
                        }
                    }
                    Err(_) => {
                        // `waitAsync` not available on this agent — should be
                        // impossible for WebTransport-capable browsers, but fall
                        // back to a microtask yield so we at least make progress
                        // and don't starve the event loop.
                        let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::NULL)).await;
                    }
                }
            } else {
                // has_data_int32_array not initialized yet — yield and retry.
                let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::NULL)).await;
            }
        }
    }

    // On graceful disconnect: send two JackTrip exit packets (63-byte control packets,
    // all 0xFF) so the hub reclaims the client slot immediately, then notify the
    // main thread so it can terminate this worker.
    if graceful_exit {
        let exit_bytes = make_exit_packet();
        let array = Uint8Array::new_with_length(exit_bytes.len() as u32);
        array.copy_from(&exit_bytes);
        let _ = JsFuture::from(writer.write_with_chunk(&array)).await; // best-effort
        let _ = JsFuture::from(writer.write_with_chunk(&array)).await; // best-effort

        post_message_to_main(&JsValue::from_str("disconnected"));
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
                    match deserialize_datagram(&data, &mut samples) {
                        Ok(sequence_number) => {
                            regulator.push(sequence_number, &samples);                            
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
                post_error_to_main("Connection lost");
                
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
            // Signal the send loop to stop. It will send the JackTrip exit packet
            // and post "disconnected" to the main thread before returning.
            worker_disconnect();

            js_sys::Promise::resolve(&JsValue::from_str("ok"))
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
            
            js_sys::Promise::resolve(&JsValue::from(obj))
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

#[cfg(test)]
mod tests {
    use super::*;

    // --- samples_per_packet ---

    #[test]
    fn samples_per_packet_accounts_for_channels() {
        assert_eq!(samples_per_packet(128, 1), 128);
        assert_eq!(samples_per_packet(128, 2), 256);
        assert_eq!(samples_per_packet(64, 2), 128);
    }

    // --- send_decision ("process vs wait") ---

    #[test]
    fn send_decision_processes_when_enough_samples() {
        let needed = samples_per_packet(128, 2); // 256
        assert_eq!(send_decision(needed, needed), SendDecision::Process);
        assert_eq!(send_decision(needed + 1, needed), SendDecision::Process);
    }

    #[test]
    fn send_decision_waits_when_not_enough_samples() {
        let needed = samples_per_packet(128, 2); // 256
        assert_eq!(send_decision(needed - 1, needed), SendDecision::Wait);
        assert_eq!(send_decision(0, needed), SendDecision::Wait);
    }

    // --- deserialize_datagram (reuses AudioPacket wire protocol) ---

    #[test]
    fn deserialize_datagram_returns_sequence_and_samples() {
        // Build a valid datagram using the shared AudioPacket serializer so we
        // never duplicate the wire format here.
        let samples: Vec<f32> = (0..128).map(|i| (i as f32) / 128.0).collect();
        let packet = AudioPacket::mono(42, 1000, samples.clone());
        let datagram = packet.serialize().unwrap();

        let mut out = Vec::new();
        let sequence = deserialize_datagram(&datagram, &mut out).unwrap();

        assert_eq!(sequence, 42);
        assert_eq!(out.len(), samples.len());
        for (a, b) in samples.iter().zip(out.iter()) {
            assert!((a - b).abs() < 1e-4, "sample mismatch: {a} vs {b}");
        }
    }

    #[test]
    fn deserialize_datagram_stereo_sequence() {
        let samples: Vec<f32> = (0..256).map(|i| (i as f32) / 256.0).collect();
        let packet = AudioPacket::stereo(7, 0, samples.clone());
        let datagram = packet.serialize().unwrap();

        let mut out = Vec::new();
        let sequence = deserialize_datagram(&datagram, &mut out).unwrap();

        assert_eq!(sequence, 7);
        assert_eq!(out.len(), samples.len());
    }

    #[test]
    fn deserialize_datagram_rejects_short_buffer() {
        // A datagram shorter than the 16-byte header must be rejected.
        let mut out = Vec::new();
        let err = deserialize_datagram(&[0u8; 4], &mut out).unwrap_err();
        assert_eq!(err, ProtocolError::BufferTooSmall);
    }

    #[test]
    fn deserialize_datagram_rejects_truncated_audio() {
        // A full, valid header but with the audio payload truncated must also be
        // rejected (header parses, but the declared samples don't fit).
        let samples: Vec<f32> = (0..128).map(|i| (i as f32) / 128.0).collect();
        let packet = AudioPacket::mono(1, 0, samples);
        let datagram = packet.serialize().unwrap();

        let truncated = &datagram[..HEADER_SIZE + 2];
        let mut out = Vec::new();
        let err = deserialize_datagram(truncated, &mut out).unwrap_err();
        assert_eq!(err, ProtocolError::BufferTooSmall);
    }

    // --- WorkerState atomic running flag ---

    #[test]
    fn worker_state_start_stop_is_running() {
        let state = WorkerState::new();
        // Fresh state is not running.
        assert!(!state.is_running());

        state.start();
        assert!(state.is_running());

        state.stop();
        assert!(!state.is_running());

        // Idempotent: starting twice keeps it running.
        state.start();
        state.start();
        assert!(state.is_running());
    }

    // ── Browser tests (worker-side #[wasm_bindgen] entry points) ──────────────
    //
    // These call the worker entry points directly in headless Chrome via
    // `npm run test:wasm`. The per-binary `run_in_browser` opt-in lives once in
    // `crate::test_support`. `post_message_to_main()` is private and can't be
    // spied on (and in the test's window context its DedicatedWorkerGlobalScope
    // post is a swallowed no-op), so routing is asserted via each entry point's
    // returned `Promise`/value. The server-dependent paths — `worker_connect()`,
    // `send_loop()`, `receive_loop()` — need a live HTTP/3 server and the
    // shared-memory ring-buffer harness, so they are intentionally not covered.

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;

    /// Build a plain JS message object (`{ key: value, … }`) for the worker
    /// entry points, mirroring the postMessage payloads the main thread sends.
    #[cfg(target_arch = "wasm32")]
    fn worker_message(fields: &[(&str, JsValue)]) -> JsValue {
        let obj = Object::new();
        for (key, value) in fields {
            Reflect::set(&obj, &(*key).into(), value).expect("Reflect::set on a fresh object");
        }
        obj.into()
    }

    /// `worker_init()` followed by `worker_get_stats()` returns zeroed stats —
    /// proves init ran and left the counters at their defaults (no packets have
    /// flowed). Null buffer pointers are safe: `configure()` guards the
    /// `Int32Array` setup on a null ring-buffer pointer and the loops never run.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn worker_init_then_stats_are_zeroed() {
        worker_init(0, 0, 128, 2);

        let stats = worker_get_stats();
        assert_eq!(stats.packets_sent, 0);
        assert_eq!(stats.packets_received, 0);
        assert_eq!(stats.bytes_sent, 0);
        assert_eq!(stats.bytes_received, 0);
        assert_eq!(stats.send_errors, 0);
        assert_eq!(stats.receive_errors, 0);
    }

    /// `handle_worker_message()` with an `"init"` payload routes to `worker_init`
    /// and resolves its `Promise` with `"ready"`.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn handle_worker_message_init_resolves_ready() {
        let msg = worker_message(&[
            ("type", JsValue::from_str("init")),
            ("ringBufferPtr", JsValue::from_f64(0.0)),
            ("regulatorPtr", JsValue::from_f64(0.0)),
            ("bufferSize", JsValue::from_f64(128.0)),
            ("channels", JsValue::from_f64(2.0)),
        ]);

        let result = JsFuture::from(handle_worker_message(msg))
            .await
            .expect("init message routing should resolve");
        assert_eq!(result.as_string().as_deref(), Some("ready"));
    }

    /// `handle_worker_message()` with a `"getStats"` payload routes to
    /// `worker_get_stats` and resolves with the stats object (zeroed here).
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn handle_worker_message_get_stats_resolves_stats_object() {
        worker_init(0, 0, 128, 2);

        let msg = worker_message(&[("type", JsValue::from_str("getStats"))]);
        let result = JsFuture::from(handle_worker_message(msg))
            .await
            .expect("getStats message routing should resolve");

        assert_eq!(
            Reflect::get(&result, &"type".into())
                .unwrap()
                .as_string()
                .as_deref(),
            Some("stats")
        );
        // Assert every numeric field of the JS stats mapping (not just a subset)
        // so a regression in any single `Reflect::set` in the getStats route is
        // caught. All are zero because no packets have flowed.
        for field in [
            "packetsSent",
            "packetsReceived",
            "bytesSent",
            "bytesReceived",
            "sendErrors",
            "receiveErrors",
        ] {
            assert_eq!(
                Reflect::get(&result, &field.into()).unwrap().as_f64(),
                Some(0.0),
                "stats field {field} should be present and zeroed"
            );
        }
    }

    /// `handle_worker_message()` with a `"disconnect"` payload routes to
    /// `worker_disconnect` and resolves with `"ok"`. With null buffers the
    /// `has_data` `Int32Array` is `None`, so the `Atomics.notify` is skipped.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn handle_worker_message_disconnect_resolves_ok() {
        worker_init(0, 0, 128, 2);

        let msg = worker_message(&[("type", JsValue::from_str("disconnect"))]);
        let result = JsFuture::from(handle_worker_message(msg))
            .await
            .expect("disconnect message routing should resolve");
        assert_eq!(result.as_string().as_deref(), Some("ok"));
    }

    /// An unrecognized message type rejects the returned `Promise` with a
    /// descriptive error rather than silently succeeding.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn handle_worker_message_unknown_type_rejects() {
        let msg = worker_message(&[("type", JsValue::from_str("bogus"))]);

        let err = JsFuture::from(handle_worker_message(msg))
            .await
            .expect_err("an unknown message type must reject");
        assert!(
            err.as_string().unwrap_or_default().contains("Unknown message type"),
            "rejection should describe the unknown message type, got: {err:?}"
        );
    }
}
