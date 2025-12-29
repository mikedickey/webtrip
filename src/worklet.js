registerProcessor("WasmProcessor", class WasmProcessor extends AudioWorkletProcessor {
    constructor(options) {
        super();
        let [module, memory, handle] = options.processorOptions;
        bindgen.initSync({ module, memory });
        this.processor = bindgen.ProcessorHandle.from_raw_ptr(handle);
        this.stopped = false;
        
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
        return this.processor.process(input, output);
    }
});
