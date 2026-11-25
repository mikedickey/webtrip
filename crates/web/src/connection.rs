//! A connection's stream buffers, and the list that keeps retired ones alive
//! until the render thread has let go of them.
//!
//! Every connection allocates a fresh [`Regulator`] and [`RingBuffer`] and
//! never reuses them: the regulator stays strictly single-producer /
//! single-consumer with no reset protocol. When a connection ends its buffers
//! are *retired*, and freed only once nothing can still be reading or writing
//! them: the worklet node that was attached to them has acknowledged
//! `"stopped"` (`worklet.js`), or the `AudioContext` has closed.

use std::cell::RefCell;
use std::rc::Rc;

use webtrip_core::limits::{MAX_CONNECTIONS, MAX_WIRE_FRAMES_PER_CALLBACK, WIRE_SAMPLE_RATE};
use webtrip_core::network::transport::{Transport, TransportType};
use webtrip_core::session_stats::{build_connection_stats, ConnectionStats};
use webtrip_core::stream::regulator::Regulator;
use webtrip_core::stream::ring_buffer::RingBuffer;

/// The published transport, shared with the audio-callback tick closure.
pub(crate) type TransportSlot = Rc<RefCell<Option<Box<dyn Transport>>>>;

/// Initial tolerance the regulator is built with: negative asks for the
/// auto-adaptive tolerance and headroom.
const REGULATOR_TOLERANCE_MS: f64 = -500.0;

/// The buffers one connection moves audio through: the send ring (worklet →
/// network) and the regulator (network → worklet). Both are shared with other
/// agents through raw pointers (the worklet, and for WebTransport its worker),
/// so they are boxed: their addresses must not move.
pub(crate) struct StreamBuffers {
    pub(crate) ring: Box<RingBuffer>,
    pub(crate) regulator: Box<Regulator>,
}

impl StreamBuffers {
    /// Buffers for a connection that sends `send_channels` wide. The
    /// regulator starts at `receive_channels` and `receive_fpp`, which it
    /// replaces with the peer's from its first packet.
    pub(crate) fn new(send_channels: u8, receive_channels: u8, receive_fpp: usize) -> Self {
        let ring = Box::new(RingBuffer::new());
        ring.set_wire_format(send_channels);
        let regulator = Box::new(
            Regulator::with_params(receive_channels as usize, receive_fpp, WIRE_SAMPLE_RATE, REGULATOR_TOLERANCE_MS)
                .with_callback_capacity(MAX_WIRE_FRAMES_PER_CALLBACK),
        );
        Self { ring, regulator }
    }

    pub(crate) fn ring_ptr(&self) -> *mut RingBuffer {
        std::ptr::from_ref::<RingBuffer>(&self.ring).cast_mut()
    }

    pub(crate) fn regulator_ptr(&self) -> *mut Regulator {
        std::ptr::from_ref::<Regulator>(&self.regulator).cast_mut()
    }

    pub(crate) fn stats(&self) -> ConnectionStats {
        build_connection_stats(
            self.regulator.stats(),
            self.regulator.depth(),
            self.regulator.latency_ms(),
            self.regulator.is_initialized(),
            self.ring.available(),
            self.ring.writes(),
            self.ring.samples_written(),
            self.ring.overruns(),
            self.ring.packets_sent(),
        )
    }
}

/// Whether a worklet may still be popping a connection's regulator after the
/// engine it belonged to was stopped. That happens when `AudioContext.close`
/// timed out: nothing then proves the render thread idle, short of the node's
/// own `"stopped"` acknowledgement.
pub(crate) enum ConsumerHold {
    /// No stopped engine may still be reading these buffers
    Clear,
    /// Until the stopped node's acknowledgement settles
    Until(js_sys::Promise),
    /// No acknowledgement can come: the buffers must never be freed
    Forever,
}

/// One network connection: its transport and the buffers it moves audio
/// through.
pub(crate) struct Connection {
    pub(crate) id: u64,
    pub(crate) transport: TransportSlot,
    /// Fixed when the connection opens; the transport itself is only
    /// published once it has connected.
    pub(crate) transport_type: TransportType,
    pub(crate) buffers: StreamBuffers,
    pub(crate) hold: ConsumerHold,
}

/// Refuse a connection beyond [`MAX_CONNECTIONS`], with an error naming the
/// limit, given the number already `live`.
pub(crate) fn check_connection_capacity(live: usize) -> Result<(), String> {
    if live >= MAX_CONNECTIONS {
        Err(format!(
            "Cannot open another connection: a session holds at most {MAX_CONNECTIONS} connection(s)"
        ))
    } else {
        Ok(())
    }
}

/// Values waiting to be freed, each under a key handed back by
/// [`retire`](Self::retire). Dropping an entry is the only way it is freed, so
/// a value nothing frees (a close that timed out) is simply kept.
///
/// An entry may also carry a tag naming what proves it safe: the generation
/// of the audio engine whose worklet node it was attached to
/// ([`retire_for`](Self::retire_for)). A successful `AudioContext.close` of
/// that engine proves every entry with its tag, and no other.
pub(crate) struct RetiredList<T> {
    entries: Vec<(u64, Option<u64>, T)>,
    next_key: u64,
}

