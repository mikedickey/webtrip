# WebTrip

[![codecov](https://codecov.io/gh/mikedickey/webtrip/branch/main/graph/badge.svg)](https://codecov.io/gh/mikedickey/webtrip)

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

The pinned nightly toolchain and required components install automatically from
[`rust-toolchain.toml`](rust-toolchain.toml) the first time you build.

## Toolchain Requirements

Two unusual build requirements are worth explaining, since neither comes from the crate's
own source — WebTrip is plain stable Rust with no `#![feature(...)]` attributes.

### Why the nightly toolchain

WebTrip runs audio on real threads, which means SharedArrayBuffer, which means the WASM
module must be linked with `--shared-memory` and built with the `atomics` target feature.
The `wasm32-unknown-unknown` `std` that rustup ships is precompiled *without* atomics, so
linking against it fails:

```
rust-lld: error: --shared-memory is disallowed by std-....rcgu.o
  because it was not compiled with 'atomics' or 'bulk-memory' features.
```

The fix is to rebuild `std` from source with matching features, via `-Zbuild-std=std,panic_abort`.
That flag is nightly-only, and it is the sole reason for the pin.

**Blockers on removing nightly:**

| Flag | Used by | Blocks removal because |
|------|---------|------------------------|
| `-Zbuild-std=std,panic_abort` | `build:wasm`, `check`, `test:wasm`, and both wasm coverage scripts | Unstable ([cargo#8733](https://github.com/rust-lang/cargo/issues/8733)). Needed until upstream ships a prebuilt atomics-enabled wasm `std`. Also why `rust-toolchain.toml` requests the `rust-src` component. |
| `-Zno-profiler-runtime` | `build:wasm:coverage`, `coverage:wasm` | Unstable. Suppresses the LLVM profiler runtime, which doesn't build for wasm, so `minicov` can supply one instead. |
| `-Ctarget-feature=+atomics` | every wasm build | Not a hard blocker — stable accepts it, but warns that the feature is unstably supported. Moot while `-Zbuild-std` applies. |

The specific nightly version is not significant; nothing depends on a given nightly's
language features, so the pin can move freely. It exists only to match the build container
(see `RUST_NIGHTLY` in [containers/builder/Containerfile](containers/builder/Containerfile)) —
bump both together.

Native (non-WASM) work needs none of this: `npm run test` and `npm run coverage` pass no
`-Z` flags and would run on stable today.

### Why `--cfg=web_sys_unstable_apis`

This one is unrelated to the release channel — it's a plain `--cfg` and works fine on
stable. `web-sys` gates bindings for browser APIs that are not yet W3C-stable behind this
cfg, and WebTrip uses the WebTransport family (`WebTransport`, `WebTransportOptions`,
`WebTransportDatagramDuplexStream`, and friends) for its QUIC datagram transport. Without
the flag the build fails with `cannot find type 'WebTransport' in crate 'web_sys'`.

**Blocker on removing it:** the WebTransport bindings must graduate out of `web-sys`'s
unstable gate. Dropping the transport itself would also do it, but WebRTC data channels are
the fallback path, not a replacement — see [Browser Compatibility](#browser-compatibility).

Because both of the above are passed as flags rather than committed to `.cargo/config.toml`,
bare `cargo check` / `cargo test` invocations will fail. Always go through the npm scripts.

## Building

Install dependencies:
```bash
npm install
```

Build both the WASM module and the website:
```bash
npm run build
```

Or build separately:
```bash
npm run build:wasm  # Build Rust to WASM
npm run build:site  # Install website deps and build the React SPA
```

## Running

Start a local web server:
```bash
npm run serve
```

Then point your browser at http://localhost:3000/demo (or https://localhost:8443/demo when serving with TLS) for the demo app.

**Note**: The demo requires microphone permissions. You'll be prompted to allow microphone access when the page loads.

## Website

The [webtrip.dev](https://webtrip.dev) website lives in [website/](website/): a React SPA
(Vite) with the demo embedded at `/demo` and a docs page at `/docs`. The demo loads the
wasm-pack output at runtime from `/pkg/`, which `website/server.js` serves from the repo
root alongside the built site.

For website development with hot reload, run `npm run dev` inside `website/`
(the demo also works there once the repo-root `npm run build:wasm` has produced `pkg/`).

## Browser Compatibility

### Core Requirements (All Browsers)

The following features are **required** for WebTrip to function:
- **WebAssembly with SharedArrayBuffer** - Used for WASM linear memory and atomic buffer operations between AudioWorklet and main thread
- **Atomics.waitAsync** - Event-driven wake-up for lock-free cross-thread communication
- **AudioWorklet API** - Low-latency audio processing
- **MediaDevices API** - Microphone/device access (getUserMedia)
- **Cross-Origin Isolation** - Required for SharedArrayBuffer (COOP/COEP headers)

### Minimum Browser Versions (WebRTC DataChannels)

- **Chrome 92+** (July 2021)
- **Edge 92+** (July 2021)
- **Firefox 89+** (June 2021)
- **Safari 16.4+** (March 2023)

### WebTransport (QUIC Datagrams)

WebTransport offers lower latency but requires newer browsers. Falls back to WebRTC when unavailable.
- **Chrome 97+**
- **Edge 97+**
- **Firefox 114+**
- **Safari 26.4+**

### Unsupported Browsers

WebTrip **will not work** on:
- Browsers without `Atomics.waitAsync` (Chrome <92, Firefox <89, Safari <16.4)
- Browsers without SharedArrayBuffer (Chrome <68, Firefox <79, Safari <15.2)
- Browsers without AudioWorklet (Chrome <66, Firefox <76, Safari <14.1)
- Sites without proper COOP/COEP headers

## Kudos

Special thanks to Chris Chafe for his work on [JackTrip](https://github.com/jacktrip/jacktrip), Matteo Sacchetto for his work on [jacktrip-webrtc project](https://github.com/jacktrip-webrtc/jacktrip-webrtc) and Lukas Lihotzki for the [WASM Audio Worklet example](https://wasm-bindgen.github.io/wasm-bindgen/examples/wasm-audio-worklet.html).
