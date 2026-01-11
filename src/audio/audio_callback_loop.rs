//! Audio Callback Loop
//!
//! Provides a timing mechanism synchronized with the audio worklet's process() callback.
//! When the worklet processes audio, it signals via Atomics.notify, and this loop
//! wakes up to trigger transport packet processing.
//!
//! ## Event Notification Strategy
//!
//! Uses Atomics.waitAsync when available for zero-CPU idle behavior (Chrome 87+, Firefox 89+, Safari 16.4+),
//! or falls back to postMessage for older browsers (primarily Safari 15.2-16.3).
//!
//! **Important:** Both modes REQUIRE SharedArrayBuffer for the underlying buffer access.
//! The postMessage fallback only affects the event notification mechanism, not the buffer itself.
//! SharedArrayBuffer is mandatory for:
//! - WASM linear memory (compiled with --shared-memory)
//! - Atomic operations in RingBuffer and JitterBuffer
//! - Cross-thread buffer synchronization
//!
//! The fallback provides compatibility for the ~15 month gap (Dec 2021 - Mar 2023) where
//! Safari had SharedArrayBuffer but not yet Atomics.waitAsync.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Check if Atomics.waitAsync is supported by the browser
/// 
/// Requires both Atomics.waitAsync AND SharedArrayBuffer to be available.
/// 
/// Note: This checks for the *optimized* event notification path. If this returns false,
/// we fall back to postMessage notifications, but SharedArrayBuffer is still required
/// for the actual buffer access.
#[wasm_bindgen(js_name = hasAtomicsWaitAsync)]
pub fn has_atomics_wait_async() -> bool {
    let global = js_sys::global();
    
    // Check for Atomics.waitAsync
    let has_wait_async = if let Ok(atomics) = js_sys::Reflect::get(&global, &"Atomics".into()) {
        if let Ok(wait_async) = js_sys::Reflect::get(&atomics, &"waitAsync".into()) {
            !wait_async.is_undefined()
        } else {
            false
        }
    } else {
        false
    };
    
    // Check for SharedArrayBuffer
    let has_sab = if let Ok(sab) = js_sys::Reflect::get(&global, &"SharedArrayBuffer".into()) {
        !sab.is_undefined()
    } else {
        false
    };
    
    // Need BOTH to be available
    has_wait_async && has_sab
}

