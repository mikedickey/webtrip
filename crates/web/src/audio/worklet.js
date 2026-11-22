// Frame and channel capacities come from the ProcessorHandle itself
// (`frames_per_quantum()` / `channel_capacity()`), never constants here.

registerProcessor("WasmProcessor", class WasmProcessor extends AudioWorkletProcessor {
    constructor(options) {
        super();
        let [module, memory, handle, hasFlagPtr] = options.processorOptions;
        bindgen.initSync({ module, memory });
        this.processor = bindgen.ProcessorHandle.from_raw_ptr(handle);
        this.stopped = false;
        this.frames = this.processor.frames_per_quantum();
        this.channelCapacity = this.processor.channel_capacity();
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
        for (let ch = 0; ch < this.channelCapacity; ch++) {
            const byteOffset = ch * this.frames * Float32Array.BYTES_PER_ELEMENT;
            this.inputViews.push(
                new Float32Array(this.memory.buffer, inputPtr + byteOffset, this.frames)
            );
            this.outputViews.push(
                new Float32Array(this.memory.buffer, outputPtr + byteOffset, this.frames)
            );
        }
    }

    process(inputs, outputs) {
        // Stop processing if signaled
        if (this.stopped) {
            return false;
        }

        this.ensureViews();

        // Copy every input channel plane the browser actually delivered this
        // callback into the corresponding WASM scratch view.
        const inputChannels = inputs[0] || [];
        const inChannels = inputChannels.length || 1;
        for (let ch = 0; ch < inChannels; ch++) {
            const chan = inputChannels[ch];
            if (chan) {
                this.inputViews[ch].set(chan);
            } else {
                this.inputViews[ch].fill(0);
            }
        }

        const outChannels = outputs[0]?.length || 1;

        // Process audio through the Wasm processor (even if no input for playback)
        const result = this.processor.render(inChannels, outChannels, this.frames);

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
