//! Portable core: audio processing, wire protocol, transport contract and the
//! stream primitives that bridge them.
//!
//! Dependency rules (see `plans/audio-network-decoupling.md`):
//! - **Subject:** `network` never imports `audio` and `audio` never imports
//!   `network`; both may use `stream`. `session.rs` composes them.
//! - **Platform:** nothing under this tree may depend on `web-sys` or
//!   `js-sys`; only the browser host does (enforced by Cargo: `npm run check:core`).

pub mod audio;
pub mod network;
pub mod stream;
#[doc(hidden)]
pub mod lifecycle;
pub mod session_stats;
