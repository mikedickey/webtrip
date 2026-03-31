# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

WebTrip is a Rust-to-WebAssembly rewrite of JackTrip for low-latency audio collaboration in web browsers. Rust code compiles to WASM via wasm-pack; a TypeScript layer handles the UI.

## Build Commands

**Always use npm scripts for building, never call wasm-pack directly** — the WASM build requires specific flags for threading support (atomics, shared memory, TLS exports).

- `npm run build` — Build both WASM and TypeScript
- `npm run build:wasm` — Build only the Rust WASM module
- `npm run build:app` — Build only the TypeScript app (`tsc`)
- `npm run clean` — Remove dist/, pkg/, and target/
- `npm run serve` — Start dev server (HTTP :3000 or HTTPS :8443 with TLS)

**Never run `cargo check` or `cargo test` directly** — they will fail because `web_sys` types like `WebTransport` are gated behind `web_sys_unstable_apis`. Use the npm scripts which pass the required flags:

- `npm run check` — Run `cargo check` with correct RUSTFLAGS and WASM target
- `npm run test` — Run `cargo test` with correct RUSTFLAGS (runs native, not WASM)

## Architecture

### Threading Model (3 threads in browser)

1. **Main Thread** — WebRTC signaling, session management, UI updates, `Session.tick()` loop
2. **AudioWorklet Thread** — Real-time DSP via `AudioProcessor`; reads/writes shared buffers
3. **Worker Thread** — WebTransport protocol handling (dedicated web worker)

All cross-thread communication uses **lock-free atomics** (no mutexes): `RingBuffer` (SPSC queue), `Regulator` (jitter buffer), and `AudioParams` (shared state).

### Audio Data Flow

**WebRTC** (main thread mediates):
```
Send: AudioWorklet → RingBuffer → Session.tick() → WebRtcTransport → DataChannel → Network
Recv: Network → DataChannel → WebRtcTransport → Session.tick() → Regulator → AudioWorklet
```

**WebTransport** (worker thread accesses shared buffers directly):
```
Send: AudioWorklet → RingBuffer → Worker send_loop() → QUIC datagrams → Network
Recv: Network → QUIC datagrams → Worker receive_loop() → Regulator → AudioWorklet
```

### Transport Layer

`Transport` trait (`src/audio/transport.rs`) with three implementations:
- **WebRtcTransport** — RTCPeerConnection + DataChannels (universal browser support)
- **WebTransportImpl** — QUIC-based, runs on dedicated worker thread (Chrome/Edge 97+, Firefox 114+, Safari 26.4+)
- **MockTransport** — Sine wave generator for testing without a server

### Key Modules

- **`src/session.rs`** — `WebTripSession`: top-level orchestrator, connection state machine, owns shared buffers
- **`src/audio/regulator.rs`** — Jitter buffer with Burg PLC (packet loss concealment), ported from JackTrip C++
- **`src/audio/protocol.rs`** — JackTrip 16-byte wire protocol (serialization, sample rate encoding)
- **`src/audio/signaling.rs`** — Hub server WebSocket signaling for WebRTC/WebTransport
- **`src/audio/ring_buffer.rs`** — Lock-free SPSC queue with `Atomics.waitAsync` wake-up
- **`src/audio/params.rs`** — Atomic shared state for volume, gain, peaks across threads
- **`src/api/`** — HTTP API client (reqwest) for JackTrip Virtual Studio REST API
- **`src/models/`** — Typed data models with auto-generated TypeScript types via `tsify-next`
- **`src/app.ts`** — TypeScript UI controller, initializes WASM, binds DOM elements
- **`src/lib.rs`** — WASM entry point, exports `init()` and public types to JavaScript

### Browser Requirements

All modes require SharedArrayBuffer (Cross-Origin Isolation via COOP/COEP headers, set by server.js), AudioWorklet API, and MediaDevices API.

**Minimum (WebRTC DataChannels):**
- Chrome 92+ / Edge 92+ / Firefox 89+ / Safari 16.4+
- Requires Atomics.waitAsync, SharedArrayBuffer, AudioWorklet, MediaDevices

**WebTransport (QUIC datagrams):**
- Chrome 97+ / Edge 97+ / Firefox 114+ / Safari 26.4+
- Falls back to WebRTC when unavailable

## Rust/WASM Specifics

- **Nightly toolchain** required (see `rust-toolchain.toml`) — needed for `-Zbuild-std` and atomics
- **Target**: `wasm32-unknown-unknown`
- **Crate type**: `cdylib` — produces WASM binary, not a Rust library
- JS interop via `wasm-bindgen`; browser APIs via `web-sys` (feature-gated, see Cargo.toml)
- The hub server may create its own WebRTC data channel — both client and server-created channels need message handlers

## API Integration

- JackTrip API base: `https://test.jacktrip.com/api`
- OpenAPI spec: `https://test.jacktrip.com/api/redirect/openapi`
- API docs in `docs/api/`; architecture docs in `docs/ARCHITECTURE.md`
