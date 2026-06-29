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

### Fake media-device flags (required for device/capture tests)

`webdriver.json` launches headless Chrome with two extra flags beyond the usual
`--headless`/`--no-sandbox`/`--disable-dev-shm-usage`:

- `--use-fake-device-for-media-stream` — gives Chrome a **synthetic** audio
  input (a generated tone) so `enumerateDevices()` reports a real input device.
  Without it, headless Chrome enumerates an **empty** device list and the
  `src/audio/devices.rs` enumeration tests would have nothing to assert on.
- `--use-fake-ui-for-media-stream` — **auto-grants** the microphone permission
  with no user gesture, so `getUserMedia()` resolves to a `MediaStream` instead
  of rejecting (headless Chrome has no permission UI and otherwise denies the
  request). Granting permission is also what makes device **labels** visible in
  `enumerateDevices()`.

These let the browser tests for `get_media_devices`, `request_audio_permission`
/`getUserMedia`, `stop_media_stream`, `enumerate_devices`, and
`get_audio_devices` run headless. They are harmless to the other suites (no real
hardware is touched). One headless caveat remains: even with the flags, Chrome
does not reliably expose an audio **output** (`audiooutput`) sink, so the device
tests assert on the input list and only validate outputs when present.

### Requirements

- Chrome or Chromium browser installed
- `wasm-pack` installed (`cargo install wasm-pack`)
- For headless mode, ensure Chrome can run with `--no-sandbox` if on Linux
- For the media-device tests, the fake-device flags above (already in
  `webdriver.json`)

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

## Integration tests (real JackTrip server)

The unit tests above stop at the transport boundary (`MockTransport` or pure
logic). The **integration tests** drive the real WASM client against an actual
`jacktrip/jacktrip:edge` hub server in Docker, for **both** the WebRTC and
WebTransport transports — the live surface unit tests can't reach (WebRTC's
WebSocket signaling + SDP/ICE + data-channel open; WebTransport's QUIC worker
loops).

```bash
npm run test:integration       # builds the app, then drives tests/integration/run.mjs
npm run test:integration:run   # inner-loop: skip the build, reuse existing pkg/ + dist/
```

### How it works (served-app + browser driver)

Unlike the unit tests, these do **not** use `wasm-bindgen-test`. The WebTransport
worker loads the wasm module from `{origin}/pkg/webtrip.js`
(`src/audio/webtransport.rs::wasm_module_url`), which only resolves when the page
is served at the site root with `pkg/` present — something the
`wasm-bindgen-test` harness origin does not do. So `tests/integration/run.mjs`:

1. serves the real app via `server.js` over plain HTTP on `localhost` (a secure
   context, so its COOP/COEP headers still yield `crossOriginIsolated` +
   WebTransport — no cert needed for the page),
2. drives it with a headless browser (`puppeteer-core`), and
3. for each transport, runs the actual exported session API
   (`createAudioParams` → `WebTripSession` → `setTransportType` →
   `connectToStudio`), then asserts the transport reports **connected** and that
   captured (fake-device) audio reaches the **send** ring buffer
   (`ring_buffer_samples_written > 0`). The receive/loopback counters are logged
   but not asserted (a lone hub client may get no echo).

The browser uses fake-media-device + `--autoplay-policy=no-user-gesture-required`
flags so capture/playback run headless without a real mic or user gesture.

### Why the `*.miked.io` cert + `localhost.miked.io`

The cert is needed by the **JackTrip server**, not the test script. WebRTC
`wss://` and WebTransport HTTP/3 require a browser-trusted TLS cert valid for the
connection hostname — a bare `127.0.0.1` can't provide one, and WebTransport's
cert check can't be bypassed. `localhost.miked.io` resolves to `127.0.0.1` via
public DNS, so a loopback JackTrip server can present the repo's CA-trusted
`*.miked.io` wildcard cert, which the browser accepts. Hosts/ports are
overridable via `JACKTRIP_TEST_HOST` / `JACKTRIP_TEST_PORT` (and `APP_HOST` /
`APP_PORT` for the page server); see the env knobs at the top of
`tests/integration/run.mjs`.

### Running locally

`npm run test:integration` does **not** need a cert — it serves the app over
plain HTTP on `localhost`. Only the JackTrip server needs the (uncommitted)
`*.miked.io` cert/key. If you already have a JackTrip server running with a
trusted cert, just point the test at it; otherwise start one via the bundled
compose file (pass `JACKTRIP_CERT_DIR` to *that*). Set
`PUPPETEER_EXECUTABLE_PATH` if your Chrome/Chromium isn't auto-detected.

