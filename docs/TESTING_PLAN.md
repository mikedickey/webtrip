# WebTrip Unit Test Coverage Plan

This document enumerates the proposed Linear tasks for the **Webtrip testing** project. Each entry is a self-contained, reviewable, deliverable piece of work — one task per PR.

## Current state

Native tests run via `npm run test` (i.e. `cargo test` with `--cfg=web_sys_unstable_apis`). Only four modules currently have tests:

| Module | Tests | Notes |
|---|---|---|
| `src/audio/protocol.rs` | 12 | JackTrip wire format, channel encoding, sample rate |
| `src/audio/signaling.rs` | 4 | Hub signaling JSON parsing |
| `src/audio/regulator.rs` | 8 | Jitter buffer, Burg PLC, wraparound |
| `src/audio/webtransport.rs` | 1 | Availability check |

Everything else (≈ 35 source files, ~4.5 kLOC of pure-logic code) has **no** unit tests.

## Constraints

- `cargo test` runs on the native target. Anything that touches `web_sys` types (e.g. `AudioContext`, `RtcPeerConnection`, `WebTransport`) cannot be unit-tested with the current setup — those modules need a future `wasm-bindgen-test` integration story (out of scope for this initiative).
- `wasm_bindgen` macros alone compile fine on native, so any module that only uses `#[wasm_bindgen]` for JS interop but does its work in plain Rust is fair game.
- No mocking framework is in place. Tasks that need HTTP mocks include adding the dev-dependency as part of their scope.

## Prioritisation

Tasks are ordered roughly by value × ease. Lock-free shared-state code (P0) is the highest-leverage to lock down with tests because a regression there causes hard-to-diagnose audio glitches. Models and mock transport (P1) are cheap wins. The API client (P2) is the largest body of untested code but needs infrastructure first.

Tasks are independent unless an explicit "Depends on" is listed.

---

## Tasks

### T1 — Add unit tests for `RingBuffer` (lock-free SPSC queue)

**Priority:** P0
**File:** `src/audio/ring_buffer.rs` (211 LOC, 0 tests)

The ring buffer carries every audio sample between the AudioWorklet and the network code. Subtle ordering bugs here cause clicks, drops, or deadlocks under load.

**Scope:**
- Push/pop round-trip preserves sample values.
- Fill-to-capacity, then pop-all returns the right count and values.
- Wrap-around across the head/tail boundary works correctly.
- `available_to_read` / `available_to_write` invariants under interleaved push/pop.
- "Has data" flag pointer reflects empty/non-empty state.
- Concurrency: spawn two `std::thread`s (one producer, one consumer) and assert all values arrive in order and none are lost across many iterations.

**Acceptance:**
- New `#[cfg(test)] mod tests` block with ≥ 8 test fns.
- `npm run test` passes.

---

### T2 — Add unit tests for `AudioParams` atomic shared state

**Priority:** P0
**File:** `src/audio/params.rs` (285 LOC, 0 tests)

Holds volume / gain / peak levels shared between threads via atomics. Wrong memory ordering or sign handling silently corrupts metering.

**Scope:**
- Setters/getters round-trip for `MonitorVolume`, `AutoGainControl`, `EchoCancellation`, `NoiseSuppression`.
- dB-level conversion math (`getDbLevel`, `getPeakDbLevel`) — boundary values: silence, unity, clipping.
- Peak-hold decay (if applicable) over multiple ticks.
- Bool params encode/decode correctly via the underlying atomic int.
- Cross-thread visibility: writer thread + reader thread observe the latest value.

**Acceptance:**
- ≥ 6 test fns covering each public method.
- `npm run test` passes.

---

### T3 — Add unit tests for `MockTransport` sine-wave generator

**Priority:** P1
**File:** `src/audio/mock_transport.rs` (380 LOC, 0 tests)

Mock transport is the loopback we'll lean on for higher-level tests later, so we need it to be provably correct first.

**Scope:**
- Generated samples form a sine wave at the configured frequency (FFT or peak-counting verification).
- Sample-rate switching produces the expected period.
- Channel count is respected (mono vs. stereo).
- Packet boundaries match the JackTrip protocol header sizing.
- `send` / `receive` half is a pure loopback: bytes in → identical bytes out.

**Acceptance:**
- ≥ 5 test fns.
- `npm run test` passes.

---

### T4 — Add serde round-trip tests for `src/models/`

**Priority:** P1
**Files:** `src/models/*.rs` (14 files, ~2.3 kLOC, 0 tests)

These are typed data models for the JackTrip Virtual Studio REST API. The risk is that a field rename or enum-variant addition silently breaks deserialization of real server responses.

