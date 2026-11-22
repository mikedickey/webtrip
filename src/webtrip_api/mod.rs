//! Portable JackTrip Virtual Studio API client and models. Becomes the
//! `webtrip-api` crate in Phase 0c.
//!
//! Dependency rule: nothing under this tree may depend on the audio engine, and
//! (once a crate) neither `web-sys` nor `js-sys`.

// Clean API client module: a friendly, ergonomic interface to the JackTrip
// Virtual Studio API.
pub mod api;

// Clean, typed models for the JackTrip API, exposed to JavaScript via
// wasm-bindgen with generated TypeScript types.
pub mod models;
