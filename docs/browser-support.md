# Browser Support

WebTrip runs entirely in the browser, but it relies on a handful of recent web platform features.

## Core Requirements (All Browsers)

The following features are **required** for WebTrip to function:
- **WebAssembly with SharedArrayBuffer** - Used for WASM linear memory and atomic buffer operations between AudioWorklet and main thread
- **AudioWorklet API** - Low-latency audio processing
- **MediaDevices API** - Microphone/device access (getUserMedia)
- **Cross-Origin Isolation** - Required for SharedArrayBuffer (COOP/COEP headers)
- **Secure context** - HTTPS (or `localhost`): signaling uses `wss://`, and `getUserMedia` and WebTransport are only available in secure contexts

`Atomics.waitAsync` (Chrome 87+, Firefox 145+, Safari 16.4+) lets a WebRTC connection tick once per audio callback. Without it, WebRTC falls back to ticking on a 4 ms timer. The WebTransport worker needs it (see below).

## Minimum Browser Versions (WebRTC DataChannels)

- **Chrome 92+** (July 2021)
- **Edge 92+** (July 2021)
- **Firefox 89+** (June 2021)
- **Safari 16.4+** (March 2023)

## WebTransport (QUIC Datagrams)

WebTransport offers lower latency but requires newer browsers. The session does not fall back to WebRTC on its own: check `WebTripSession.isWebTransportAvailable()` and select WebRTC when it returns `false`.
- **Chrome 97+**
- **Edge 97+**
- **Firefox 145+** (WebTransport shipped in 114, `Atomics.waitAsync` in 145)
- **Safari 26.4+**

## Unsupported Browsers

WebTrip **will not work** on:
- Browsers without SharedArrayBuffer (Chrome <68, Firefox <79, Safari <15.2)
- Browsers without AudioWorklet (Chrome <66, Firefox <76, Safari <14.1)
- Sites without proper COOP/COEP headers, or served over plain HTTP (other than `localhost`)
