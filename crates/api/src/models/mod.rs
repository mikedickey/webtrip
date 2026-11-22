//! JackTrip Virtual Studio API Models
//!
//! Clean, typed data models for the JackTrip API. All models are serializable
//! via serde and automatically generate TypeScript types via tsify.

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

