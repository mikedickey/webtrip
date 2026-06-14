# AGENTS.md

For general project background see [README.md](README.md). For the threading model, audio data flow, browser API constraints, and transport architecture see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Release Status Policy

This project has not been released yet. Do not preserve or design for backward compatibility; prefer the simplest clean changes and avoid paying compatibility costs before first release.

## No Code Duplication

**Do not duplicate code anywhere in this codebase — including test code.** Before writing any function, type, constant, or block of logic, check whether it already exists and reuse it. If the same code would appear in more than one place, extract it to a shared location first.

This applies equally to test helpers, serialization utilities, fixture builders, and any other repeated patterns. When you spot existing duplication, fix it as part of the task at hand rather than leaving it in place.

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
- `npm run test:wasm` — Run `wasm-bindgen-test` for browser-only modules (see [docs/WASM_TESTING.md](docs/WASM_TESTING.md) for details)

**Note**: WASM tests require a properly configured browser environment. See the testing guide for requirements and troubleshooting.

## Architecture

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

## Environment-specific instructions

Only read the doc for your environment; skip the others.

| Environment | Instructions |
|-------------|--------------|
| Cursor Cloud | Read [docs/CURSOR_CLOUD.md](docs/CURSOR_CLOUD.md) before running tests or opening the app in Chrome |
