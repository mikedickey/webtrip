//! Portable audio processing and shared parameter state.
//!
//! Dependency rules: `audio` never imports `network` (and vice versa); both may
//! use `stream`. Nothing here may depend on `web-sys` or `js-sys`.

pub mod dsp;
pub mod mixer;
pub mod params;
pub mod processor;

pub use params::AudioParams;
pub use processor::AudioProcessor;