**Scope:**
- For each module (`common`, `device`, `studio`, `user`, `stream`, `recording`, `event`, `region`, `chat`, `billing`, `pagination`, `requests`, `responses`):
  - Build a representative instance, serialize to JSON, deserialize back, and assert equality.
  - Where the API uses `#[serde(rename_all = ...)]` or `#[serde_repr]`, assert the wire format matches a hand-written JSON sample (use a small inline fixture string per type).
  - Verify enum exhaustiveness: every variant round-trips.
- Add at least one "known good" fixture sourced from `docs/api/` per major resource.

**Acceptance:**
- ≥ 25 test fns total, distributed across the modules.
- `npm run test` passes.

**Out of scope:** API client behavior — that's T6.

---

### T5 — Expand `Regulator` test coverage for auto-tolerance and stats

**Priority:** P1
**File:** `src/audio/regulator.rs` (already 8 tests)

The existing tests cover the Burg PLC core. Recent commits (`5346442 Patches to try fixing regulator PLC predictions`, `7def5fc Fix potential regulator wrap-around race`) suggest the surrounding state machine is still settling.

**Scope:**
- Auto-tolerance adjustment over a sequence of late/on-time/early packets.
- Headroom statistics: assert reported values match the actual queue depth over time.
- Reset-on-reconnect: state is fully cleared (covers the race that `7def5fc` fixed).
- PLC kicks in after the configured number of consecutive misses, and stops once real packets resume.
- Underrun / overrun counters increment correctly.

**Acceptance:**
- ≥ 6 new test fns in addition to existing 8.
- `npm run test` passes.

---

### T6 — Add HTTP mocking infrastructure + tests for `src/api/`

**Priority:** P2
**Files:** `src/api/*.rs` (9 files, ~2.2 kLOC, 0 tests)

The API client is the largest untested body of code but needs an HTTP mock to test usefully. Bundle the infrastructure into this task.

**Scope:**
- Add `mockito` (or `wiremock`) as a dev-dependency.
- For each of `users`, `streams`, `studios`, `devices`, `recordings`, `events`, `billing`, `system`:
  - One happy-path test (mock the expected response, assert the parsed model matches).
  - One error-path test (HTTP 4xx/5xx → caller gets a typed error, not a panic).
  - Verify the request URL, method, and any query/body parameters are what the spec expects.
- Cross-reference `docs/api/` and the OpenAPI spec for fixture bodies.

**Acceptance:**
- `mockito`/`wiremock` added as dev-dependency.
- ≥ 16 test fns (2 per resource module × 8).
- `npm run test` passes.

**Note:** This is the largest task; split into two PRs if it gets unwieldy (infrastructure + first 3 resources, then the remaining 5).

---

### T7 — Establish `wasm-bindgen-test` harness for browser-only modules

**Priority:** P3
**Files:** test setup; later enables tests for `engine.rs`, `devices.rs`, `worklet.rs`, `webrtc.rs`, `audio_callback_loop.rs`, `webtransport_worker.rs`, `session.rs`

The web_sys-heavy modules can't be exercised by native `cargo test` at all. This task lays the groundwork so they *can* be tested in a future iteration.

**Scope:**
- Add `wasm-bindgen-test` dev-dependency.
- Add an `npm run test:wasm` script that drives `wasm-pack test --headless --chrome` (or `--firefox`) with the project's existing RUSTFLAGS.
- Write a single placeholder test against a trivial helper (e.g. `RingBuffer::new` invoked in a WASM context) to prove the harness works in CI and locally.
- Update `AGENTS.md` with how to run WASM tests.

**Acceptance:**
- New script + dev-dependency.
- One passing WASM test demonstrating the harness.
- Documentation updated.

**Out of scope:** Actually writing tests for the browser-coupled modules — those will be follow-up tasks once the harness exists.

---

### T8 — Add CI workflow to run tests on every PR

**Priority:** P3
**File:** `.github/workflows/test.yml` (new)

Tests only catch regressions if they run automatically.

**Scope:**
- GitHub Actions workflow that runs `npm run test` on push to `main` and on every PR.
- Uses the nightly toolchain specified in `rust-toolchain.toml`.
- Caches `target/` between runs.
- (Optional) Runs `npm run check` as a separate job.
- (After T7) Add a job that runs `npm run test:wasm`.

**Acceptance:**
- Workflow file added and green on the PR that introduces it.
- Failing tests block PR merge (require status check in branch protection — call out as follow-up if not done in this PR).

---

## Summary

8 self-contained tasks. T1–T6 give us solid coverage of all pure-logic Rust (~5 kLOC). T7 unlocks the browser-coupled modules. T8 makes the whole effort durable.
