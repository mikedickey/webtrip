/**
 * WebTransport Worker Bootstrap
 * 
 * Minimal JavaScript glue to:
 * 1. Load the WASM module in the worker context
 * 2. Initialize it with shared memory from the main thread
 * 3. Hand off all message handling to Rust
 */

let wasm = null;

// Initialize WASM module with shared memory. `init` and `bindgen` come from the
// header `dependent_module!` prepends, which imports the bindgen glue from its
// own `import.meta.url` — so this works wherever the glue is served from.
async function initWasm(memory) {
    await init({ memory });
    wasm = bindgen;
    return wasm;
}

// Forward all messages to Rust after WASM is loaded
self.onmessage = async (event) => {
    try {
        const msg = event.data;
        
        // Special handling for init message - must load WASM first
        if (msg.type === 'init') {
            await initWasm(msg.wasmMemory);
        }
        
        // Forward to Rust message handler (all logic is in Rust)
        if (wasm && wasm.handleWorkerMessage) {
            const result = await wasm.handleWorkerMessage(msg);
            // Rust will call postMessage directly, but we can return result if needed
            if (result && typeof result === 'string') {
                self.postMessage(result);
            }
        } else if (!wasm) {
            console.error('[WebTransport Worker] ❌ WASM not initialized yet!');
            self.postMessage({ 
                type: 'error', 
                error: 'WASM module not initialized'
            });
        } else if (!wasm.handleWorkerMessage) {
            console.error('[WebTransport Worker] ❌ handleWorkerMessage function not found in WASM!');
            self.postMessage({ 
                type: 'error', 
                error: 'handleWorkerMessage function not found'
            });
        }
    } catch (error) {
        console.error('[WebTransport Worker] ❌ Error processing message:', error);
        console.error('[WebTransport Worker] 📋 Error stack:', error.stack);
        self.postMessage({ 
            type: 'error', 
            error: error.message || String(error)
        });
    }
};