```bash
# Option A: server already running in another terminal on localhost.miked.io:4464
npm run test:integration

# Option B: start the server via the bundled compose file (needs the cert)
JACKTRIP_CERT_DIR=/path/to/certs docker compose -f tests/integration/docker-compose.integration.yml up -d
npm run test:integration
docker compose -f tests/integration/docker-compose.integration.yml down
```

### CI

The `integration` job in `.github/workflows/ci.yml` runs on every push and on
PRs from branches in this repo. It writes the cert/key from the `MIKED_TLS_CERT`
/ `MIKED_TLS_KEY` secrets, starts the JackTrip server with host networking, then
builds the app and runs the harness inside the build image (chromium at
`/usr/bin/chromium`) on the same network. **Fork PRs are skipped** — GitHub does
not expose secrets to them.

## Code Coverage

Coverage spans the **whole** Rust surface by combining three runs:

| Command | Target | Output | Covers |
|---------|--------|--------|--------|
| `npm run coverage` | native host | `lcov.info` | `#[test]` logic + `#[cfg(not(target_arch = "wasm32"))]` paths |
| `npm run coverage:wasm` | `wasm32-unknown-unknown` | `lcov.wasm.info` | `#[wasm_bindgen_test]` + `#[cfg(target_arch = "wasm32")]` paths |
| `npm run coverage:integration` | `wasm32-unknown-unknown` | `lcov.integration.info` | the *live* transport surface (WebRTC signaling/SDP/ICE/data-channel, WebTransport QUIC worker, protocol wire) the unit suites can't reach |

CI runs all three and uploads all three files; Codecov unions them by
`file:line`, so a line covered by *any* run counts as covered. This is why the native-only
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

### Integration coverage (`coverage:integration`)

`coverage:wasm` covers what `#[wasm_bindgen_test]` exercises, but the live
transport surface — WebRTC SDP/ICE/data-channel negotiation, the WebTransport
QUIC **worker**, and the protocol wire path — only runs when the real client
talks to a real JackTrip server. `coverage:integration` captures *that* run:

1. **`build:wasm:coverage`** builds the actual app (`wasm-pack build --target
   web`) with the same instrument-coverage flags as `coverage:wasm` layered on,
   plus `--features coverage`. `--dev` keeps `wasm-opt` off so the
   `__llvm_covmap`/`__llvm_covfun` sections survive. The `coverage` feature pulls
   in `minicov` and enables a `__coverageDump` export (`src/lib.rs`).
2. The harness (`tests/integration/run.mjs`, with `INTEGRATION_COVERAGE` set)
   drives both transports against the live server, then calls `__coverageDump`
   and writes `coverage/integration.profraw`.
3. **`scripts/integration-coverage-report.sh`** runs `llvm-profdata` +
   `llvm-cov export` against `pkg/webtrip_bg.wasm`, filtering out std/build-std
   and registry sources, to produce `lcov.integration.info`.

**The WebTransport worker is covered by the same single dump.** Its counters
live in the module's linear memory, and the worker is handed
`wasm_bindgen::memory()` at init (it shares that memory for the SPSC ring
buffers), so one `__coverageDump` call from the main thread captures the worker
thread's execution too — no per-thread profile merge needed.

