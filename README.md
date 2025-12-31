# WASM Audio Worklet - Audio Capture

A WebAssembly-based audio capture and monitoring application. Audio processing runs in Rust/WASM in the browser's audio worklet thread, while the UI is built with TypeScript.

## Features

- **Audio Capture**: Capture audio from any input device (microphone, virtual audio devices, etc.)
- **Real-time Volume Meter**: Visual volume level indicator with smooth animations
- **Device Selection**: Choose from available input and output audio devices
- **Audio Processing Controls**:
  - Auto Gain Control (AGC)
  - Echo Cancellation
  - Noise Suppression
  - Loopback Mode (play captured audio through speakers)

## Architecture

This application uses a multi-threaded architecture with WebAssembly for high-performance audio processing:

```
┌─────────────────────────────────────────────────────────────────┐
│                     Main Thread (TypeScript)                     │
│                          src/app.ts                              │
│                                                                   │
│  • UI rendering and interaction                                  │
│  • Device selection and configuration                            │
│  • Volume meter visualization                                    │
│  • Communicates with WASM via JavaScript bindings               │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                 WASM Layer (Rust - Main Thread)                  │
│              src/lib.rs, src/wasm_audio.rs                       │
│                                                                   │
│  AudioEngine (High-level orchestrator)                           │
│    • Manages Web Audio API AudioContext                          │
│    • Handles device enumeration (MediaDevices API)               │
│    • Creates and connects audio nodes                            │
│    • Lifecycle management (start/stop capture)                   │
│    • Wraps AudioProcessor in AudioWorkletNode                    │
│                                                                   │
│  AudioParams (Shared state)                                      │
│    • Lock-free atomic values for thread-safe communication       │
│    • Volumes, gains, dB levels, peak tracking                    │
│    • Shared between main thread and audio worklet thread         │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│            Audio Worklet Thread (JavaScript + WASM)              │
│                  src/worklet.js (bridge)                         │
│                  src/audio_processor.rs (core)                   │
│                                                                   │
│  WasmAudioProcessor (JS bridge)                                  │
│    • AudioWorkletProcessor implementation                        │
│    • Calls into WASM for each audio buffer                       │
│    • Bridges Web Audio API and Rust audio processing            │
│                                                                   │
│  AudioProcessor (Rust DSP engine)                                │
│    • Real-time audio processing on 128-sample buffers            │
│    • Volume metering (RMS → dB conversion)                       │
│    • Peak level tracking with hold and decay                     │
│    • Input gain control                                          │
│    • Audio monitoring/loopback routing                           │
└─────────────────────────────────────────────────────────────────┘
```

### Component Details

#### **Frontend Layer** (`src/app.ts`)
TypeScript application that manages the UI and user interactions:
- Renders audio controls and volume meters
- Handles device selection
- Reads shared audio parameters for visualization
- Calls into WASM for audio operations

#### **WASM Orchestration Layer**

**`AudioEngine`** (`src/wasm_audio.rs`)  
High-level manager for the Web Audio infrastructure:
- Creates and configures `AudioContext`
- Enumerates audio input/output devices
- Manages MediaStream acquisition (getUserMedia)
- Constructs the audio node graph: `MediaStreamSource → AudioWorkletNode → AudioDestination`
- Provides JavaScript-callable API via `#[wasm_bindgen]`

**`AudioParams`** (`src/audio_params.rs`)  
Thread-safe shared state using atomic operations:
- Stores audio parameters (volumes, gains, processing flags)
- Accessible from both main thread and audio worklet thread
- Lock-free design for real-time audio requirements
- Uses `AtomicI32` and `AtomicU32` for cross-thread communication

#### **Audio Processing Layer**

**`WasmAudioProcessor`** (`src/worklet.js`)  
JavaScript bridge that implements `AudioWorkletProcessor`:
- Runs on the audio worklet thread (dedicated audio processing thread)
- Receives audio buffers from Web Audio API
- Calls into WASM `AudioProcessor` for each buffer
- Handles message passing between threads

**`AudioProcessor`** (`src/audio_processor.rs`)  
Core real-time audio processing engine in Rust:
- Processes 128-sample audio buffers at audio rate (~375 times/second @ 48kHz)
- Calculates RMS volume levels and converts to dB
- Implements peak metering with configurable hold and decay
- Applies input gain and output routing
- Handles audio monitoring (loopback) with volume control
- Extensible for future DSP features (effects, filters, etc.)

### Data Flow

1. **Initialization**: `app.ts` → `AudioEngine.create()` → Loads audio worklet
2. **Start Capture**: `app.ts` → `AudioEngine.startCapture()` → Creates audio node graph
3. **Real-time Processing**: Audio input → `WasmAudioProcessor` → `AudioProcessor.process()` → Audio output
4. **Visualization**: `app.ts` reads `AudioParams` atomic values → Updates UI meters

### Why This Architecture?

- **Performance**: Audio processing in Rust/WASM on dedicated audio thread
- **Thread Safety**: Lock-free atomics for communication without blocking
- **Separation of Concerns**: UI, orchestration, and DSP are cleanly separated
- **Extensibility**: Easy to add new audio effects or analysis features to `AudioProcessor`
- **Web Standards**: Uses Web Audio API best practices (AudioWorklet for low-latency)

## Building

Install dependencies:
```bash
npm install
```

Build both WASM and TypeScript:
```bash
npm run build
```

Or build separately:
```bash
npm run build:wasm  # Build Rust to WASM
npm run build:ts    # Compile TypeScript
```

## Running

Start a local web server:
```bash
npm run serve
```

Then point your browser at http://localhost:8080/

**Note**: The application requires microphone permissions. You'll be prompted to allow microphone access when the page loads.

## Browser Compatibility

Requires a browser with support for:
- WebAssembly with SharedArrayBuffer
- AudioWorklet API
- MediaDevices API (getUserMedia)

Tested on:
- Chrome 80+
- Firefox 76+
- Safari 14.1+
- Edge 80+

## Credits

Developed by Mike Dickey

Special thanks to Chris Chafe for his work on [JackTrip](https://github.com/jacktrip/jacktrip), Matteo Sacchetto for his work on [jacktrip-webrtc project](https://github.com/jacktrip-webrtc/jacktrip-webrtc) and Lukas Lihotzki for the [WASM Audio Worklet example](https://wasm-bindgen.github.io/wasm-bindgen/examples/wasm-audio-worklet.html).
