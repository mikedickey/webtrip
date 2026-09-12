# AGENTS.md

For general project background see [README.md](README.md). For the threading model, audio data flow, browser API constraints, and transport architecture see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Answering Questions vs. Changing Code

**A question is not a work order.** "Would it make sense to…", "is this
correct?", "I'm not sure this reviewer is right", "what would it take to…" ask
for a finding. Deliver the finding and stop. Confirming that a problem is real
is not authorization to fix it — whether it is worth a change, and what shape
that change takes, is the user's call.

Investigation is unrestricted: read anything, run `npm run test` / `npm run
check`, and write throwaway code to settle a question empirically rather than
reasoning about what the code probably does. But an experiment that touched
tracked files is reverted before the turn ends; the answer reports what the
experiment showed instead of leaving the change behind.

**Git operations require an instruction in the user's most recent message.**
`git commit`, `git push`, `gh pr create`, and `gh pr edit` are outward-facing —
they publish to a branch that other people and CI can see. Authorization does
not carry forward. "Create and push a PR" authorizes that PR, not the next
commit to it three questions later. A new finding about an already-pushed
branch is reported, not pushed.

**When work is the obvious next step, offer it in one line and wait.** End with
"want me to fix it?", not with the fix already applied.

## Release Status Policy

This project has not been released yet. Do not preserve or design for backward compatibility; prefer the simplest clean changes and avoid paying compatibility costs before first release.

## Threading Model and Portability

**"WASM is single-threaded" is false — never reason from it.** WebAssembly has had a
threads proposal for years, and this project *builds with it enabled*: see
`wasm_rustflags` in `package.json` (`-Ctarget-feature=+atomics`, `--shared-memory`,
the `__wasm_init_tls`/`__tls_*` exports). What is single-threaded is an individual
JavaScript *agent* (a page or a worker) and its event loop — not the WASM module, and
not this code. At runtime WebTrip already has three agents executing the same WASM
instance over one shared linear memory: the main thread, the AudioWorklet render
thread, and the WebTransport worker. Real concurrent access is happening today.

**WASM is also not the only target.** WebTrip is a reusable Rust library that happens
to ship a browser artifact first. Future iterations compile natively for desktop
operating systems and get wrapped in CLIs and other host applications, where OS
threads — not workers — will drive the same audio path. `crate-type = ["cdylib"]` and
the browser-only `web-sys` gating describe today's *build*, not the design contract.
(`npm run test` already runs the unit tests natively, off wasm32.)

Therefore:

- **Core logic must carry its own thread-safety guarantees.** Audio buffers, the
  jitter buffer/regulator, the protocol codec, and shared parameter state are
  concurrent data structures. Their soundness must follow from atomics and the
  `Send`/`Sync` contract, not from "only one thread runs anyway."
- **`Rc`, `RefCell`, `Cell`, `thread_local!`, and other non-atomic interior
  mutability are justified by a confinement argument, never by a file's name.** The
  only such argument we currently have is: *this value is captured by a JS event
  handler and is only ever touched from the one agent that registered it* — which is
  why the existing uses are the `web_sys` callback wiring inside `session.rs`,
  `audio/webrtc.rs`, `audio/webtransport.rs`, and `audio/signaling.rs`. That is a
  description of where the argument holds today, **not an allowlist**: those same
  modules also contain portable logic — the session state machine and `SessionStats`,
  `SignalingMessage`/`HubConnectionState`, `tick_decision`,
  `parse_ice_candidate_json` — that a native host will reuse against OS threads and a
  non-`web_sys` transport. Keep that logic free of thread-affine types, and keep the
  `Rc`/`RefCell` at the callback boundary rather than letting it leak inward. If a
  value can be reached from more than one agent or thread, it uses atomics regardless
  of which file it lives in.
