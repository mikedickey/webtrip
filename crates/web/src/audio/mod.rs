//! Browser audio host: device enumeration, the audio engine, and the
//! AudioWorklet integration. The portable processor and parameters live in
//! `webtrip_core::audio`.
//!
//! Dependency rule: `audio` never imports `network` (and vice versa);
//! `session.rs` composes them.

pub mod devices;
pub mod worklet;
pub mod engine;
pub mod audio_callback_loop;

pub use devices::DeviceInfo;
pub use worklet::ProcessorHandle;
pub use engine::AudioEngine;
pub use audio_callback_loop::{AudioCallbackLoop, has_atomics_wait_async};
