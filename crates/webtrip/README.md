# webtrip

[WebTrip](https://webtrip.dev) is a toolkit for lossless, low-latency audio
collaboration over the Internet, inspired by
[JackTrip](https://github.com/jacktrip/jacktrip). This crate is the one-dependency
entry point: it re-exports the WebTrip crates and contains no logic of its own.

| Module | Crate | Contents |
|--------|-------|----------|
| [`webtrip::core`](crate::core) | [`webtrip-core`](https://docs.rs/webtrip-core) | Portable audio processing, JackTrip wire protocol, jitter buffer, transport contract |
| [`webtrip::api`](crate::api) | [`webtrip-api`](https://docs.rs/webtrip-api) | Typed client for the JackTrip Virtual Studio REST API |
| `webtrip::web` | [`webtrip-web`](https://docs.rs/webtrip-web) | Browser host: AudioWorklet engine, WebRTC/WebTransport, `WebTripSession` (feature `web`) |

With default features `webtrip` is as portable as `webtrip-core`: no browser
dependencies, and it builds natively.

## Features

- `web` — re-export the browser host as `webtrip::web`. It only builds for
  `wasm32-unknown-unknown` with threads enabled.
- `wasm` — JS exports on core and api types, without the browser host.

## Learn more

- [Getting started](https://webtrip.dev/docs/getting-started)
- [Architecture](https://webtrip.dev/docs/architecture)
- [Source on GitHub](https://github.com/mikedickey/webtrip)
