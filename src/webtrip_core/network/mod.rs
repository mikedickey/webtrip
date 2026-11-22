//! Portable network layer: the JackTrip wire protocol and the `Transport`
//! contract.
//!
//! Dependency rules: `network` never imports `audio` (and vice versa); both may
//! use `stream`. Nothing here may depend on `web-sys` or `js-sys`.

pub mod protocol;
pub mod signaling_message;
pub mod transport;

pub use protocol::{AudioFormat, AudioPacket, PacketHeader, StreamStats};
pub use transport::{Transport, TransportState, TransportType};
