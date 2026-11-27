//! Browser audio host: device enumeration, the audio engine, and the
//! AudioWorklet integration. The portable processor and parameters live in
//! `webtrip_core::audio`.
//!
//! Dependency rule: `audio` never imports `network` (and vice versa);
//! `session.rs` composes them.

/// Audio input/output device enumeration and permission prompts.
pub mod devices;
/// The AudioWorklet node and the processor the render thread drives.
#[doc(hidden)]
pub mod worklet;
/// The audio engine: capture, playback and the worklet graph around them.
pub mod engine;
/// The `Atomics.waitAsync` and interval loops the session's tick clock runs on.
pub(crate) mod audio_callback_loop;

pub use devices::DeviceInfo;
pub use engine::AudioEngine;
