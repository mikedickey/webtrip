# WebTrip

WebTrip is a software development toolkit for lossless, low-latency audio collaboration over the Internet. It can run entirely within any modern web browser on any popular device, avoiding the need for users to install any apps or software.

WebTrip is inspired by the popular open-source [JackTrip project](https://github.com/jacktrip/jacktrip), which originated from Stanford University's Center for Computer Reserch in Music and Acoustics ([CCRMA](https://ccrma.stanford.edu/)). WebTrip envisions a complete rewrite of JackTrip's core library and command line tools using the [Rust programming language](https://rust-lang.org/), with a focus on reuse by developers.


For more details on the architecture and how WebTrip handles real-time audio streaming, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (via `rustup`)
- [Node.js](https://nodejs.org/) (via `nvm` or direct install)
- [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/) — install with:
  ```bash
  cargo install wasm-pack
  ```

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
npm run build:app   # Compile TypeScript UI example
```

## Running

Start a local web server:
```bash
npm run serve
```

Then point your browser at http://localhost:3000/

**Note**: The application requires microphone permissions. You'll be prompted to allow microphone access when the page loads.

## Browser Compatibility

### Core Requirements (All Browsers)

The following features are **required** for WebTrip to function:
- **WebAssembly with SharedArrayBuffer** - Used for WASM linear memory and atomic buffer operations between AudioWorklet and main thread
- **AudioWorklet API** - Low-latency audio processing
- **MediaDevices API** - Microphone/device access (getUserMedia)
- **Cross-Origin Isolation** - Required for SharedArrayBuffer (COOP/COEP headers)

### Recommended Browser Versions (Optimized Performance)

For best performance with event-driven wake-up (`Atomics.waitAsync`):
- **Chrome 92+** (July 2021)
- **Firefox 89+** (June 2021)
- **Safari 16.4+** (March 2023)
- **Edge 92+** (July 2021)

### Minimum Browser Versions (Fallback Mode)

Older browsers with SharedArrayBuffer but without `Atomics.waitAsync` will work with reduced performance using `postMessage` fallback:
- **Chrome 87-91**
- **Safari 15.2-16.3** (primarily affects older macOS/iOS versions)

The fallback has ~2-3x higher CPU usage but maintains full functionality.

### Browser Versions Without Support

WebTrip **will not work** on:
- Browsers without SharedArrayBuffer (Chrome <68, Firefox <79, Safari <15.2)
- Browsers without AudioWorklet (Chrome <66, Firefox <76, Safari <14.1)
- Sites without proper COOP/COEP headers

## Kudos

Special thanks to Chris Chafe for his work on [JackTrip](https://github.com/jacktrip/jacktrip), Matteo Sacchetto for his work on [jacktrip-webrtc project](https://github.com/jacktrip-webrtc/jacktrip-webrtc) and Lukas Lihotzki for the [WASM Audio Worklet example](https://wasm-bindgen.github.io/wasm-bindgen/examples/wasm-audio-worklet.html).
