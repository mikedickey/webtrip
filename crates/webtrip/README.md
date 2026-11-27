# webtrip

[WebTrip](https://webtrip.dev) is a toolkit for lossless, low-latency audio
collaboration over the Internet, inspired by
[JackTrip](https://github.com/jacktrip/jacktrip). This crate is the one-dependency
entry point: it re-exports the WebTrip crates and contains no logic of its own.

| Module | Crate | Contents |
|--------|-------|----------|
| [`webtrip::core`](https://docs.rs/webtrip/latest/webtrip/core/) | [`webtrip-core`](https://docs.rs/webtrip-core) | Portable audio processing, JackTrip wire protocol, jitter buffer, transport contract |
| [`webtrip::api`](https://docs.rs/webtrip/latest/webtrip/api/) | [`webtrip-api`](https://docs.rs/webtrip-api) | Typed client for the JackTrip Virtual Studio REST API |
| [`webtrip::web`](https://docs.rs/webtrip/latest/webtrip/web/) | [`webtrip-web`](https://docs.rs/webtrip-web) | Browser host: AudioWorklet engine, WebRTC/WebTransport, `WebTripSession` (feature `web`) |

With default features `webtrip` is as portable as `webtrip-core`: no browser
dependencies, and it builds natively.

The [`webtrip` npm package](https://www.npmjs.com/package/webtrip) is a different
artifact with the same name: the wasm-pack build of `webtrip-web`, for JavaScript
and TypeScript. `cargo add webtrip` gives you this Rust facade instead.

## Example

```rust,no_run
use webtrip::api::api::{ApiClient, ApiError};

# async fn run() -> Result<(), ApiError> {
let client = ApiClient::new();
client.set_bearer_token("your-jwt-token".to_string());
let studios = client.studios().list_studios().await?;
println!("{} studios", studios.len());
# Ok(()) }
```

## Features

- `web` — re-export the browser host as `webtrip::web`. It only builds for
  `wasm32-unknown-unknown` with threads, and needs the same nightly `-Zbuild-std`
  and `RUSTFLAGS` as [`webtrip-web`](https://docs.rs/webtrip-web).
- `wasm` — `#[wasm_bindgen]` exports on core and api types, without the browser
  host. Enable it when you build your own `cdylib` that exposes those types to
  JavaScript.

## Learn more

- [Getting started](https://webtrip.dev/docs/getting-started)
- [Architecture](https://webtrip.dev/docs/architecture)
- [Source on GitHub](https://github.com/mikedickey/webtrip)
