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

### Build/test flag parity

`test:wasm` compiles the test binaries with the **same** flags as
`build:wasm`/`check`/`coverage:wasm`, so browser tests exercise the binary we
actually ship. To keep them in lockstep, the flags live in **one place** — the
`config` block of `package.json`, exposed to each script as environment
variables:

- `$npm_package_config_web_sys_cfg` — `--cfg=web_sys_unstable_apis` (needed by
  *every* build, native and WASM).
- `$npm_package_config_wasm_rustflags` — the `+atomics` + shared-memory/TLS
  linker block below (WASM only).

Each script concatenates these (e.g. `coverage:wasm` appends its
instrumentation flags on top). Edit the flags in the `config` block, not in the
individual scripts. The flag set comprises:

- `-Ctarget-feature=+atomics` (in `RUSTFLAGS`) — without it,
  `core::sync::atomic` lowers to *non-atomic* instructions on
  `wasm32-unknown-unknown`, so e.g. the `ring_buffer.rs` atomics test would
  otherwise run against different generated code than production.
- `-Zbuild-std=std,panic_abort` — passed to cargo via
  `wasm-pack test … -- -Zbuild-std=std,panic_abort`.
- The shared-memory/TLS linker flags (`--shared-memory --import-memory
  --max-memory=… --export=__wasm_init_tls/__tls_*/__heap_base`) — these make the
  module **import** a shared `WebAssembly.Memory`, matching the app's ABI.

The `wasm-pack`/`wasm-bindgen-test` versions in this repo generate a harness
loader that supplies a shared `WebAssembly.Memory{shared:true}` import and a
cross-origin-isolated test page, so the full shared-memory build instantiates
and runs headless. (You can confirm the test binary imports shared memory:
`env.memory … shared=true`.) If a future toolchain bump breaks this, the
fallback is to drop **only** the linker/memory ABI flags from `test:wasm` while
keeping `+atomics`/`-Zbuild-std`, and document the exact error here — never
silently ship a build/test flag mismatch.

### Requirements

- Chrome or Chromium browser installed
- `wasm-pack` installed (`cargo install wasm-pack`)
- For headless mode, ensure Chrome can run with `--no-sandbox` if on Linux

### Test Organization (convention)

1. **Unit / module tests**: Place `#[wasm_bindgen_test]` tests **inline** in the
   module's `#[cfg(test)] mod tests` block, alongside the native `#[test]`
   tests, gated on `#[cfg(target_arch = "wasm32")]` (e.g.
   `src/audio/ring_buffer.rs`, `src/audio/webrtc.rs`). This keeps browser tests
   next to the code they cover and lets them reuse `pub(crate)` helpers and
   private fields without widening visibility.
2. **Integration tests**: Place in `tests/*.rs` for standalone test files (e.g.
   `tests/wasm_harness.rs`).

#### `run_in_browser` placement — shared once per binary

`wasm_bindgen_test_configure!(run_in_browser)` must appear **exactly once per
test binary**. To avoid duplicating it across modules (DRY, per AGENTS.md):

- **Library unit-test binary** (everything under `src/`): the single invocation
  lives in [`src/test_support.rs`](../src/test_support.rs). Individual modules
  (`ring_buffer`, `webrtc`, future `engine`, …) only `use
  wasm_bindgen_test::wasm_bindgen_test;` and add `#[wasm_bindgen_test]`
  functions — they must **not** repeat the configure call. `test_support` also
  holds reusable assertions like `assert_valid_sdp`.
- **Each `tests/*.rs` integration file** is its own binary and therefore needs
  its own `wasm_bindgen_test_configure!(run_in_browser)`.

### Writing WASM Tests

