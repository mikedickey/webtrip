//! A raw address of a `T: Sync` value shared across threads.
//!
//! Hosts hand buffer addresses between threads as plain integers. In the
//! browser build that is how a buffer crosses into the AudioWorklet or the
//! WebTransport worker, since `postMessage` carries numbers, not Rust references
//! (see [Architecture](https://webtrip.dev/docs/architecture)). On the receiving
//! side the address is turned back into a reference.
//!
//! `SharedPtr<T>` is the single place that address reconstruction and deref
//! happen. It:
//!
//! - carries the [`Send`]/[`Sync`] assertion **once**, bounded on `T: Sync` (so
//!   container structs no longer need their own `unsafe impl Send`, and a
//!   pointee that is not thread-safe fails to compile rather than being
//!   laundered across threads),
//! - makes [`new`](SharedPtr::new) / [`from_addr`](SharedPtr::from_addr)
//!   `unsafe` so each call site must acknowledge that the address points at a
//!   live `T` for as long as anything may borrow through the pointer (the one
//!   safe constructor, [`from_static`](SharedPtr::from_static), takes a
//!   `&'static T`, which already guarantees that), and
//! - hands out a shared `&T` through one audited `unsafe` block inside
//!   [`as_ref`](SharedPtr::as_ref) instead of the `unsafe { &*ptr }`
//!   incantation being copy-pasted at every call site.
//!
//! The pointee must coordinate cross-thread access through atomics and/or
//! interior mutability, and say so by being `Sync` (e.g. [`RingBuffer`](crate::stream::ring_buffer::RingBuffer)
//! and [`Regulator`](crate::stream::regulator::Regulator)'s `&self` APIs). Every
//! borrow is a shared one: there is no way to obtain a `&mut T` through a
//! `SharedPtr`.

/// A shared address, typed as a pointer to `T`, that may cross threads.
///
/// Cheap to copy (it is just an address). Construct with [`SharedPtr::new`] from
/// a raw pointer or [`SharedPtr::from_addr`] from an integer received from
/// another thread — both are `unsafe` and require a live pointee — or safely
/// with [`SharedPtr::from_static`] from a `&'static T`.
pub struct SharedPtr<T> {
    ptr: *mut T,
}

// SAFETY: a `SharedPtr` is an integer address that [`as_ref`](SharedPtr::as_ref)
// turns into a `&T` on whichever thread holds it, so sending or sharing one
// across threads is equivalent to sharing `&T` — sound exactly when `T: Sync`.
// The bound makes the compiler check that the pointee coordinates cross-thread
// access itself (atomics / `UnsafeCell` with a proven protocol) instead of this
// impl asserting it for any `T`. Lifetime (the pointee outliving every borrow)
// is asserted at construction (`new` / `from_addr`); see those `# Safety`
// sections. This is the one place the Send/Sync assertion is made, rather than
// an `unsafe impl` per container struct that happens to hold one of these
// pointers.
unsafe impl<T: Sync> Send for SharedPtr<T> {}
unsafe impl<T: Sync> Sync for SharedPtr<T> {}

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
    ///
    /// # Safety
    ///
    /// `ptr` must be null, or point to a live `T` that outlives every borrow
    /// obtained through [`as_ref`](Self::as_ref). Reconstructing from an
    /// arbitrary address and then calling that method is otherwise undefined
    /// behaviour — that is why construction, not deref, is the `unsafe`
    /// boundary.
    pub const unsafe fn new(ptr: *mut T) -> Self {
        Self { ptr }
    }

    /// Point at a value that lives for the rest of the program.
    ///
    /// Safe because a `&'static T` already guarantees what [`new`](Self::new)
    /// asks its caller to: the pointee outlives every borrow that
    /// [`as_ref`](Self::as_ref) can hand out. (A `SharedPtr` is `Copy` and may
    /// outlive a shorter borrow, which is why no `&'a T` constructor exists.)
    pub fn from_static(value: &'static T) -> Self {
        // SAFETY: `value` lives for `'static`, so no borrow through `as_ref`
        // can outlive it.
        unsafe { Self::new(std::ptr::from_ref(value).cast_mut()) }
    }

    /// A null `SharedPtr` (points at nothing; [`is_null`](Self::is_null) is true).
    pub const fn null() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
        }
    }

    /// Rebuild a `SharedPtr` from an integer address, typically one received
    /// from another thread (in the browser build, over `postMessage`).
    ///
    /// # Safety
    ///
    /// `addr` must be 0 (null), or the address of a live `T` that outlives
    /// every borrow obtained through [`as_ref`](Self::as_ref). The sender must
    /// have published a real allocation that remains alive for the receiver's
    /// use.
    pub const unsafe fn from_addr(addr: usize) -> Self {
        Self {
            ptr: addr as *mut T,
        }
    }

    /// The pointer as an integer address (for handing to another thread or for
    /// logging).
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
    /// The pointee must therefore use interior mutability (atomics / `UnsafeCell`)
    /// for any state it mutates through `&self`.
    ///
    /// Safe because [`new`](Self::new) / [`from_addr`](Self::from_addr) already
    /// required the address to point at a live `T` that outlives every borrow.
    pub fn as_ref<'a>(&self) -> Option<&'a T> {
        if self.ptr.is_null() {
            return None;
        }
        // SAFETY: non-null; construction (`new` / `from_addr`) asserted the
        // pointee is live for the duration of every borrow obtained here.
        Some(unsafe { &*self.ptr })
    }
}
