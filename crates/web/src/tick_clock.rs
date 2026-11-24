//! What ticks a published transport.
//!
//! `Transport::tick` moves packets between the transport and the stream
//! buffers. With audio running, the worklet's send ring wakes it
//! (`Atomics.waitAsync` on the ring's has-data flag, one tick per audio
//! callback). Only `RingBuffer::write` from the worklet sets that flag, so with
//! no audio nothing would ever tick: a WebRTC connection could not even
//! receive, and its packets would pile up in the transport's queue. The clock
//! therefore falls back to a fixed-period interval while audio is stopped.
//! A connection that needs no tick (WebTransport's worker drives itself) gets
//! no clock at all.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

use crate::audio::audio_callback_loop::{has_atomics_wait_async, AudioCallbackLoop, IntervalTickLoop};
use crate::connection::TransportSlot;

/// What drives ticks right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TickKind {
    None,
    Atomics,
    Interval,
}

/// Which clock a session needs: `connection_needs_tick` is whether a live
/// connection's transport must be ticked at all.
pub(crate) fn tick_kind(audio_running: bool, connection_needs_tick: bool, atomics_available: bool) -> TickKind {
    match (connection_needs_tick, audio_running && atomics_available) {
        (false, _) => TickKind::None,
        (true, true) => TickKind::Atomics,
        (true, false) => TickKind::Interval,
    }
}

/// The connection a clock ticks: its transport slot, and the send ring's
/// has-data flag the audio-driven clock sleeps on.
pub(crate) struct TickSource {
    pub(crate) connection: u64,
    pub(crate) transport: TransportSlot,
    pub(crate) flag_ptr: usize,
}

/// The loop in use. Never read: each stops itself when dropped, which is all
/// the clock needs of it.
#[allow(dead_code)]
enum Running {
    Atomics(AudioCallbackLoop),
    Interval(IntervalTickLoop),
}

/// The session's tick clock: at most one loop, rebuilt whenever audio starts
/// or stops or the connection changes.
#[derive(Default)]
pub(crate) struct TickClock {
    running: Option<(Running, u64, Closure<dyn FnMut()>)>,
}

impl TickClock {
    pub(crate) fn kind(&self) -> TickKind {
        match &self.running {
            None => TickKind::None,
            Some((Running::Atomics(_), ..)) => TickKind::Atomics,
            Some((Running::Interval(_), ..)) => TickKind::Interval,
        }
    }

    /// Make the running loop the one `audio_running` and `source` call for,
    /// leaving it alone when it already is. `source` is the live connection
    /// that needs ticking, if any.
    pub(crate) fn ensure_running_for(&mut self, audio_running: bool, source: Option<&TickSource>) {
        let wanted = tick_kind(audio_running, source.is_some(), has_atomics_wait_async());
        let same_connection = matches!(
            (&self.running, source),
            (Some((_, connection, _)), Some(source)) if *connection == source.connection
        );
        if wanted == self.kind() && (wanted == TickKind::None || same_connection) {
            return;
        }
        self.stop();
        let Some(source) = source else { return };
        if wanted == TickKind::None {
            return;
        }

        // The slot is never borrowed across an await, so the borrow only fails
        // if a tick re-enters itself; skip it then.
        let transport = std::rc::Rc::clone(&source.transport);
        let closure = Closure::<dyn FnMut()>::new(move || {
            if let Ok(mut slot) = transport.try_borrow_mut() {
                if let Some(transport) = slot.as_mut() {
                    transport.tick();
                }
            }
        });
        let tick_fn = closure.as_ref().unchecked_ref::<js_sys::Function>().clone();

        if wanted == TickKind::Atomics {
            let mut callback_loop = AudioCallbackLoop::new();
            match callback_loop.start_with_atomics(&wasm_bindgen::memory(), source.flag_ptr, tick_fn.clone()) {
                Ok(true) => {
                    self.running = Some((Running::Atomics(callback_loop), source.connection, closure));
                    return;
                }
                Ok(false) | Err(_) => {
                    web_sys::console::warn_1(&"⚠️ Atomics.waitAsync not available; ticking on an interval".into());
                }
            }
        }
        match IntervalTickLoop::start(&tick_fn) {
            Ok(interval) => self.running = Some((Running::Interval(interval), source.connection, closure)),
            Err(e) => web_sys::console::error_2(&"Could not start the network tick".into(), &e),
        }
    }

    /// Stop whatever is ticking.
    pub(crate) fn stop(&mut self) {
        // Dropping each loop stops it, before the closure it calls goes.
        self.running = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every combination of what a session can be doing picks the clock that
    /// can actually wake the transport.
    #[test]
    fn the_clock_follows_audio_and_the_connection() {
        use TickKind::*;
        for (audio, needs_tick, atomics, expected) in [
            // No ticking connection: no clock, whatever audio does.
            (true, false, true, None),
            (false, false, true, None),
            // Audio running: the worklet wakes the transport...
            (true, true, true, Atomics),
            // ...unless the browser cannot wait on it.
            (true, true, false, Interval),
            // The regression: connected with no audio must still tick.
            (false, true, true, Interval),
            (false, true, false, Interval),
        ] {
            assert_eq!(
                tick_kind(audio, needs_tick, atomics),
                expected,
                "audio={audio} needs_tick={needs_tick} atomics={atomics}"
            );
        }
    }
}