/// Start an audio-callback-driven loop using Atomics.waitAsync
/// 
/// This function spawns a JavaScript async loop that:
/// 1. Waits for the ring buffer flag to change (via Atomics.waitAsync)
/// 2. Calls the provided tick callback when woken (audio callback occurred)
/// 3. Returns a stop function to terminate the loop
/// 
/// # Arguments
/// * `memory` - WebAssembly memory containing the ring buffer flag
/// * `flag_ptr` - Pointer to the atomic flag (byte offset)
/// * `tick_callback` - Function to call when audio callback occurs
/// 
/// # Returns
/// A JavaScript function that stops the loop when called
#[wasm_bindgen(js_name = startAudioCallbackLoop)]
pub fn start_audio_callback_loop(
    memory: &JsValue,
    flag_ptr: usize,
    tick_callback: &js_sys::Function,
) -> Result<js_sys::Function, JsValue> {
    let flag_index = flag_ptr / 4; // Convert byte offset to Int32Array index
    
    // Create the async loop function
    let loop_fn = js_sys::Function::new_with_args(
        "memory,flagIndex,tickCallback",
        &format!(r#"
            let running = true;
            
            // Main loop
            (async () => {{
                while (running) {{
                    try {{
                        // Create Int32Array view (may need to refresh if memory grows)
                        const int32View = new Int32Array(memory.buffer);
                        
                        // Wait for flag to change from 0 (data available)
                        const result = Atomics.waitAsync(int32View, flagIndex, 0);
                        
                        if (result.async) {{
                            // Wait for notification
                            const waitResult = await result.value;
                            
                            if (waitResult === "timed-out") {{
                                // Shouldn't happen (no timeout set)
                                console.warn("Atomics.waitAsync timed out unexpectedly");
                                continue;
                            }}
                        }}
                        
                        // Data is ready - call tick callback
                        if (running) {{
                            tickCallback();
                        }}
                    }} catch (error) {{
                        console.error("Error in event loop:", error);
                        // Small delay to prevent tight error loop
                        await new Promise(resolve => setTimeout(resolve, 100));
                    }}
                }}
            }})();
            
            // Return stop function
            return () => {{
                running = false;
                
                // Wake up the loop if it's sleeping
                const int32View = new Int32Array(memory.buffer);
                Atomics.notify(int32View, flagIndex);
            }};
        "#)
    );
    
    // Call the loop function to start it
    let stop_fn = loop_fn.call3(
        &JsValue::NULL,
        memory,
        &JsValue::from_f64(flag_index as f64),
        tick_callback,
    )?;
    
    Ok(stop_fn.dyn_into::<js_sys::Function>()?)
}

/// Audio callback loop manager
/// 
/// Manages the lifecycle of an audio-callback-driven loop.
/// This loop is triggered whenever the audio worklet's process() callback runs,
/// allowing transport layers to send/receive packets in sync with audio processing.
/// 
/// Automatically chooses between Atomics.waitAsync (optimized) and postMessage (fallback)
/// based on browser support.
/// 
/// ## Browser Compatibility
/// 
/// - **Atomics.waitAsync mode** (preferred): Chrome 87+, Firefox 89+, Safari 16.4+
///   - Zero CPU when idle, immediate wake-up (<0.1ms)
/// - **postMessage mode** (fallback): Safari 15.2-16.3, Chrome 68-86
///   - Higher CPU usage (~2-3x), still requires SharedArrayBuffer for buffers
/// 
/// Both modes require SharedArrayBuffer and Cross-Origin Isolation (COOP/COEP headers).
pub struct AudioCallbackLoop {
    /// Stop function (for Atomics.waitAsync mode)
    stop_fn: Option<js_sys::Function>,
    
    /// Message handler closure (for postMessage mode)
    message_closure: Option<Closure<dyn FnMut(web_sys::MessageEvent)>>,
}

impl AudioCallbackLoop {
    /// Create a new audio callback loop (not started)
    pub fn new() -> Self {
        Self {
            stop_fn: None,
            message_closure: None,
        }
    }
    
    /// Start the audio callback loop with Atomics.waitAsync
    /// 
    /// Returns true if started successfully, false if not supported
    pub fn start_with_atomics(
        &mut self,
        memory: &JsValue,
        flag_ptr: usize,
        tick_callback: js_sys::Function,
    ) -> Result<bool, JsValue> {
        if !has_atomics_wait_async() {
            return Ok(false);
        }
        
        // Start the async loop
        let stop_fn = start_audio_callback_loop(memory, flag_ptr, &tick_callback)?;
        self.stop_fn = Some(stop_fn);
        
        web_sys::console::debug_1(&"✅ Audio callback loop started (Atomics.waitAsync)".into());
        Ok(true)
    }
    
    /// Start the audio callback loop with postMessage fallback
    /// 
    /// This is used when Atomics.waitAsync is not available (primarily Safari 15.2-16.3).
    /// The AudioWorklet will post messages when audio is processed.
    /// 
    /// Note: This still requires SharedArrayBuffer for the actual buffer access - it only
    /// changes the event notification mechanism from Atomics.waitAsync to postMessage.
    pub fn start_with_postmessage(
        &mut self,
        port: &web_sys::MessagePort,
        tick_callback: js_sys::Function,
    ) -> Result<(), JsValue> {
        // Set up message handler
        let closure = Closure::wrap(Box::new(move |_event: web_sys::MessageEvent| {
            let _ = tick_callback.call0(&JsValue::NULL);
        }) as Box<dyn FnMut(web_sys::MessageEvent)>);
        
        port.set_onmessage(Some(closure.as_ref().unchecked_ref()));
        self.message_closure = Some(closure);
        
        web_sys::console::debug_1(&"✅ Audio callback loop started (postMessage fallback)".into());
        Ok(())
    }
    
    /// Stop the audio callback loop
    pub fn stop(&mut self) {
        // Stop Atomics.waitAsync loop
        if let Some(stop_fn) = self.stop_fn.take() {
            let _ = stop_fn.call0(&JsValue::NULL);
            web_sys::console::debug_1(&"Audio callback loop stopped (Atomics.waitAsync)".into());
        }
        
        // Clean up postMessage handler
        if self.message_closure.take().is_some() {
            web_sys::console::debug_1(&"Audio callback loop stopped (postMessage)".into());
        }
    }
    
    /// Check if the loop is running
    pub fn is_running(&self) -> bool {
        self.stop_fn.is_some() || self.message_closure.is_some()
    }
}

impl Default for AudioCallbackLoop {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for AudioCallbackLoop {
    fn drop(&mut self) {
        self.stop();
    }
}
