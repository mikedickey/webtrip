//! Canary test binary proving the wasm-bindgen-test harness itself runs.
//!
//! Per docs/WASM_TESTING.md, every `tests/*.rs` binary needs its own
//! `wasm_bindgen_test_configure!(run_in_browser)`, and a binary with no passing
//! test is silently skipped by the runner. This deliberately trivial test
//! exists so a broken harness (misconfigured browser, missing headers, etc.)
//! shows up as a failure here instead of as silently absent coverage.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::*;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test_configure!(run_in_browser);

/// Deliberate canary: asserts nothing about the crate, only that the harness
/// compiled, launched the browser, and executed a test in this binary.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
fn wasm_harness_works() {
    assert_eq!(2 + 2, 4);
}
