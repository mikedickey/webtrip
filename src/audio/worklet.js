registerProcessor("WasmProcessor", class WasmProcessor extends AudioWorkletProcessor {
    constructor(options) {
        super();
        let [module, memory, handle, hasFlagPtr] = options.processorOptions;
        bindgen.initSync({ module, memory });
        this.processor = bindgen.ProcessorHandle.from_raw_ptr(handle);
        this.stopped = false;
        this.memory = memory;
        this.hasFlagPtr = hasFlagPtr;
        
        // Create Int32Array view for Atomics operations
        // We'll update this on each process() call in case the buffer grows
        this.int32View = null;
        
        // Check if Atomics.notify is available
        // Note: This is the optimized event notification path. If unavailable (Safari 15.2-16.3),
        // we fall back to postMessage for notifications. SharedArrayBuffer is still required for
        // the actual buffer access in both cases.
        this.hasAtomics = typeof Atomics !== 'undefined' && typeof Atomics.notify === 'function';
        
        if (!this.hasAtomics) {
            console.warn('⚠️ Atomics.notify not available, falling back to postMessage (Safari 15.2-16.3)');
        }
        
        // Listen for stop message from main thread
        this.port.onmessage = (event) => {
            if (event.data === 'stop') {
                this.stopped = true;
            }
        };
    }
    process(inputs, outputs) {
        // Stop processing if signaled
        if (this.stopped) {
            return false;
        }
        
        // Get input buffer (from microphone/audio source)
        const input = inputs[0]?.[0];
        
        // Get output buffer
        const output = outputs[0]?.[0] || new Float32Array(128);
        
        // Process audio through the Wasm processor (even if no input for playback)
        const result = this.processor.process(input || new Float32Array(128), output);
        
        // Always signal main thread to process send/receive
        // This ensures bidirectional audio works even in listen-only mode
        if (this.hasAtomics && this.hasFlagPtr !== undefined) {
            // Optimized path: Use Atomics.notify() for zero-CPU wake-up
            // Available in: Chrome 87+, Firefox 89+, Safari 16.4+
            // Update Int32Array view (in case memory grew)
            this.int32View = new Int32Array(this.memory.buffer);
            const flagIndex = this.hasFlagPtr / 4;
            
            // The RingBuffer.write() already set the flag to 1
            // Now notify any waiters (main thread waiting via Atomics.waitAsync)
            const numWoken = Atomics.notify(this.int32View, flagIndex, 1);
            // numWoken will be 1 if main thread was waiting, 0 if it wasn't
        } else {
            // Fallback path: Use postMessage for event notification
            // Used in: Safari 15.2-16.3 (browsers with SharedArrayBuffer but no Atomics.waitAsync)
            // Note: SharedArrayBuffer is still required for buffer access, this only affects notifications
            this.port.postMessage('audio-ready');
        }
        
        return result;
    }
});
