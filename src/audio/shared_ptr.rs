//! A raw pointer to a value living in shared WASM linear memory.
//!
//! WebTrip hands buffer addresses across the AudioWorklet / WebTransport-worker
//! boundary as integers — the browser only lets you `postMessage` numbers, not
//! Rust references (see [`docs/ARCHITECTURE.md`](../../../docs/ARCHITECTURE.md)).
//! On the receiving side the address is turned back into a reference.
//!
//! `SharedPtr<T>` is the single place that deref happens. It:
//!
//! - carries the [`Send`]/[`Sync`] assertion **once** (so container structs no
//!   longer need their own `unsafe impl Send`), and
//! - hands out a shared `&T` through one audited `unsafe` block instead of the
//!   `unsafe { &*ptr }` incantation being copy-pasted at every call site.
//!
//! The pointee is expected to coordinate cross-thread access through atomics
//! (e.g. [`RingBuffer`](crate::audio::ring_buffer::RingBuffer)'s `&self` API).
//! For pointees that still expose `&mut self` methods, [`SharedPtr::as_mut`] is
//! provided but marked `unsafe` — see its docs.

/// An integer address into shared WASM linear memory, typed as a pointer to `T`.
///
/// Cheap to copy (it is just an address). Construct with [`SharedPtr::new`] from
/// a raw pointer or [`SharedPtr::from_addr`] from an integer received over
/// `postMessage`.
pub struct SharedPtr<T> {
    ptr: *mut T,
}

// SAFETY: a `SharedPtr` is only an integer address. Moving/sharing that address
// between threads is always sound; any real data-race hazard lives in how the
// *pointee* is accessed, which is the pointee's responsibility to coordinate
// (the buffers live in a `SharedArrayBuffer` and synchronise via atomics). This
// is the one place that assertion is made, rather than an `unsafe impl` per
// container struct that happens to hold one of these pointers.
unsafe impl<T> Send for SharedPtr<T> {}
unsafe impl<T> Sync for SharedPtr<T> {}

// Manual `Copy`/`Clone` (not derived) so they carry no `T: Copy`/`T: Clone`
// bound — the pointee (`RingBuffer`, `Regulator`) is not `Copy`, but its address
// always is.
impl<T> Copy for SharedPtr<T> {}
impl<T> Clone for SharedPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> std::fmt::Debug for SharedPtr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SharedPtr({:#x})", self.addr())
    }
}

impl<T> SharedPtr<T> {
    /// Wrap a raw pointer.
    pub const fn new(ptr: *mut T) -> Self {
        Self { ptr }
    }

    /// A null `SharedPtr` (points at nothing; [`is_null`](Self::is_null) is true).
    pub const fn null() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
        }
    }

    /// Rebuild a `SharedPtr` from an integer address received over `postMessage`.
    pub fn from_addr(addr: usize) -> Self {
        Self {
            ptr: addr as *mut T,
        }
    }

    /// The pointer as an integer address (for handing back across the worker
    /// boundary or for logging).
    pub fn addr(&self) -> usize {
        self.ptr as usize
    }

    /// Whether this points at nothing.
    pub fn is_null(&self) -> bool {
        self.ptr.is_null()
    }

    /// Borrow the pointee as a shared reference, or `None` if the pointer is null.
    ///
    /// This is the sound way to reach the pointee: `&T` may freely alias across
    /// threads, so both the producer and consumer may hold one simultaneously.
    /// The pointee must therefore use interior mutability (atomics) for any
    /// state it mutates through `&self`.
    pub fn as_ref<'a>(&self) -> Option<&'a T> {
        if self.ptr.is_null() {
            return None;
        }
        // SAFETY: non-null here. The caller-side lifecycle (buffers are owned by
        // `WebTripSession`/the worker and are torn down only after the worklet
        // and network loops have stopped, per docs/ARCHITECTURE.md) guarantees
        // the pointee outlives the returned borrow.
        Some(unsafe { &*self.ptr })
    }

    /// Borrow the pointee as a mutable reference, or `None` if the pointer is null.
    ///
    /// # Safety
    ///
    /// This materialises a `&mut T` from a shared address, which asserts
    /// exclusive access to the whole pointee. It is sound only if no other
    /// reference to the pointee is live for the duration of the returned borrow.
    ///
    /// Prefer [`as_ref`](Self::as_ref). This exists for pointees that still
    /// expose `&mut self` methods — currently only
    /// [`Regulator`](crate::audio::regulator::Regulator), whose `push`/`pop` are
    /// driven from different threads. That pattern is not yet provably sound (a
    /// concurrency redesign of the ported jitter buffer is tracked separately);
    /// routing it through here at least keeps the raw deref in one module.
    pub unsafe fn as_mut<'a>(&self) -> Option<&'a mut T> {
        if self.ptr.is_null() {
            return None;
        }
        Some(&mut *self.ptr)
    }
}
