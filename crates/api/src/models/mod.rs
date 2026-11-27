//! Typed request and response models for the JackTrip Virtual Studio API.
//!
//! Each type mirrors one JSON shape from the API's OpenAPI spec, with the same
//! wire names (camelCase unless a type's docs say otherwise). Where the spec
//! composes one shape from another (`allOf`), the shared part is a nested
//! struct marked `#[serde(flatten)]`, so its keys sit at the top level of the
//! JSON object; e.g. a [`ServerWithSubscription`]'s studio fields are on
//! `.server`.
//!
//! **Decoding is lenient.** Nearly every field is an `Option`: a field the
//! server omits decodes as `None`, and fields this crate does not model are
//! ignored. A response therefore never fails to decode because the server
//! added a field, but a misnamed field shows up only as a `None`. When
//! serialized, `None` fields are omitted, so request bodies send only what
//! you set.
//!
//! With the `wasm` feature every model also derives `tsify` and gets a
//! generated TypeScript type of the same name and shape.

// Shared enums and common types
mod common;
pub use common::*;

// Core domain models
mod user;
pub use user::*;

mod studio;
pub use studio::*;

mod device;
pub use device::*;

mod stream;
pub use stream::*;

mod recording;
pub use recording::*;

mod event;
pub use event::*;

mod billing;
pub use billing::*;

mod subscription;
pub use subscription::*;

mod chat;
pub use chat::*;

mod region;
pub use region::*;

// Pagination wrapper types
mod pagination;
pub use pagination::*;

// Shared test utilities
#[cfg(test)]
pub mod test_utils;

// API request/response types
mod requests;
pub use requests::*;

mod responses;
pub use responses::*;