This run needs a live JackTrip server, so the CI `integration` job (which has
the `*.miked.io` cert) owns it, uploading under the **`integration` Codecov flag
with `carryforward: true`** (see `codecov.yml`). Fork PRs skip the job for lack
of the `MIKED_TLS_*` secrets, so they inherit the last integration coverage
rather than reading those lines as a regression. Locally it needs `clang` and
the LLVM tools on `PATH` (`brew install llvm`); see [Local setup](#local-setup-macos).

## Local setup (macOS)

CI runs everything inside the toolchain container, which already has the tools
below. On a local macOS checkout, `npm run test:wasm` and `npm run coverage:wasm`
need three things the system doesn't provide by default. Both scripts route
through [`scripts/wasm-browser-env.sh`](../scripts/wasm-browser-env.sh), which
auto-configures what it can and otherwise **fails fast with the exact fix** — so
in practice you only act when it tells you to. The wrapper is a no-op in CI (the
container already satisfies every check). What it handles:

1. **A wasm-capable `clang` (coverage only).** `minicov` (the WASM coverage
   runtime) compiles C to `wasm32` via the `cc` crate, and Apple's
   `/usr/bin/clang` has no `wasm32` target. Install Homebrew LLVM once:

   ```sh
   brew install llvm
   ```

   The wrapper auto-prepends `$(brew --prefix llvm)/bin` to `PATH` when present;
   you do **not** need to edit your shell profile.

2. **A `wasm-bindgen-cli` matching the pin (coverage only).** `coverage:wasm`
   uses the `wasm-bindgen-test-runner` from `PATH`; it must equal the
   `wasm-bindgen` version pinned in `Cargo.lock`. The wrapper validates this by
   reading that pin from `Cargo.lock` directly. If the wrapper reports a
   mismatch, run the command it prints, e.g.:

   ```sh
   cargo install wasm-bindgen-cli --version 0.2.118 --locked
   ```

3. **A `chromedriver` matching your installed Chrome (both scripts).** `wasm-pack`
   otherwise auto-downloads a driver whose major version may not match Chrome;
   the mismatched driver is then `SIGKILL`ed at session start (symptom:
   `driver status: signal: 9 (SIGKILL)` / `http status: 404`). Install a matching
   driver and the wrapper will pick it up from `~/.local/bin/chromedriver`
   (or set `CHROMEDRIVER` yourself):

   ```sh
   # read the installed Chrome's major version (e.g. 126)
   major=$("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --version | grep -oE '[0-9]+' | head -1)
   # install a matching driver; it prints the path it wrote
   npx @puppeteer/browsers install "chromedriver@$major"
   # copy that driver to where the wrapper looks (create the dir first)
   mkdir -p ~/.local/bin
   cp chromedriver/*/chromedriver-mac-*/chromedriver ~/.local/bin/chromedriver
   ```

   When Chrome later auto-updates to a new major version, refresh that driver.

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
- `src/audio/engine.rs`: AudioContext bootstrap — `AudioEngine::create` builds a
  real `AudioContext` reporting a plausible (>0, in-range) sample rate
- `src/audio/worklet.rs`: worklet module registration via the `dependent_module!`
  Blob/URL path (`register_audio_worklet` resolves against `audioWorklet.addModule`)
- `src/audio/webtransport.rs`: `is_webtransport_available` reports `true` in the
  headless Chrome harness (inverse of the native check); `create_worker` builds
  a `web_sys::Worker` via the `dependent_module!` Blob-URL flow and registers the
  `onmessage`/`onerror` closures; `build_init_message` (the assembly behind
  `init_worker`) layers the browser-only `wasmMemory` (a `WebAssembly.Memory`)
  and `wasmUrl` (the bindgen glue URL) onto the plain-JSON buffer config (live
  `connect()` needs an HTTP/3 server — skipped)
- `src/audio/webtransport_worker.rs`: worker-side `#[wasm_bindgen]` entry points
  called directly — `worker_init`+`worker_get_stats` (zeroed stats prove init
  ran) and `handle_worker_message` routing for `init`/`getStats`/`disconnect`
  (resolves) plus an unknown type (rejects). The server-dependent
  `worker_connect`/`send_loop`/`receive_loop` paths are intentionally not covered
- `src/audio/audio_callback_loop.rs`: `has_atomics_wait_async` reports `true`
  under the shared-memory harness, plus an end-to-end smoke test of the
  `Atomics.waitAsync` wake-up path (set flag + `Atomics.notify` → tick fires),
  which exercises the imported shared `WebAssembly.Memory` / `SharedArrayBuffer`
- `src/audio/devices.rs`: MediaDevices glue around the native categorization
  core — `get_media_devices` reaches `navigator.mediaDevices`,
  `request_audio_permission`/`getUserMedia` resolves under the fake-device flags,
  `stop_media_stream` ends a live stream's tracks, and `get_audio_devices`
  returns a populated `{ inputDevices, outputDevices }` object (requires the
  fake-device flags above)
- `src/test_support.rs`: shared browser-test scaffolding (`run_in_browser`
  opt-in for the lib binary, `assert_valid_sdp`, `sleep_ms` async yield helper)
- `src/session.rs`: the async connect/disconnect state machine over the
  server-free `MockTransport` — a full `Idle → Connecting → Connected → Idle`
  cycle (via `state()` and the `on_state_change` callback order), the
  `AudioContext`-backed `is_audio_suspended`/`resume_audio` both with a live
  engine (after a mock connect) and on the no-engine branch, plus the
  invalid-host fast-fail path. The capture path inside connect uses
  `getUserMedia`, enabled headless by the synthetic-device flags in
  `webdriver.json` (`--use-fake-device-for-media-stream` /
  `--use-fake-ui-for-media-stream`)
