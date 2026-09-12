// Must match `RENDER_QUANTUM_FRAMES` in worklet.rs (fixed by the Web Audio spec)
// and `protocol::MAX_CHANNELS` in protocol.rs (the wire's channel limit).
const RENDER_QUANTUM_FRAMES = 128;
const MAX_CHANNELS = 8;

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

        // Float32Array views over the WASM processor's planar scratch
        // buffers, one per channel plane. Rebuilt whenever memory.buffer has
        // detached (grown) since the last build — see ensureViews().
        this.inputViews = null;
        this.outputViews = null;

        // Listen for stop message from main thread
        this.port.onmessage = (event) => {
            if (event.data === 'stop') {
                this.stopped = true;
            }
        };
    }

    // (Re)build the per-channel views over the processor's input/output
    // scratch buffers. A no-op unless memory.buffer has detached (grown)
    // since the last call, since a detached buffer's views can no longer be
    // read or written.
    ensureViews() {
        if (this.inputViews !== null && this.inputViews[0].buffer === this.memory.buffer) {
            return;
        }
        const inputPtr = this.processor.input_ptr();
        const outputPtr = this.processor.output_ptr();
        this.inputViews = [];
        this.outputViews = [];
        for (let ch = 0; ch < MAX_CHANNELS; ch++) {
            const byteOffset = ch * RENDER_QUANTUM_FRAMES * Float32Array.BYTES_PER_ELEMENT;
            this.inputViews.push(
                new Float32Array(this.memory.buffer, inputPtr + byteOffset, RENDER_QUANTUM_FRAMES)
            );
            this.outputViews.push(
                new Float32Array(this.memory.buffer, outputPtr + byteOffset, RENDER_QUANTUM_FRAMES)
            );
        }
    }

    process(inputs, outputs) {
        // Stop processing if signaled
        if (this.stopped) {
            return false;
        }

        this.ensureViews();

        // Capture stays mono until real multichannel capture lands, so only
        // channel 0 of the input plane is ever populated with real samples.
        const inputChannel = inputs[0]?.[0];
        if (inputChannel) {
            this.inputViews[0].set(inputChannel);
        } else {
            this.inputViews[0].fill(0);
        }

        const inChannels = inputs[0]?.length || 1;
        const outChannels = outputs[0]?.length || 1;

        // Process audio through the Wasm processor (even if no input for playback)
        const result = this.processor.render(inChannels, outChannels, RENDER_QUANTUM_FRAMES);

        for (let ch = 0; ch < outChannels; ch++) {
            outputs[0][ch]?.set(this.outputViews[ch]);
        }

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