impl<T> RetiredList<T> {
    pub(crate) fn new() -> Self {
        Self { entries: Vec::new(), next_key: 0 }
    }

    /// Keep `value` alive; the returned key frees it via [`free`](Self::free).
    pub(crate) fn retire(&mut self, value: T) -> u64 {
        self.retire_tagged(value, None)
    }

    /// Like [`retire`](Self::retire), also freeable as a group through
    /// [`free_tagged`](Self::free_tagged).
    pub(crate) fn retire_for(&mut self, value: T, tag: u64) -> u64 {
        self.retire_tagged(value, Some(tag))
    }

    fn retire_tagged(&mut self, value: T, tag: Option<u64>) -> u64 {
        let key = self.next_key;
        self.next_key += 1;
        self.entries.push((key, tag, value));
        key
    }

    /// Free the entry under `key`. A key already freed is a no-op, so a late
    /// acknowledgement cannot free anything twice.
    pub(crate) fn free(&mut self, key: u64) -> bool {
        match self.entries.iter().position(|(k, ..)| *k == key) {
            Some(index) => {
                self.entries.remove(index);
                true
            }
            None => false,
        }
    }

    /// Free every entry retired under `tag`, returning how many.
    pub(crate) fn free_tagged(&mut self, tag: u64) -> usize {
        let before = self.entries.len();
        self.entries.retain(|(_, t, _)| *t != Some(tag));
        before - self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

impl<T> Drop for RetiredList<T> {
    /// Leak what is left: an entry is freed only by [`RetiredList::free`],
    /// never because the list (or the last handle to it) went away.
    fn drop(&mut self) {
        std::mem::forget(std::mem::take(&mut self.entries));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counts how many of its kind are alive, so the test can see what a free
    /// actually dropped.
    struct Live(Rc<std::cell::Cell<usize>>);
    impl Live {
        fn new(count: &Rc<std::cell::Cell<usize>>) -> Self {
            count.set(count.get() + 1);
            Self(Rc::clone(count))
        }
    }
    impl Drop for Live {
        fn drop(&mut self) {
            self.0.set(self.0.get() - 1);
        }
    }

    #[test]
    fn retired_entries_are_freed_individually() {
        let alive = Rc::new(std::cell::Cell::new(0));
        let mut list = RetiredList::new();
        let first = list.retire(Live::new(&alive));
        let second = list.retire(Live::new(&alive));
        let third = list.retire(Live::new(&alive));
        assert_eq!((list.len(), alive.get()), (3, 3), "retiring keeps the value alive");

        // An acknowledgement frees only the entry it was keyed to.
        assert!(list.free(second));
        assert_eq!((list.len(), alive.get()), (2, 2));

        // A second acknowledgement for the same key frees nothing.
        assert!(!list.free(second));
        assert_eq!(alive.get(), 2);

        // Dropping the list frees nothing still in it.
        let leaked = Rc::new(std::cell::Cell::new(0));
        let mut kept = RetiredList::new();
        kept.retire(Live::new(&leaked));
        drop(kept);
        assert_eq!(leaked.get(), 1, "a dropped list must leak its entries, not free them");

        // Keys are never reused, so a stale key cannot free a later entry.
        assert!(list.free(first) && list.free(third));
        assert_eq!((list.len(), alive.get()), (0, 0));
        let fourth = list.retire(Live::new(&alive));
        assert!(fourth > third, "a freed key must not be handed out again");
        assert!(!list.free(first), "a stale key must not free the new entry");
        assert_eq!(alive.get(), 1);
    }

    /// A tag frees exactly the entries retired under it, so one engine's
    /// successful close cannot free a set another (timed-out) engine left.
    #[test]
    fn a_tag_frees_only_its_own_entries() {
        let alive = Rc::new(std::cell::Cell::new(0));
        let mut list = RetiredList::new();
        list.retire_for(Live::new(&alive), 1);
        let other = list.retire_for(Live::new(&alive), 2);
        list.retire_for(Live::new(&alive), 1);
        let untagged = list.retire(Live::new(&alive));

        assert_eq!(list.free_tagged(1), 2);
        assert_eq!((list.len(), alive.get()), (2, 2));
        assert_eq!(list.free_tagged(1), 0, "a tag already freed frees nothing");
        assert!(list.free(other) && list.free(untagged));
        assert_eq!(alive.get(), 0);
    }

    #[test]
    fn a_connection_beyond_the_limit_is_refused_naming_it() {
        assert!(check_connection_capacity(MAX_CONNECTIONS - 1).is_ok());
        let error = check_connection_capacity(MAX_CONNECTIONS).expect_err("at the limit");
        assert!(error.contains(&MAX_CONNECTIONS.to_string()), "the error must name the limit: {error}");
        assert!(check_connection_capacity(MAX_CONNECTIONS + 1).is_err());
    }
}