A browser test inside a `src/` module (the lib unit-test binary) — the
`run_in_browser` opt-in is already provided once by `crate::test_support`, so
the module only imports the attribute:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "wasm32")]
    use wasm_bindgen_test::wasm_bindgen_test;

    #[cfg(target_arch = "wasm32")]
    #[wasm_bindgen_test]
    fn my_browser_test() {
        // Test code that uses web_sys types.
        assert_eq!(2 + 2, 4);
    }
}
```

A standalone `tests/*.rs` integration file is its own binary, so it declares the
configure call itself:

```rust
#[cfg(target_arch = "wasm32")]
use wasm_bindgen_test::*;

// REQUIRED in each tests/*.rs binary — without it the suite is skipped under
// `--chrome` (see Troubleshooting). The lib unit-test binary gets this from
// crate::test_support instead.
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test_configure!(run_in_browser);
```

**`wasm_bindgen_test_configure!(run_in_browser);` is required, not optional.**
`wasm-bindgen-test` defaults to running in Node.js. Since `npm run test:wasm`
passes `--chrome`, any binary *without* this directive (exactly once) is
silently skipped (see
[Troubleshooting](#this-test-suite-is-only-configured-to-run-in-nodejs)). See
[Test Organization](#test-organization-convention) for where it lives.

## Continuous Integration

WASM tests run in CI via the `build` job in `.github/workflows/ci.yml`, inside
the toolchain container (`containers/build/Containerfile`). The browser setup
(`chromium` + `chromium-driver`, `CHROMEDRIVER`, and the root-level
`webdriver.json` flags) is documented at those sources.

## Code Coverage

Coverage spans the **whole** Rust surface by combining two runs:

| Command | Target | Output | Covers |
|---------|--------|--------|--------|
| `npm run coverage` | native host | `lcov.info` | `#[test]` logic + `#[cfg(not(target_arch = "wasm32"))]` paths |
| `npm run coverage:wasm` | `wasm32-unknown-unknown` | `lcov.wasm.info` | `#[wasm_bindgen_test]` + `#[cfg(target_arch = "wasm32")]` paths |

CI runs both and uploads both files; Codecov unions them by `file:line`, so a
line covered by *either* run counts as covered. This is why the native-only
report previously hid the browser/transport surface: `#[cfg(target_arch =
"wasm32")]` code is compiled *out* of the native build, so it never appeared in
`lcov.info` at all (not even as 0%) — it was excluded from the denominator.

`coverage:wasm` uses the experimental `wasm-bindgen-test` coverage path
(see the [wasm-bindgen guide](https://wasm-bindgen.github.io/wasm-bindgen/wasm-bindgen-test/coverage.html)).
The mechanism, encoded in the npm script:

- **`-Cinstrument-coverage -Zno-profiler-runtime`** — instrument, but skip the
  default LLVM profiler runtime; the `minicov` crate (a transitive dev-dep of
  `wasm-bindgen-test`) provides the WASM-side runtime instead.
- **`--cfg=wasm_bindgen_unstable_test_coverage`** — opts the test runner into
  writing `.profraw` data out of the browser.
- **`-Clink-args=--no-gc-sections`** — keeps coverage symbols from being
  stripped by the linker.
- **`CFLAGS_wasm32_unknown_unknown="-matomics -mbulk-memory"`** — `minicov`
  compiles its runtime C via the `cc` crate; on our `+atomics` build that C must
  be compiled with the matching wasm features or the instrumented module fails to
  link. Required, not optional, because we build with `-Ctarget-feature=+atomics`.
- Flags go through `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS` (not plain
  `RUSTFLAGS`) so host-built build scripts / proc-macros stay uninstrumented.

All of the above (and the shared `web_sys`/`+atomics` blocks) live in the
`config` section of `package.json` as the single source of truth. The build
container (`containers/build/Containerfile`) installs `clang` for minicov's C
runtime and pre-warms the `test:wasm` dep graph, but **not** the coverage
profile — a test-less stub can't link the instrumented build (it lacks the
minicov `__llvm_profile_runtime` reference a real harness provides), so
`coverage:wasm` recompiles its deps + `std` once per CI run, as native coverage
does.

These coverage flags are layered **on top of** the same
`+atomics`/shared-memory/TLS flags and `-Zbuild-std=std,panic_abort` that
`test:wasm` uses (see [flag parity](#buildtest-flag-parity)) — coverage must
instrument the *same* generated code the browser tests and the shipped build run,
not a different, unthreaded build. `coverage:wasm` drives the run with
`cargo llvm-cov test` (rather than `wasm-pack test`) so cargo-llvm-cov owns the
`.profraw` → `lcov` plumbing, but the underlying runner is still
`wasm-bindgen-test-runner`, so the browser/shared-memory harness behaves the same.

Requirements (all satisfied by the toolchain container): Rust ≥ 1.87,
`wasm-bindgen-test` ≥ 0.3.57, `llvm-tools-preview`, and a `cargo-llvm-cov` that
drives the wasm32 target.

## Limitations

### SharedArrayBuffer / shared-memory Requirements

The app proper needs `SharedArrayBuffer`, which requires a cross-origin-isolated
page (`Cross-Origin-Opener-Policy: same-origin` +
`Cross-Origin-Embedder-Policy: require-corp`, set by `server.js`).

For `npm run test:wasm`, the current `wasm-pack`/`wasm-bindgen-test` harness
serves a test page and module loader that already provide a shared
`WebAssembly.Memory{shared:true}` import and the isolation the threaded module
needs, so the full shared-memory build (see
[flag parity](#buildtest-flag-parity)) instantiates and runs headless without
extra setup. If a toolchain bump regresses this (e.g. `SharedArrayBuffer is not
defined` at instantiation), options are:
1. Run in a real (non-headless) browser via `wasm-pack test` without `--headless`
2. Serve with proper COOP/COEP headers (e.g. `npm run serve`)
3. As a documented last resort, drop **only** the linker/memory ABI flags from
   `test:wasm` (keeping `+atomics`/`-Zbuild-std`) and record the exact error.

### Browser Compatibility

- **Chrome/Chromium**: Full support for WebTransport, SharedArrayBuffer
- **Firefox**: Requires configuration for some features
- **Safari**: Limited WebTransport support

## Troubleshooting

### "This test suite is only configured to run in node.js"

The full message reads:

```
This test suite is only configured to run in node.js, but we're only running
browser tests so skipping.
```

The suite was compiled and the test binary ran, but every test was skipped
because the crate/file is missing the browser opt-in. Add this once per test
binary:

```rust
#[cfg(target_arch = "wasm32")]
wasm_bindgen_test_configure!(run_in_browser);
```

Note this is *per test binary* — adding it to one file does not cover the
others. See [Writing WASM Tests](#writing-wasm-tests).

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
- `src/audio/webrtc.rs`: WebRTC glue against `web_sys` — `RtcPeerConnection`
  creation from a `TransportConfig`, `create_offer` SDP validity, data-channel
  initial state, and ICE-candidate JSON → `RtcIceCandidate` parsing
- `src/test_support.rs`: shared browser-test scaffolding (`run_in_browser`
  opt-in for the lib binary, `assert_valid_sdp`)

## Future Work

As the test harness is established, we can add tests for:
- `src/audio/engine.rs`: AudioContext setup / feature detection
- `src/audio/devices.rs`: MediaDevices enumeration  
- `src/audio/worklet.rs`: AudioWorklet communication
- `src/session.rs`: Full session integration
