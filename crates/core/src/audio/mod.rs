//! Portable audio processing and shared parameter state.
//!
//! Dependency rules: `audio` never imports `network` (and vice versa); both may
//! use `stream`. Nothing here may depend on `web-sys` or `js-sys`.

pub(crate) mod dsp;
pub mod mixer;
/// Atomic level, gain and meter state shared between the control and audio threads.
pub mod params;
/// The per-callback processor that feeds a [`mixer::MixEngine`] from the network buffers and publishes meters.
pub mod processor;

pub use params::AudioParams;
pub use processor::AudioProcessor;
