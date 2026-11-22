//! Browser backend for the `log` facade.
//!
//! Portable code (`webtrip_core`, `webtrip_api`) logs through `log::{debug,
//! warn,error}!` and never touches `web_sys::console`; the browser host
//! installs this backend once in [`init`](crate::init). The logger is a
//! zero-sized static, so it is visible to every agent sharing the WASM memory
//! (the WebTransport worker, the AudioWorklet) without being installed again.

use log::{Level, LevelFilter, Log, Metadata, Record};

struct ConsoleLogger;

static LOGGER: ConsoleLogger = ConsoleLogger;

impl Log for ConsoleLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        let message = wasm_bindgen::JsValue::from_str(&record.args().to_string());
        match record.level() {
            Level::Error => web_sys::console::error_1(&message),
            Level::Warn => web_sys::console::warn_1(&message),
            Level::Info => web_sys::console::info_1(&message),
            Level::Debug | Level::Trace => web_sys::console::debug_1(&message),
        }
    }

    fn flush(&self) {}
}

/// Route `log` records to the browser console. Idempotent: a second install
/// (another agent calling `init`) is ignored.
pub(crate) fn install() {
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(LevelFilter::Debug);
    }
}
