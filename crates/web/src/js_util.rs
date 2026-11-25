//! Small JS interop helpers shared by the browser hosts. They live at the
//! crate root, not under `audio` or `network`, so neither of those modules has
//! to import the other to reach them.

/// An `Error` with the given `name`. A connect rejects with the DOM's
/// conventional names for an aborted (`"AbortError"`) or timed-out
/// (`"TimeoutError"`) operation, so a caller can tell a cancellation it caused
/// from a failure (a cross-language contract, relied on by the demo).
pub(crate) fn named_error(name: &str, message: &str) -> wasm_bindgen::JsValue {
    let error = js_sys::Error::new(message);
    error.set_name(name);
    error.into()
}

pub(crate) fn cancelled_error() -> wasm_bindgen::JsValue {
    named_error("AbortError", "Connection cancelled by disconnect()")
}

// Helper function for creating a Promise with extractable resolve/reject
pub(crate) fn make_promise() -> (js_sys::Promise, js_sys::Function, js_sys::Function) {
    let mut resolve_fn = None;
    let mut reject_fn = None;
    let promise = js_sys::Promise::new(&mut |resolve, reject| {
        resolve_fn = Some(resolve);
        reject_fn = Some(reject);
    });
    (promise, resolve_fn.unwrap(), reject_fn.unwrap())
}
