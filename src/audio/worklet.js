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
        this.hasAtomics = typeof Atomics !== 'undefined' && typeof Atomics.notify === 'function';
        
        if (!this.hasAtomics) {
            console.warn('⚠️ Atomics.notify not available, falling back to postMessage');
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
        
        // If no input or empty input, keep running but skip processing
        if (!input || input.length === 0) {
            return true;
        }
        
        // Get output buffer
        const output = outputs[0]?.[0] || new Float32Array(128);
        
        // Process audio through the Wasm processor
        const result = this.processor.process(input, output);
        
        // Signal main thread that audio data is ready to send
        if (this.hasAtomics && this.hasFlagPtr !== undefined) {
            // Event-driven: Use Atomics.notify() for zero-CPU wake-up
            // Update Int32Array view (in case memory grew)
            this.int32View = new Int32Array(this.memory.buffer);
            const flagIndex = this.hasFlagPtr / 4;
            
            // The RingBuffer.write() already set the flag to 1
            // Now notify any waiters (main thread waiting via Atomics.waitAsync)
            const numWoken = Atomics.notify(this.int32View, flagIndex, 1);
            // numWoken will be 1 if main thread was waiting, 0 if it wasn't
        } else {
            // Fallback: Use postMessage (old behavior)
        this.port.postMessage('audio-ready');
        }
        
        return result;
    }
});
