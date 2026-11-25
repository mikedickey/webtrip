# WebTrip

[![codecov](https://codecov.io/gh/mikedickey/webtrip/branch/main/graph/badge.svg)](https://codecov.io/gh/mikedickey/webtrip)
[![docs.rs](https://img.shields.io/docsrs/webtrip)](https://docs.rs/webtrip)

WebTrip is a software development toolkit for lossless, low-latency audio collaboration over the Internet. It can run entirely within any modern web browser on any popular device, avoiding the need for users to install any apps or software.

WebTrip is inspired by the popular open-source [JackTrip project](https://github.com/jacktrip/jacktrip), which originated from Stanford University's Center for Computer Research in Music and Acoustics ([CCRMA](https://ccrma.stanford.edu/)). WebTrip envisions a complete rewrite of JackTrip's core library and command line tools using the [Rust programming language](https://rust-lang.org/), with a focus on reuse by developers.

## Documentation

- **[webtrip.dev/docs](https://webtrip.dev/docs)** — the guide, rendered from [docs/](docs/):
  [getting started](docs/getting-started.md), [architecture](docs/architecture.md),
  [channel model](docs/channel-model.md), [browser support](docs/browser-support.md)
- **[docs.rs/webtrip](https://docs.rs/webtrip)** — API reference for every crate
- **[docs/contributing/](docs/contributing/)** — [toolchain](docs/contributing/toolchain.md),
  [testing](docs/contributing/testing.md), [containers](docs/contributing/containers.md)

## Crates

| Crate | What it is |
|-------|------------|
| [`webtrip`](crates/webtrip/) | One-dependency facade re-exporting the crates below |
| [`webtrip-core`](crates/core/) | Portable core: audio processing, JackTrip wire protocol, jitter buffer, transport contract |
| [`webtrip-api`](crates/api/) | Typed client for the JackTrip Virtual Studio REST API |
| [`webtrip-web`](crates/web/) | Browser host: AudioWorklet engine, WebRTC and WebTransport transports, `WebTripSession` |

## Quick start

```bash
npm install
npm run build   # WASM module + website
npm run serve   # then open http://localhost:3000/demo
```

See [getting started](docs/getting-started.md) for prerequisites and a minimal session.

## Website

The [webtrip.dev](https://webtrip.dev) website lives in [website/](website/): a React SPA
(Vite) with the docs at `/docs` and the demo at `/demo`. The demo loads the wasm-pack
output at runtime from `/pkg/`, which `website/server.js` serves from the repo root
alongside the built site.

## Kudos

Special thanks to Chris Chafe for his work on [JackTrip](https://github.com/jacktrip/jacktrip), Matteo Sacchetto for his work on [jacktrip-webrtc project](https://github.com/jacktrip-webrtc/jacktrip-webrtc) and Lukas Lihotzki for the [WASM Audio Worklet example](https://wasm-bindgen.github.io/wasm-bindgen/examples/wasm-audio-worklet.html).