- **Cross-thread pointers go through `SharedPtr` (`src/audio/shared_ptr.rs`).** It
  carries the `Send`/`Sync` assertion once, in an audited place. Do not add a
  hand-rolled `unsafe impl Send`/`Sync` to a struct to silence the compiler — that
  asserts a guarantee the type may not have. If a type genuinely cannot be shared,
  fix the sharing, don't assert it away.
- **Do not weaken or remove synchronization on the grounds that the browser is
  single-threaded, and do not `cfg(target_arch = "wasm32")` around a correctness
  concern.** A data race that a browser happens to tolerate is still a data race on a
  native build.
- When a structure is *not* yet provably sound under concurrent access, say so
  explicitly in its docs and link the tracking work (see `SharedPtr::as_mut` for the
  house style) rather than leaving a silent assumption behind.

## No Code Duplication

**Do not duplicate code anywhere in this codebase — including test code.** Before writing any function, type, constant, or block of logic, check whether it already exists and reuse it. If the same code would appear in more than one place, extract it to a shared location first.

This applies equally to test helpers, serialization utilities, fixture builders, and any other repeated patterns. When you spot existing duplication, fix it as part of the task at hand rather than leaving it in place.

## Build Commands

**Always use npm scripts for building, never call wasm-pack directly** — the WASM build requires specific flags for threading support (atomics, shared memory, TLS exports).

- `npm run build` — Build the WASM module and the React website
- `npm run build:wasm` — Build only the Rust WASM module
- `npm run build:site` — Install deps and build the React website into website/dist
- `npm run clean` — Remove pkg/, target/, and website/dist/
- `npm run serve` — Serve the website + demo (HTTP :3000 or HTTPS :8443 with TLS)

**Never run `cargo check` or `cargo test` directly** — they will fail because `web_sys` types like `WebTransport` are gated behind `web_sys_unstable_apis`. Use the npm scripts which pass the required flags:

- `npm run check` — Run `cargo check` with correct RUSTFLAGS and WASM target
- `npm run test` — Run `cargo test` with correct RUSTFLAGS (runs native, not WASM)
- `npm run test:wasm` — Run `wasm-bindgen-test` for browser-only modules (see [docs/WASM_TESTING.md](docs/WASM_TESTING.md) for details)

**Note**: WASM tests require a properly configured browser environment. See the testing guide for requirements and troubleshooting.

## Testing Guidelines

Every test must be able to fail on a plausible bug in production logic. Before adding a test, ask: "what realistic mistake would this catch that no existing test catches?" If the answer is "someone edited a constant on purpose" or "serde/std/the browser is broken", don't write it. Coverage percentage is never a reason by itself to add a test — no zero-assertion tests, no "doesn't throw" tests.

**Do not write:**

- Tests asserting `Default`/constructor field values, derived trait output, or getter/setter round-trips with no logic in between.
- Tests that restate a constant, a one-line `matches!`, a format string, or an enum-to-string literal. (Exception: strings that form a cross-language or wire contract — e.g. transport state strings matched by JS, the 63-byte exit packet — should be pinned, with a comment saying which contract they pin.)
- Tests of another module's code from your module's test file. Packet serialization tests belong in `protocol.rs`, not in transport tests; a thin wrapper only needs a test for the logic it adds.
- Tests of mocks, fixtures, or test helpers themselves, and tests that simulate the algorithm under test inside the test body and then assert a bare load.
- Point tests on a code path that an existing matrix/boundary/sequence test already exercises. Prefer one thorough test (full permutation matrix, both sides of each boundary, a multi-step sequence) over several single-value tests; when adding a case, extend the existing test.
- Bare atomic store/load tests, and "cross-thread" tests whose synchronization (e.g. `thread::join`) makes them unable to fail.

**Do write:**

