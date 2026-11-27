#![doc = include_str!("../README.md")]
// docs.rs builds on nightly with `--cfg docsrs`: badge the `web` re-export with
// the feature it needs.
#![cfg_attr(docsrs, feature(doc_cfg))]
//
// Re-exports only; no logic lives here. Write `::core` inside this crate
// wherever the built-in `core` crate is meant.

/// Typed client for the JackTrip Virtual Studio REST API ([`webtrip-api`](https://docs.rs/webtrip-api)).
pub use webtrip_api as api;
/// Portable audio processing, wire protocol, jitter buffer and transport contract
/// ([`webtrip-core`](https://docs.rs/webtrip-core)).
pub use webtrip_core as core;
/// Browser host: audio engine, WebRTC/WebTransport transports and `WebTripSession`
/// ([`webtrip-web`](https://docs.rs/webtrip-web)). Only builds for the threaded
/// `wasm32-unknown-unknown` target.
#[cfg(feature = "web")]
#[cfg_attr(docsrs, doc(cfg(feature = "web")))]
pub use webtrip_web as web;
