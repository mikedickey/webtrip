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
        
        // Signal main thread to process send/receive via Atomics.notify
        // This ensures bidirectional audio works even in listen-only mode
        if (this.hasFlagPtr !== undefined) {
            // Update Int32Array view (in case memory grew)
            this.int32View = new Int32Array(this.memory.buffer);
            const flagIndex = this.hasFlagPtr / 4;

            // The RingBuffer.write() already set the flag to 1
            // Now notify any waiters (main thread waiting via Atomics.waitAsync)
            Atomics.notify(this.int32View, flagIndex, 1);
        }
        
        return result;
    }
});
