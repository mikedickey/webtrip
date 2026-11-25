#![doc = include_str!("../README.md")]
//
// Re-exports only; no logic lives here. Write `::core` inside this crate
// wherever the built-in `core` crate is meant.

pub use webtrip_api as api;
pub use webtrip_core as core;
#[cfg(feature = "web")]
pub use webtrip_web as web;
