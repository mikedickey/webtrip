//! Audio Callback Loop
//!
//! Provides a timing mechanism synchronized with the audio worklet's process() callback.
//! When the worklet processes audio, it signals via Atomics.notify, and this loop
//! wakes up via Atomics.waitAsync to trigger transport packet processing.
//!
//! Requires Atomics.waitAsync and SharedArrayBuffer (Chrome 87+, Firefox 89+, Safari 16.4+).

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// Check if Atomics.waitAsync is supported by the browser
///
/// Requires both Atomics.waitAsync AND SharedArrayBuffer to be available.
/// Always returns `false` on non-WASM targets (native `cargo test` runs).
#[wasm_bindgen(js_name = hasAtomicsWaitAsync)]
pub fn has_atomics_wait_async() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        let global = js_sys::global();

        // Check for Atomics.waitAsync
        let has_wait_async =
            if let Ok(atomics) = js_sys::Reflect::get(&global, &"Atomics".into()) {
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

    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
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
/// Uses Atomics.waitAsync for zero-CPU idle behavior and immediate wake-up (<0.1ms).
/// Requires SharedArrayBuffer and Cross-Origin Isolation (COOP/COEP headers).
pub struct AudioCallbackLoop {
    /// Stop function returned by the Atomics.waitAsync loop
    stop_fn: Option<js_sys::Function>,
}

impl AudioCallbackLoop {
    /// Create a new audio callback loop (not started)
    pub fn new() -> Self {
        Self {
            stop_fn: None,
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
    
    /// Stop the audio callback loop
    pub fn stop(&mut self) {
        if let Some(stop_fn) = self.stop_fn.take() {
            let _ = stop_fn.call0(&JsValue::NULL);
            web_sys::console::debug_1(&"Audio callback loop stopped".into());
        }
    }
    
    /// Check if the loop is running
    pub fn is_running(&self) -> bool {
        self.stop_fn.is_some()
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

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // AudioCallbackLoop construction and running state
    // -----------------------------------------------------------------------

    #[test]
    fn new_loop_is_not_running() {
        let lp = AudioCallbackLoop::new();
        assert!(!lp.is_running());
    }

    #[test]
    fn default_loop_is_not_running() {
        let lp = AudioCallbackLoop::default();
        assert!(!lp.is_running());
    }

    #[test]
    fn stopped_loop_is_not_running() {
        let mut lp = AudioCallbackLoop::new();
        // stop() on a never-started loop must not panic and must leave it stopped
        lp.stop();
        assert!(!lp.is_running());
    }

    // -----------------------------------------------------------------------
    // has_atomics_wait_async on the native target
    // -----------------------------------------------------------------------

    /// On the native (`cargo test`) target there is no browser runtime, so
    /// `has_atomics_wait_async` must return `false` — confirming that the
    /// cfg guard is in place and that calling the function does not panic.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn has_atomics_returns_false_on_native() {
        assert!(!has_atomics_wait_async());
    }

    // ── Browser tests (web_sys / Atomics.waitAsync) ──────────────────────────
    //
    // Real-browser coverage run in headless Chrome via `npm run test:wasm`. The
    // per-binary `run_in_browser` opt-in lives once in `crate::test_support`.
    // These exercise the shared-memory build: T17 established that the
    // `wasm-pack`/`wasm-bindgen-test` harness serves a cross-origin-isolated
    // page with an imported shared `WebAssembly.Memory{shared:true}` (see the
    // flag-parity section of docs/WASM_TESTING.md), so `SharedArrayBuffer` and
    // `Atomics.notify` are available here without extra setup.

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;

    /// In the cross-origin-isolated harness (SharedArrayBuffer +
    /// Atomics.waitAsync both present) detection must report support — the
    /// inverse of the native check above.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn has_atomics_wait_async_true_in_browser() {
        assert!(
            has_atomics_wait_async(),
            "shared-memory harness must expose Atomics.waitAsync + SharedArrayBuffer"
        );
    }

    /// Smoke-test the `Atomics.waitAsync` wake-up path end to end: start the
    /// loop sleeping on a zeroed flag in shared wasm memory, then flip the flag
    /// and `Atomics.notify` it exactly the way the audio worklet does. The tick
    /// callback must fire. This depends on the shared-memory build — `notify`
    /// only works on a `SharedArrayBuffer`, which the harness supplies via the
    /// imported shared `WebAssembly.Memory`.
    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    async fn audio_callback_loop_fires_on_notify() {
        use std::cell::Cell;
        use std::rc::Rc;
        use std::sync::atomic::{AtomicI32, Ordering};
        use wasm_bindgen::closure::Closure;
        use wasm_bindgen::JsCast;

        assert!(
            has_atomics_wait_async(),
            "smoke test requires the shared-memory harness (Atomics.waitAsync + SAB)"
        );

        // A 4-byte-aligned flag living in wasm linear memory — the same kind of
        // location the RingBuffer's has-data flag occupies.
        let flag = Box::new(AtomicI32::new(0));
        let flag_ptr = (&*flag as *const AtomicI32) as usize;

        // Int32 view over the shared wasm memory for store/notify, using the
        // same byte-offset/4 index math the loop applies internally.
        let memory: js_sys::WebAssembly::Memory = wasm_bindgen::memory().unchecked_into();
        let int32 = js_sys::Int32Array::new(&memory.buffer());
        let flag_index = (flag_ptr / 4) as u32;

        // Tick callback: count invocations and reset the flag to 0 so the loop
        // sleeps again (otherwise waitAsync would return "not-equal" forever and
        // spin). Mirrors how the real tick drains the ring buffer.
        let count = Rc::new(Cell::new(0u32));
        let count_for_cb = count.clone();
        let flag_for_cb = flag_ptr;
        let cb = Closure::wrap(Box::new(move || {
            count_for_cb.set(count_for_cb.get() + 1);
            // SAFETY: `flag_for_cb` points at the live `flag` AtomicI32 below,
            // which outlives the loop (we stop the loop before dropping it).
            unsafe {
                (*(flag_for_cb as *const AtomicI32)).store(0, Ordering::SeqCst);
            }
        }) as Box<dyn FnMut()>);

        let mut lp = AudioCallbackLoop::new();
        let started = lp
            .start_with_atomics(
                &wasm_bindgen::memory(),
                flag_ptr,
                cb.as_ref().unchecked_ref::<js_sys::Function>().clone(),
            )
            .expect("starting the callback loop should not error");
        assert!(started, "loop must start when Atomics.waitAsync is supported");
        assert!(lp.is_running());

        // Wake the loop the way the worklet does: set the flag non-zero, notify.
        js_sys::Atomics::store(&int32, flag_index, 1).expect("Atomics.store");
        js_sys::Atomics::notify(&int32, flag_index).expect("Atomics.notify");

        // Hand control back to the event loop so the waitAsync continuation can
        // run the callback before we assert.
        crate::test_support::sleep_ms(100).await;

        assert!(
            count.get() >= 1,
            "tick callback must fire after Atomics.notify (got {})",
            count.get()
        );

        // Stop sets running=false and notifies; the loop exits without invoking
        // the callback again. Drain once more, then keep `cb`/`flag` alive until
        // teardown so the JS loop can never call a dropped closure or read freed
        // memory.
        lp.stop();
        assert!(!lp.is_running());
        crate::test_support::sleep_ms(20).await;
        drop(cb);
        drop(flag);
    }
}
