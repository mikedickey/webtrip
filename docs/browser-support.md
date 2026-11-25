# Browser Support

WebTrip runs entirely in the browser, but it relies on a handful of recent web platform features.

## Core Requirements (All Browsers)

The following features are **required** for WebTrip to function:
- **WebAssembly with SharedArrayBuffer** - Used for WASM linear memory and atomic buffer operations between AudioWorklet and main thread
- **Atomics.waitAsync** - Event-driven wake-up for lock-free cross-thread communication
- **AudioWorklet API** - Low-latency audio processing
- **MediaDevices API** - Microphone/device access (getUserMedia)
- **Cross-Origin Isolation** - Required for SharedArrayBuffer (COOP/COEP headers)

## Minimum Browser Versions (WebRTC DataChannels)

- **Chrome 92+** (July 2021)
- **Edge 92+** (July 2021)
- **Firefox 89+** (June 2021)
- **Safari 16.4+** (March 2023)

## WebTransport (QUIC Datagrams)

WebTransport offers lower latency but requires newer browsers. Falls back to WebRTC when unavailable.
- **Chrome 97+**
- **Edge 97+**
- **Firefox 114+**
- **Safari 26.4+**

## Unsupported Browsers

WebTrip **will not work** on:
- Browsers without `Atomics.waitAsync` (Chrome <92, Firefox <89, Safari <16.4)
- Browsers without SharedArrayBuffer (Chrome <68, Firefox <79, Safari <15.2)
- Browsers without AudioWorklet (Chrome <66, Firefox <76, Safari <14.1)
- Sites without proper COOP/COEP headers

