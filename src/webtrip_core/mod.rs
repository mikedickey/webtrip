//! Portable core: audio processing, wire protocol, transport contract and the
//! stream primitives that bridge them. Becomes the `webtrip-core` crate in
//! Phase 0c.
//!
//! Dependency rules (see `plans/audio-network-decoupling.md`):
//! - **Subject:** `network` never imports `audio` and `audio` never imports
//!   `network`; both may use `stream`. `session.rs` composes them.
//! - **Platform:** nothing under this tree may depend on `web-sys` or
//!   `js-sys`; only the browser host does (enforced once this is a crate).

pub mod audio;
pub mod network;
pub mod stream;
pub(crate) mod lifecycle;
pub mod session_stats;