- Wire-format pins: explicit serde renames (`priceID`, `type`, `_meta`), `flatten` shapes, `serde_repr` integer values, absolute byte layouts. A round-trip alone is not enough — serialize and deserialize can share a symmetric bug; assert the actual bytes/JSON. Conversely, a model test whose only content is re-verifying `rename_all = "camelCase"` adds nothing after the first one.
- Distinct branches and edges: validation boundaries (both sides), error paths, wraparound (`u16` sequence numbers, ring indices), empty-input early returns, null-pointer guards.
- Regression tests for fixed bugs, with a comment referencing the bug/commit.
- Browser (wasm) tests that drive real handlers with synthetic events and assert observable contracts (callback payloads, teardown ordering, promise rejection) — not browser-provided initial states or `web_sys` echoes.
- Exactly one trivial canary test per `tests/*.rs` wasm binary — a binary whose tests all fail to register is silently skipped (see [docs/WASM_TESTING.md](docs/WASM_TESTING.md)).

When deleting or refactoring production code, delete its tests rather than porting low-value ones forward.

## Architecture

### Key Modules

- **`src/session.rs`** — `WebTripSession`: top-level orchestrator, connection state machine, owns shared buffers
- **`src/audio/regulator.rs`** — Jitter buffer with Burg PLC (packet loss concealment), ported from JackTrip C++
- **`src/audio/protocol.rs`** — JackTrip 16-byte wire protocol (serialization, sample rate encoding)
- **`src/audio/signaling.rs`** — Hub server WebSocket signaling for WebRTC/WebTransport
- **`src/audio/ring_buffer.rs`** — Lock-free SPSC queue with `Atomics.waitAsync` wake-up
- **`src/audio/params.rs`** — Atomic shared state for volume, gain, peaks across threads
- **`src/api/`** — HTTP API client (reqwest) for JackTrip Virtual Studio REST API
- **`src/models/`** — Typed data models with auto-generated TypeScript types via `tsify-next`
- **`src/lib.rs`** — WASM entry point, exports `init()` and public types to JavaScript
- **`website/`** — React SPA for webtrip.dev (Vite, own package.json); the demo lives at the `/demo` route (`website/src/pages/demo/`) and loads the wasm-pack output at runtime from `/pkg/` (unbundled — the WebTransport worker re-imports `{origin}/pkg/webtrip.js`, see `wasm_module_url`). `website/server.js` serves the built site plus repo-root `pkg/` with the COOP/COEP headers SharedArrayBuffer needs
- **`plans/`** — planning documents for work that is in progress, already completed, or superseded; see [plans/AGENTS.md](plans/AGENTS.md). Only read files in this directory when actively working on a specific plan, or when trying to understand the original implementation of a specific feature — do not read it otherwise

## Rust/WASM Specifics

- **Nightly toolchain** required (see `rust-toolchain.toml`) — needed for the unstable cargo flags `-Zbuild-std` (rebuild `std` with atomics/bulk-memory so it can link against shared memory) and `-Zno-profiler-runtime` (wasm coverage builds). The crate itself is stable Rust; no `#![feature(...)]` anywhere. See [Toolchain Requirements](README.md#toolchain-requirements) for what would have to change to drop nightly.
- **Target**: `wasm32-unknown-unknown`
- **Crate type**: `cdylib` — the current build produces a WASM binary rather than an
  rlib. This is a property of today's artifact, not a design constraint; write the
  code as a portable, thread-safe library (see [Threading Model and
  Portability](#threading-model-and-portability))
- JS interop via `wasm-bindgen`; browser APIs via `web-sys` (feature-gated, see Cargo.toml)
- The hub server may create its own WebRTC data channel — both client and server-created channels need message handlers

## API Integration

- JackTrip API base: `https://test.jacktrip.com/api`
- OpenAPI spec: `https://test.jacktrip.com/api/redirect/openapi`
- API docs in `docs/api/`; architecture docs in `docs/ARCHITECTURE.md`

## Environment-specific instructions

Only read the doc for your environment; skip the others.

| Environment | Instructions |
|-------------|--------------|
| Cursor Cloud | Read [docs/CURSOR_CLOUD.md](docs/CURSOR_CLOUD.md) before running tests or opening the app in Chrome |
