//! One-dependency facade over the WebTrip crates.
//!
//! Re-exports only; no logic lives here. `webtrip::web` exists solely behind
//! the off-by-default `web` feature, so a default `webtrip` dependency is as
//! portable as `webtrip-core`. (Write `::core` inside this crate wherever the
//! built-in `core` crate is meant.)

pub use webtrip_api as api;
pub use webtrip_core as core;
#[cfg(feature = "web")]
pub use webtrip_web as web;
