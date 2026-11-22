//! Stream primitives that bridge audio and network: the SPSC ring buffer, the
//! jitter-buffer regulator, and the cross-thread pointer wrapper.
//!
//! Both `audio` and `network` may use these. Nothing here may depend on
//! `web-sys` or `js-sys`.

pub mod regulator;
pub mod ring_buffer;
pub mod shared_ptr;

pub use regulator::{Regulator, RegulatorStats};
pub use ring_buffer::RingBuffer;
