# WASM Testing Guide

This document describes how to run WebAssembly tests for browser-only modules.

## Overview

The project uses `wasm-bindgen-test` to test code that depends on browser APIs (Web Audio, WebRTC, WebTransport, etc.). These tests run in an actual browser environment (or headless browser) to access `web_sys` types.

## Running WASM Tests

### Local Development

```bash
npm run test:wasm
```

This command:
- Compiles the test suite to WASM
- Launches a headless Chrome browser
- Runs all tests marked with `#[wasm_bindgen_test]`
- Reports results back to the console

### Requirements

- Chrome or Chromium browser installed
- `wasm-pack` installed (`cargo install wasm-pack`)
- For headless mode, ensure Chrome can run with `--no-sandbox` if on Linux

### Test Organization

1. **Unit tests**: Place `#[wasm_bindgen_test]` tests alongside native `#[test]` tests in module test blocks
2. **Integration tests**: Place in `tests/*.rs` for standalone test files

### Writing WASM Tests

```rust
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test]
fn my_browser_test() {
    // Test code that uses web_sys types
    assert_eq!(2 + 2, 4);
}
```

For tests that need to run in a browser (not Node.js), add this to your test module:

```rust
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test_configure!(run_in_browser);
```

## Continuous Integration

### GitHub Actions Example

```yaml
name: WASM Tests

on: [push, pull_request]

jobs:
  wasm-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@nightly
      - uses: Swatinem/rust-cache@v2
      
      - name: Install wasm-pack
        run: cargo install wasm-pack
      
      - name: Install Chrome
        uses: browser-actions/setup-chrome@latest
      
      - name: Run WASM tests
        run: npm run test:wasm
```

## Limitations

### SharedArrayBuffer Requirements

Tests using threading features (atomics, shared memory) require special HTTP headers:
- `Cross-Origin-Opener-Policy: same-origin`
- `Cross-Origin-Embedder-Policy: require-corp`

Headless browsers may not support these headers properly. In such cases:
1. Run tests in a real browser with `wasm-pack test` (without `--headless`)
2. Use a local dev server that sets proper headers (e.g., `npm run serve`)
3. Run tests in CI with browsers configured for cross-origin isolation

### Browser Compatibility

- **Chrome/Chromium**: Full support for WebTransport, SharedArrayBuffer
- **Firefox**: Requires configuration for some features
- **Safari**: Limited WebTransport support

## Troubleshooting

### "ChromeDriver was killed" or "HTTP 404"

This usually means:
- ChromeDriver version mismatch with Chrome
- Missing Chrome dependencies
- Insufficient permissions to run Chrome in headless mode

Solution:
```bash
# On Linux, ensure Chrome can run with sandbox disabled
google-chrome --headless --no-sandbox --version
```

### "exports is not defined"

This error occurs when trying to run browser-targeted code in Node.js. Ensure you're using `--headless --chrome` or `--headless --firefox`, not `--node`.

### "SharedArrayBuffer is not defined"

The test requires threading support. Either:
1. Run in a browser with proper COOP/COEP headers
2. Simplify the test to not use threading features
3. Use a test server that sets the required headers

## Current Test Coverage

- `tests/wasm_harness.rs`: Basic harness validation tests
- `src/audio/ring_buffer.rs`: WASM test for RingBuffer basic operations

## Future Work

As the test harness is established, we can add tests for:
- `src/audio/engine.rs`: AudioContext setup
- `src/audio/devices.rs`: MediaDevices enumeration  
- `src/audio/worklet.rs`: AudioWorklet communication
- `src/audio/webrtc.rs`: WebRTC data channels
- `src/session.rs`: Full session integration
