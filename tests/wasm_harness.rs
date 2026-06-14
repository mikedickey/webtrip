//! Simple WASM test to demonstrate the wasm-bindgen-test harness is working.
//!
//! This test validates that:
//! 1. wasm-bindgen-test is correctly configured as a dev dependency
//! 2. Tests can be compiled to WASM and executed
//! 3. The test infrastructure can run basic assertions
//!
//! Note: Browser-specific modules (AudioContext, WebRTC, etc.) require running
//! in a browser environment with proper COOP/COEP headers for SharedArrayBuffer.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
fn wasm_harness_works() {
    // Basic assertion to prove the test harness can run
    assert_eq!(2 + 2, 4);
    assert!(true);
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
fn wasm_string_operations() {
    let s = String::from("Hello, WASM!");
    assert_eq!(s.len(), 12);
    assert!(s.contains("WASM"));
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
fn wasm_vec_operations() {
    let mut v = vec![1, 2, 3];
    v.push(4);
    assert_eq!(v.len(), 4);
    assert_eq!(v[3], 4);
}
