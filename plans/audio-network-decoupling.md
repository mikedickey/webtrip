# Decouple the audio interface from the network protocol

## Context

The request that started this was narrow: now that wire and device channel
counts are configured independently (#86), make WebTrip *see* every channel a
multichannel interface offers. Auditing that found the Web Audio plumbing
already correct — `channelCount: {ideal: N}` capture (`engine.rs:91-97`),
explicit/discrete destination (`engine.rs:309-316`), `outputChannelCount` on the
worklet node (`worklet.rs:126-135`). The plumbing was never the limit.

What the audit exposed is that **one constant, one frame count, and one
connection are each doing the work of two independent concepts**. The channel
ceiling is the most visible symptom, not the disease:

1. **`MAX_CHANNELS = 8` (`protocol.rs:60`) is simultaneously the wire limit and
   the device limit.** Its own doc concedes 8 is "the protocol's own, tighter
   limit" — a WebTrip choice, not a JackTrip constraint. An 18-channel
   interface is addressed at its first 8.
2. **Network frames-per-packet is welded to the audio callback size.**
   `session.rs:438` hardcodes `buffer_size = 128` with no setter;
   `processor.rs:501` asserts `fpp == frames`; `regulator.rs:1149-1153` rejects
   any packet not exactly `channels * self.fpp` long. The regulator *adopts* a
   peer's channel count but not its fpp. **A peer at 64/256/1024 fpp has every
   packet rejected today** — silence, `packets_rejected` climbing, no
   diagnosis.
3. **Packet size is bounded by nothing.** A packet is `16 + fpp*channels*2`
   bytes. WebTransport ships these as QUIC datagrams
   (`webtransport_worker.rs:661-691`) with no size check; QUIC forbids IP
   fragmentation (RFC 9000 §14) and a DATAGRAM frame must fit one packet
   (RFC 9221), so the practical cap is ~1200 bytes — already exceeded at 5
   channels × 128 fpp. `protocol.rs:315` accepts a legal **65,552-byte**
   packet.
4. **The session owns exactly one device and one connection, inseparably.**
5. **The device sample rate is assumed to equal the wire sample rate, and
   nothing checks it.** `AudioContext` is created with no `sampleRate`
   (`engine.rs:147-150`), so the browser runs it at the output device's native
   rate, while the regulator and wire are fixed at 48 kHz (`session.rs:438-439`,
   `sample_rate = 48000`). No code compares the two (`get_sample_rate`,
   `engine.rs:169`, has no non-test caller). Observed failure: microphone at
   48 kHz, speakers at 96 kHz → the context runs at 96 kHz, the worklet's
   128-frame quantum fires every 1.333 ms instead of 2.667 ms, and `pop` runs
   at twice the packet rate. Diagnosed from the regulator's diagnostics log
   (`pop_interval` mean 1.333 ms against `arrival_interval` mean 2.67 ms,
   `rejected=0`): every packet is played once and the other pop finds nothing
   new, so about half of all pops are concealed (`conceal_caught_up` ≈ 99% of
   concealed pops) and the stats panel shows ~102% PLC. The false glitches
   also inflate the auto-adaptive tolerance and headroom, adding latency. The
   same mismatch affects the send path: capture frames are produced at the
   device rate but packetised as if they were wire-rate frames. This is the
   same welding as items 1-2 — the device's clock and the wire's clock are
   treated as one — so it belongs in this work, not a separate fix.
6. **There is no platform boundary.** The same welding, between portable logic
   and its browser host. Portable types carry their JS exports on the type
   itself (`AudioParams` `params.rs:23`, `RingBuffer` `ring_buffer.rs:29,71`,
   `TransportType`/`TransportState` `transport.rs:58,95`, four types in
   `protocol.rs:614-661`, every `tsify` model). Portable logic logs straight to
   `web_sys::console` (`protocol.rs:272-317`, `transport.rs:43-52,147`). The
   regulator picks its clock by `cfg` — `js_sys::Date::now()` on wasm32
   (`regulator.rs:903-917`). The `Transport` trait takes a `js_sys::Function`
   (`transport.rs:215`) and returns `JsValue` errors. `wasm-bindgen`, `web-sys`
   and `js-sys` are unconditional dependencies of one `cdylib` crate, so
   "core stays portable" (AGENTS.md, *Threading Model and Portability*) is a
   convention nothing enforces — and so is this plan's own `network`/`audio`
   dependency rule.

Philosophy driving the scope: most audio-over-network tech is built around the
limits of decades-old infrastructure; a library should let someone with a good
connection and a real need push past them. Caps must be **diagnosable rails
with a stated reason**, never silent ceilings — the current habit of
incrementing a counter and going quiet is exactly the failure mode to design
out.

## Scope

**In:** the channel-ceiling split (device 32 / wire 8); the capture-width fix; a
mixing-engine boundary; full fpp decoupling in both directions; **sample-rate conversion between
the device rate and the wire rate, in both directions**; automatic fpp
fitting on **both** transports; device↔network separation; structural
groundwork for N connections; **a core/web crate split enforced by Cargo**
(`webtrip-core`, `webtrip-api`, `webtrip-web` in unprefixed `crates/core`,
`crates/api`, `crates/web`), plus a **`webtrip` facade crate** (`crates/webtrip`)
re-exporting them as `webtrip::core` / `webtrip::api` (and `webtrip::web` behind
an opt-in feature).

**Out:** `website/src/pages/demo/Demo.tsx` (stays at 1-2 channel capture
modes); changing PLC/Burg concealment behaviour; actually supporting N>1
connections; a native host crate (none exists to put in it — the core/web
boundary is what makes one possible); making `Transport::connect` futures
`Send` (the trait moves to core with portable error and callback types, but
its futures stay `!Send` because the browser drivers are main-thread or
worker-confined — a native host revisits this). Pre-release project — **no backward-compatibility shims, rename in
place** (AGENTS.md).

## Target architecture

### Module layout

Everything network-related currently lives under `src/audio/` — `transport.rs`,
`webrtc.rs`, `webtransport*.rs`, `signaling.rs`, `protocol.rs` — which is the
conflation this work exists to undo. `src/audio/mod.rs` already groups its
modules into commented blocks (audio · buffers · wire protocol · network ·
signaling); the directory simply hasn't caught up with how the code is already
thought about.

The code is cut along two independent axes — **subject** (audio / network /
the stream primitives that bridge them) and **platform** (portable core /
browser host) — and the platform axis is a Cargo workspace:

```
crates/webtrip/src/lib.rs         package `webtrip`: the facade; re-exports only
                                  (`webtrip::core`, `webtrip::api`, `webtrip::web` under `web`)
crates/core/src/                  package `webtrip-core`; portable; no web-sys / js-sys, ever
  limits.rs
  audio/    processor, params, dsp, mixer
  network/  protocol, transport (trait + deliver_received_packet + state mapping),
            signaling_message (SignalingMessage, HubConnectionState,
            parse_ice_candidate_json)
  stream/   shared_ptr, ring_buffer, regulator   (the bridge primitives)
  lifecycle.rs, session_stats.rs (SessionStats, build_session_stats, tick_decision)
crates/api/src/                   package `webtrip-api`; portable; today's api/ + models/
crates/web/src/                   package `webtrip-web`; the cdylib (built as pkg/webtrip.js);
                                  the only crate that touches the browser
  lib.rs, session.rs, dependent_module.rs, test_support.rs (browser-test harness)
  audio/    devices, engine, worklet(+js), audio_callback_loop
  network/  webrtc, webtransport, webtransport_worker(+js), signaling (WebSocket wiring)
```

**The deliverable is the dependency rules, not the folders:**

- **Subject:** within each crate, `network` never imports `audio` and `audio`
  never imports `network`; both may use `stream`, `limits` and `dsp`.
  `session.rs` composes them.
- **Platform:** `webtrip-core` and `webtrip-api` never depend on `web-sys` or
  `js-sys`; only `webtrip-web` does. This one is **compiler-enforced**: CI runs
  `cargo check -p webtrip-core -p webtrip-api --no-default-features` natively.

`regulator.rs` has no `wasm_bindgen` references but is *not* yet portable — it
reads `js_sys::Date::now()` (`regulator.rs:903-917`). Phase 0b injects the
clock.

Decisions behind the platform split, settled with the user:

- **Three crates, not two.** `api/` + `models/` get their own `webtrip-api` so
  the audio engine never pulls in reqwest/serde, and a native CLI can use one
  without the other.
- **JS exports are feature-gated attributes.** Rust's orphan rule stops
  `webtrip-web` from adding `#[wasm_bindgen]` to a type it does not own. Core
  and api therefore get an optional `wasm` feature that enables only
  `wasm-bindgen`/`tsify-next`, used as `#[cfg_attr(feature = "wasm",
  wasm_bindgen)]` / `cfg_attr(feature = "wasm", tsify(...))`; `webtrip-web`
  turns it on. Wrapper newtypes in web were rejected: every exported type and
  getter would be written twice.
- **`log` facade, injected time.** Core logs through `log::{debug,warn,error}!`;
  `webtrip-web` installs a console backend once in `init()`. Core never reads a
  clock: the regulator takes `now_ms` from its caller (`push_at`/`pop_at`).
  Project-defined `Logger`/`Clock` traits were rejected as reinventing `log`
  and threading a handle through every type.
- **No native crate yet.** It would be empty.
- **Unprefixed directories, one `webtrip` facade.** Crate directories drop the
  project prefix (`crates/core` holds package `webtrip-core`). `crates/webtrip`
  (package `webtrip`) re-exports the others so a consumer takes one dependency
  and writes `webtrip::core::…` / `webtrip::api::…`. `webtrip::web` exists only
  behind an off-by-default `web` feature, so a default `webtrip` dependency is
  as portable as core. The facade also forwards `wasm`. It takes the package
  name `webtrip`, so the browser package becomes `webtrip-web`, and its
  wasm-pack artifact keeps the name `webtrip` through `--out-name webtrip`.
- **The JS contract is one module.** The website and integration tests import
  only `pkg/webtrip` (`website/src/lib/webtrip.ts`, `tests/integration/run.mjs`).
  JS has no notion of Rust crates: core/api types reach JS because
  `webtrip-web` links them into its one cdylib with their `wasm` feature on.
  The crate layout is invisible from TypeScript and must stay so.

### New modules

Three new portable modules, all in `webtrip-core`, layered acyclically
`dsp ← mixer ← processor` (`limits.rs` sits at core's root, above both
`audio` and `network`, since both use it):

- **`limits.rs`** — `MAX_WIRE_CHANNELS = 8` (renamed in place from
  `protocol.rs:60`, carrying the rail's reason) and `MAX_DEVICE_CHANNELS = 32`.
- **`audio/dsp.rs`** — the pure DSP math moved verbatim from
  `processor.rs:15-242` (`map_to_output`, `conform_interleaved_channels`,
  `interleave_planar`, `downmix_to_mono`, `max_channel_rms`, `apply_gain`) with
  its tests. Makes the portability claim structural: `mixer.rs` then has no path
  to `params.rs`'s `#[wasm_bindgen]` types.
- **`audio/mixer.rs`** — `trait MixEngine: Send` + `SimpleMixer`.

**`MixEngine`** is the device↔network bridge, expressed in N device streams / M
network streams while the only implementation handles 1+1:

```rust
pub trait MixEngine: Send {
    /// Off the RT thread. Sizes every scratch buffer. The only place it allocates.
    fn prepare(&mut self, layout: &MixLayout) -> Result<(), MixError>;
    /// On the RT thread. No allocation, no locking, no unbounded work.
    fn render(&mut self, controls: &MixControls, io: &mut MixIo<'_,'_>) -> MixMeters;
}
```
with `MixIo { device_in: &[DeviceIn], device_out: &mut [DeviceOut], net_in:
&[NetIn], net_out: &mut [NetOut] }`.

Three properties that make this work:

- **No I/O in the engine.** No `Regulator`, `RingBuffer`, `SharedPtr` or
  `AudioParams` in `mixer.rs`. `AudioProcessor` does all pointer work, decodes
  atomics into `MixControls`, encodes `MixMeters` back. That is what keeps it
  portable and unit-testable with plain slices.
- **No per-callback allocation.** Descriptor arrays are built on the *stack* in
  `process()`; for N>1 they become fixed-capacity inline arrays under a
  `MAX_STREAMS` rail. The trait only ever sees slices.
- **Absent network is an empty slice.** `net_in: &[]` means silence,
  `net_out: &mut []` means nothing is sent — mapping exactly onto today's
  `remote_channels == 0` branch (`processor.rs:385-386`) and null-ring early
  return (`processor.rs:457`). Audio-without-network falls out of the boundary
  for free rather than needing a flag.

**Dispatch: `Box<dyn MixEngine>`.** There is already one indirect call per
callback at the worklet ABI (`ProcessorCallback`, `worklet.rs:17`, invoked at
`worklet.rs:65`); a second vtable dispatch is a few nanoseconds against a
2.67 ms budget. Static dispatch would infect
`build_and_connect_worklet_node` (`engine.rs:332`) with a type parameter at the
`#[wasm_bindgen]` boundary for no measurable gain; an enum would need editing
for every future engine — the "reshaped boundary" this work exists to avoid.

## Phases

Each phase compiles alone and keeps all three npm checks green.

### Phase 0 — Module and crate reorganization
*First, so every later phase puts its new files in their final crate. Split
into four steps so the pure renames stay reviewable apart from the real
refactor.*

#### Phase 0a — Module moves inside the single crate (pure renames)
*No logic change, no test change beyond paths.*

`git mv` so that `src/` already holds the three final crate trees, with
`src/` itself as the web crate's root (a single crate's `lib.rs` cannot move
into a subdirectory):

```
src/                      → becomes crates/webtrip-web/src/ in 0c
  lib.rs, session.rs, dependent_module.rs, test_support.rs
  audio/    devices, engine, worklet(+js), audio_callback_loop
  network/  webrtc, webtransport, webtransport_worker(+js), signaling
  webtrip_core/           → becomes crates/webtrip-core/src/ (mod.rs → lib.rs)
    mod.rs, lifecycle.rs
    audio/    processor, params
    network/  protocol, transport
    stream/   shared_ptr, ring_buffer, regulator
  webtrip_api/            → becomes crates/webtrip-api/src/ (mod.rs → lib.rs)
    mod.rs
    api/      (today's src/api/, unchanged inside)
    models/   (today's src/models/, unchanged inside)
```

The module names are chosen so the future crate names already appear in every
path: core code is reached as `crate::webtrip_core::…` and api code as
`crate::webtrip_api::…`, and 0c only has to drop the `crate::` prefix. Web
paths (`crate::audio::engine`, `crate::session`) never change. (Do not name a
module `core`: at the crate root it collides with the built-in `core` crate.)

A file whose contents straddle the boundary (`session.rs`, `webrtc.rs`,
`signaling.rs`, `transport.rs`) moves whole to the side most of it belongs on;
0b splits it. Update `src/lib.rs` module declarations, split
`src/audio/mod.rs`'s re-export blocks (`:64-84`) across the new `mod.rs`
files, and fix imports crate-wide. The browser helpers `src/audio/mod.rs`
also defines (`named_error`, `cancelled_error`, `make_promise`, `:37-62`)
stay on the web side. `dependent_module!`
(`src/dependent_module.rs`, used by `worklet.rs:98` to inline `worklet.js`)
must keep resolving — verify with `npm run test:wasm`, not just `check`.

State both dependency rules in each new `mod.rs` header so the next person
inherits the intent rather than the folder alone.

Reviewable as a diff of pure renames: no line of logic should change in this
step.

#### Phase 0b — Separate core from the browser (real refactor, still one crate)

- **Logging:** route every log call under `src/webtrip_core/` and `src/webtrip_api/`
  through `log`. Add a console `log` backend in web (a maintained crate, or
  ~20 lines in web if none fits) and install it in `init()`. Drop the
  `cfg(target_arch = "wasm32")` around core logs (`transport.rs:21,43`,
  `protocol.rs:272-317`): the throttling logic then runs, and is testable,
  natively.
- **Clock:** the clock-taking variants already exist privately
  (`Regulator::push_internal`/`pop_internal(.., now_ms)`, `regulator.rs:1036`,
  `:1049`); expose them as `push_at`/`pop_at` in place of `push`/`pop`. Web
  callers supply the time from the same source `Regulator::now_ms`
  uses today (`js_sys::Date::now()`, as `crate::now_ms()`). Delete
  `Regulator::now_ms` and its `cfg` branches. `deliver_received_packet`,
  `AudioProcessor::process` (which pops the regulator inside the worklet
  callback) and the WebTransport worker's `handle_datagram` /
  `deliver_datagram_if_running` each gain a `now_ms` parameter; the
  `ProcessorCallback` ABI is unchanged, the engine's closure supplies the time.
- **Bindings:** move every `#[wasm_bindgen]`/`tsify` on core and api types
  behind `cfg_attr(feature = "wasm", ...)`, with the JS names unchanged. The
  generated `pkg/webtrip.d.ts` must be unchanged before and after: every public
  declaration identical (the internal `InitOutput` export list reorders between
  builds, so compare the sorted file or the part above `InitOutput`, not raw
  bytes). In the single crate `wasm` is a placeholder feature (`default =
  ["wasm"]`, `wasm = []`); 0c turns it into an optional-dependency feature on
  core and api. Each api `#[wasm_bindgen] impl` that holds only JS wrappers
  (the `_js` methods) is gated whole with `#[cfg(feature = "wasm")]`; `ApiClient`
  and the `api_module_struct!` macro use `cfg_attr` because their methods are
  also called natively. Keep `WebRtcTransport`'s inherent JS-facing
  `set_on_state_change(js_sys::Function)` (it wraps the function into the
  portable callback) — removing it changes the `.d.ts`.
- **Transport trait:** `set_on_state_change` takes
  `Box<dyn Fn(TransportState)>`; `connect` returns a portable
  `TransportError`. Web converts to `JsValue` / `named_error` at its
  `#[wasm_bindgen]` boundary (a `TransportError` carries only a message; web
  turns it into a JS `Error`, where some errors were plain strings before).
  `notify_transport_state` moves to web (`network/mod.rs`) and operates on a
  `SharedStateCallback = Rc<dyn Fn(TransportState)>`: the web transports convert
  the `Box` to an `Rc` once and clone it into their JS event closures, so `Rc`
  stays at the callback boundary and out of core.
- **Extract the portable logic AGENTS.md already names** out of the browser
  modules into core: `SessionStats`/`build_session_stats`
  (`session.rs:72,153`), `tick_decision` and `parse_ice_candidate_json`
  (`webrtc.rs:93,59`), `SignalingMessage`/`HubConnectionState`
  (`signaling.rs`), and `transport_state_is_failure` (`session.rs`; AGENTS.md
  names the transport-failure mapping; it lands in core's `transport.rs`).
  `SessionStats`/`build_session_stats`/`tick_decision` go to
  `session_stats.rs`; the signaling types and `parse_ice_candidate_json` to
  `network/signaling_message.rs`. Their tests move with them, never duplicated.
  `signaling_state_is_closed` / `signaling_close_error_message` (pure, in
  `webrtc.rs`) and `is_valid_channel_count` (`session.rs`) stay in web for now.
- **Acceptance:** `grep -rE 'web_sys::|js_sys::' src/webtrip_core src/webtrip_api`
  is empty, and nothing under those two trees imports a web module
  (`crate::session`, `crate::audio`, `crate::network`, `crate::test_support`). This is what makes 0c mechanical.
  Because one crate cannot prove the platform rule, also build the two trees as
  standalone native crates (no wasm-bindgen/web-sys/js-sys) in a scratch
  workspace and run their tests; 0c's `check:core` then enforces it for good.

#### Phase 0c — Lift into a Cargo workspace (mechanical)

Root `Cargo.toml` becomes `[workspace]`. Move `src/webtrip_core/` to
`crates/webtrip-core/src/` and `src/webtrip_api/` to `crates/webtrip-api/src/`
(each `mod.rs` becomes `lib.rs`); everything left in `src/` moves to
`crates/webtrip-web/src/`. Rewrite `crate::webtrip_core::` →
`webtrip_core::` and `crate::webtrip_api::` → `webtrip_api::` (inside core and
api, their own paths become `crate::…`); no other import changes. Turn
`pub(crate)` into `pub` only where a web→core edge needs it, and mark those
items `#[doc(hidden)]` when they are not public API. Update the npm scripts:

- `build:wasm`, `build:wasm:coverage`, `test:wasm` target `crates/webtrip-web`
  with `--out-dir` at the repo-root `pkg/`. The site and the WebTransport
  worker load `/pkg/webtrip.js`, so the cdylib keeps the name `webtrip` and
  the output path does not move.
- `test` and `coverage` gain `--workspace`.
- `test:miri` becomes `-p webtrip-core stream::regulator`.
- New `check:core`: `cargo check -p webtrip-core -p webtrip-api
  --no-default-features` (native). Run it in CI. This is the platform
  rule's enforcement.

Update the AGENTS.md *Build Commands* and *Key Modules* sections, and the
`README.md` paths.

(0d renames these `crates/webtrip-*` directories; the paths above describe
0c as executed.)

#### Phase 0d — Unprefixed crate directories + `webtrip` facade
*Mechanical. No logic change, no `use` path change in core or api.*

- **Moves (`git mv`):** `crates/webtrip-core` → `crates/core`,
  `crates/webtrip-api` → `crates/api`, `crates/webtrip-web` → `crates/web`.
  Package names `webtrip-core` / `webtrip-api` are unchanged, so no import
  changes. Update the path dependencies (`../core`, `../api`) and the root
  `members`.
- **Rename the browser package** `webtrip` → `webtrip-web` (its library name
  becomes `webtrip_web`). Make its `crate-type = ["cdylib", "rlib"]` so the
  facade can depend on it. Pass `--out-name webtrip` in `build:wasm` and
  `build:wasm:coverage`, so the output stays `pkg/webtrip.js` /
  `pkg/webtrip_bg.wasm`. Those names are load-bearing:
  `wasm_module_url` (`webtransport.rs`), `website/src/lib/webtrip.ts`,
  `tests/integration/run.mjs`, `containers/webtrip/Containerfile`,
  `scripts/integration-coverage-report.sh`. Retarget `-p webtrip` →
  `-p webtrip-web` in `coverage:wasm` and in the builder Containerfile's
  `cargo clean` lines.
- **New `crates/webtrip`** (package `webtrip`, rlib, workspace-inherited
  metadata; `publish` stays false, and publishing is a separate decision):
  ```toml
  [features]
  wasm = ["webtrip-core/wasm", "webtrip-api/wasm"]
  web  = ["dep:webtrip-web"]

  [dependencies]
  webtrip-core = { path = "../core" }
  webtrip-api  = { path = "../api" }
  webtrip-web  = { path = "../web", optional = true }
  ```
  ```rust
  pub use webtrip_core as core;
  pub use webtrip_api as api;
  #[cfg(feature = "web")]
  pub use webtrip_web as web;
  ```
  Re-exports only: no logic, so no tests. (Inside the facade, write `::core`
  wherever the built-in crate is meant.) `webtrip-web` keeps depending on
  `webtrip-core` / `webtrip-api` directly. Depending on the facade instead
  would form a `webtrip-web → webtrip → webtrip-web` cycle once `web` is on.
- **Platform rule covers the facade:** `check:core` becomes
  `cargo check -p webtrip-core -p webtrip-api -p webtrip --no-default-features`.
- **Path references** (`crates/webtrip-*` → `crates/*`): root `Cargo.toml`,
  `package.json` (`build:wasm*`, `test:wasm`), `codecov.yml`, the builder
  `containers/builder/Containerfile` (manifest COPYs and the prewarm stub loop,
  which must also stub `crates/webtrip`, since every member's manifest must
  exist), the `ci.yml` comment, `tests/integration/run.mjs` comments,
  AGENTS.md (*Threading Model*'s `SharedPtr` path, *Workspace layout* now four
  crates, *Key Modules*), `README.md`, `docs/ARCHITECTURE.md`,
  `docs/CHANNEL_MODEL.md`, `docs/WASM_TESTING.md` (including
  `crates/web/webdriver.json`). Doc comments that name the *package*
  `webtrip-web` stay correct.
- **Acceptance:** `pkg/webtrip.d.ts` is unchanged against the 0c build (same
  comparison rule as 0b). `pkg/webtrip.js` and `pkg/webtrip_bg.wasm` exist
  under those names. No TypeScript file changes. `git grep 'crates/webtrip-'`
  is empty outside `plans/`. `cargo check -p webtrip --features web` builds
  under the wasm flags.

### Phase 1 — Channel-ceiling split + capture-width fix
*Delivers: an 18-channel interface addressed at 32; the mono-narrowing capture
bug fixed. Independent of every later phase.*

- New `limits.rs` at `webtrip-core`'s root with both ceilings. Retarget the
  **device-side call sites** to `MAX_DEVICE_CHANNELS`: `clamp_channel_count`
  (`devices.rs:122`, which already serves capture width `engine.rs:261`, the
  destination `engine.rs:312`, and both device probes `devices.rs:129,201`),
  the `channelCount.ideal` constraint (`engine.rs:95`), `worklet.rs:62-63`,
  `worklet.rs:87`, `worklet.js:4,46`. `params.rs:65` clamps `send_channels` and
  correctly stays on the wire ceiling.
- **`AudioProcessor`'s device-width scratch buffers must grow with the
  ceiling.** `gained_buffer`, `captured_interleaved`, `remote_mapped` and
  `monitor_mapped` are preallocated at `128 * MAX_CHANNELS`
  (`processor.rs:294-299`, `:315-320`) and are indexed by device channel
  counts, so raising the device ceiling without raising them makes a wide
  interface `resize` on the render thread (`processor.rs:345-373`). Size them
  from `MAX_DEVICE_CHANNELS`; the wire-width buffers (`wire_conformed`,
  `remote_interleaved`) stay on `MAX_WIRE_CHANNELS`.
- `regulator.rs:80` stays a **deliberate mirror** (that module has no `crate::`
  imports so a native host can reuse it) but becomes `pub(crate)` so a native
  test can pin `regulator::MAX_CHANNELS == MAX_WIRE_CHANNELS`.
- **Delete both `worklet.js:3-4` constants** in favour of new
  `ProcessorHandle::channel_capacity()` (derived from
  `input_scratch.len() / RENDER_QUANTUM_FRAMES`) and `frames_per_quantum()`,
  read in `ensureViews()`. This eliminates the JS duplicate outright rather than
  centralising it — a stale `worklet.js` is a `TypeError` inside the render
  callback that kills the AudioWorkletProcessor and stops all audio.
- Capture fix: make `capability_max_channels` (`devices.rs:145`) `pub(crate)`
  and extract one `track_channels(track, resolve)` that reads a track's two raw
  values, so `engine.rs` capture and `get_input_device_channels` cannot drift
  in *how they query*. They deliberately differ in *precedence*: the probe asks
  what the device offers (`resolve_input_channels`, capability first); capture
  opens the worklet at what the track delivers (`resolve_granted_channels`,
  granted settings first, capability only when settings are unreported). The
  node is Explicit/Discrete, so channels the track doesn't deliver arrive as
  silent planes, and every later stage that narrows (`downmix_to_mono`, the
  mixer's conform to wire width) averages them in — halving the send/monitor
  level for Chrome's echo-cancellation mono force. No later phase corrects
  this; the capture width must be right at its source.
- `setInputChannels`/`setOutputChannels` validate against `MAX_DEVICE_CHANNELS`
  (send/receive stay on `MAX_WIRE_CHANNELS`); otherwise a wide interface
  cannot be selected. `is_valid_channel_count` takes the ceiling.

### Phase 2 — `dsp.rs` extraction + `MixEngine`/`SimpleMixer`
*Pure refactor. Groundwork for everything else. `dsp.rs` and `mixer.rs` are
created in `crates/core/src/audio/`.*

`SimpleMixer::render` reproduces `processor.rs:330-423` exactly, composing the
`dsp.rs` functions and never reimplementing them: gain → `max_channel_rms` →
`interleave_planar` **once** → conform to wire width → `map_to_output` for
remote → `map_to_output` for monitor → sum/scale/clamp.

Acceptance: `test_process_stereo_capture_feeds_ring_and_monitor`
(`processor.rs:1008`) and `test_process_conforms_mono_capture_to_stereo_wire_width`
(`processor.rs:1074`) pass **untouched**.

Known cost to state in the commit: the matched-width send case gains one
`frames × channels` memcpy (today it writes straight to the ring,
`processor.rs:467`). Mitigate by passing the preallocated wire staging buffer
*as* `net_out[0].interleaved`, bounding it to one ≤4 KB copy per callback.

`SimpleMixer::prepare` sizes its device-width scratch from a `MixLayout`
(`max_frames`, `max_device_channels`); `AudioProcessor` still passes the 128-frame
render quantum. Raising that basis so decoupled fpp can never trigger a
`resize` on the audio thread (the `remote_interleaved` / `wire_staging`
buffers and `receive_from_network`'s resize) belongs to Phase 3, where the
regulator's frame FIFO determines the real maxima.

### Phase 3 — Frame-granular `Regulator::pop_frames_at`
*Delivers: callback size and packet size fully independent, both directions.*

The regulator owns a frame FIFO (in its consumer-owned state) between its
packet slots and the audio callback. `pop_frames_at(output, frames, now_ms) ->
channels` serves exactly `frames` frames per call, pulling whole packets into
the FIFO as needed; every audio callback makes exactly one call, so a packet
larger than the callback is simply cached across calls. `pop_at(output,
now_ms)` remains as the packet-granular call for a host whose callback is one
packet long; a given regulator uses one or the other.

**Packet pulls are internal and exactly sized.** `pull_packet` (the old pop
body minus the stats tick) always writes into the FIFO's own `fpp * channels`
target, so `interleave_output` (via `interleave_channel`, whose `zip` truncates
silently) can neither drop a tail on a short output nor leave stale audio on
a long one: tail-drop and stale-tail are **unreachable by construction**, and
`interleave_output` carries a `debug_assert_eq!(output.len(), fpp * channels)`.
`last_seq_out` still advances once per packet; the underrun/overrun counters
and the tolerance controller remain packet-denominated.

**The pull clock is the audio callback.** `tick_pull` runs once per
`pop_frames_at` call, recording the true interval since the previous call —
including calls that pull no packet (callback smaller than the packet), and
however many packets a call does pull. Every packet in one call is pulled as
of the same `now_ms`; `TimingStats::tick` discards the resulting zero-length
intervals. `pull_stats` therefore carries the device callback's real jitter
into `pull_tol = calc_auto()`, the floor under the auto tolerance
(`new_tolerance = max(push_tol + headroom, pull_tol)`), instead of the packet
clock. Consequence: tolerance settles near the slower of the callback and
packet periods, and a *fixed* tolerance below the callback span makes packets
that queued within one callback look stale to `find_best_packet` — a tolerance
that small cannot be honoured at that callback size.

**Startup and width.** `pop_frames_at` takes one Acquire per call; the stream's
width is fixed for every packet pulled in it. Before the stream starts it
fills the FIFO with the startup silence `pull_packet` would give (at the
regulator's initial width). When the adopted width differs from the FIFO's, the
FIFO (at the old stride) is flushed. `reset()` flushes it too, so audio pulled
for one stream never plays in the next.

**Sizing.** `Regulator::with_callback_capacity(frames)` preallocates the FIFO
at `(frames + fpp) * MAX_CHANNELS` samples (~200 KB) and the pull target at
`fpp * MAX_CHANNELS`, once, before the regulator is shared; `pop_frames_at`
never allocates, and a call asking for more than the capacity is a
`debug_assert` and returns `0` (silence). The session builds the regulator with
`RENDER_QUANTUM_FRAMES`, which lives in core's `limits.rs` (`worklet.rs`
re-exports it). `AudioProcessor::with_network` takes `max_callback_frames` and
sizes `MixLayout` and `remote_interleaved` / `wire_staging` at
`max_callback_frames * MAX_WIRE_CHANNELS`. A callback never exceeds the
maximum; if one ever did, `ensure_len` degrades to a single allocation rather
than a panic on the render thread. `fpp` is fixed at
construction until Phase 5 (see there).

**Latency.** The frames sitting in the FIFO are published with the regulator's
other snapshot atomics (`pub_queued_frames`) and counted by `latency_ms()`
alongside the packets queued in the slots, which would otherwise under-report
by up to `fpp-1` frames. `Regulator::frames_to_ms` converts frames at the wire
rate.

Delete `processor.rs:501`'s `debug_assert_eq!(fpp, frames)`.

### Phase 4 — Sample-rate conversion (device rate ↔ wire rate)
*Delivers: a 48 kHz link works regardless of the device's native rate (e.g.
48 kHz mic + 96 kHz speakers) with no spurious PLC. Builds on Phase 3:
`pop_frames_at` and the processor's frames-needed arithmetic both assume device
frames and wire frames are the same unit, and that assumption is what this
phase removes.*

Requirement: the device rate and the wire rate are independent values, and
every place that converts between "frames the device needs" and "packets the
wire supplies" accounts for the ratio.

- **Make the rates explicit.** `limits::WIRE_SAMPLE_RATE = 48_000` is the one
  named wire rate (`AudioFormat` and the packet header's rate code derive from it, and the
  session builds its regulator with it). The device rate is read back from
  `AudioContext.sampleRate` when the worklet node is built and passed, with the
  wire rate, to `AudioProcessor::with_network` and on into `MixLayout`
  (`device_rate`, `wire_rate`). The regulator's `sample_rate` stays the **wire**
  rate and never sees the device rate; `with_network` asserts this
  (`Regulator::sample_rate()`).
- **Where the conversion lives: both (decided).** The `AudioContext` is created
  with `sampleRate: 48000` (retried without it if the browser throws), and our
  own conversion engages whenever the actual rate differs. Where the browser
  honours the request nothing runs. `dsp::StreamResampler` wraps rubato's
  asynchronous sinc `Async` resampler (`default-features = false`: no FFT; sinc
  length 128, linear interpolation over a 256× table, ratio fixed). It
  preallocates everything, so `process` never allocates on the render thread,
  and uses rubato's active-channel mask so a narrow stream pays only for its
  channels. It is built for `MAX_WIRE_CHANNELS`, so adoption of the peer's
  channel count needs no rebuild.
- **The mixer owns both conversions.** `SimpleMixer::prepare` builds a send
  resampler (fixed input: one render quantum in, a varying number of wire
  frames out) and a receive resampler (fixed output: one render quantum out, a
  varying number of wire frames in) only when the rates differ. Equal rates
  build nothing and the mixer behaves as before.
  - **Receive:** `net_in` carries wire-rate frames. A new trait method,
    `MixEngine::net_in_frames_needed(device_frames)`, returns exactly how many
    the resampler consumes this call (the default is `device_frames`). The
    processor pops that many from `pop_frames_at`, so the pull clock still ticks
    once per device callback. The count is exact, not the
    `ceil(frames * wire / device)` approximation, because a fractional ratio
    (44.1 kHz) varies call to call.
  - **Send:** the capture is conformed to wire width at the device rate, then
    resampled; `NetOut.frames` reports the wire frames written, and the
    processor writes exactly that many to the ring, so packets carry wire-rate
    audio and the header's rate field is truthful.
  - A callback whose size differs from the prepared chunk cannot be converted
    (the worklet's quantum is fixed at 128): `debug_assert`, and that call sends
    nothing / plays silence.
- **Sizing.** `limits::wire_frames_per_callback(frames, device, wire)` (exact
  ratio rounded up, plus one frame of slack, or `frames` when the rates match)
  sizes the processor's `remote_interleaved` and `wire_staging`.
  `MIN_DEVICE_SAMPLE_RATE = 8_000` and `MAX_DEVICE_SAMPLE_RATE = 768_000` bound
  the supported device rates. The regulator is built before the context exists,
  so the session sizes its callback capacity for the slowest device:
  `limits::MAX_WIRE_FRAMES_PER_CALLBACK` (769).
- **Fail loudly.** `MixLayout` is refused with `MixError::UnsupportedRates`
  (naming both rates) when either rate is zero, the device rate is outside the
  supported range, or the resampler cannot be built. `AudioProcessor::with_network`
  returns it, `build_and_connect_worklet_node` turns it into a JS error, and
  `connect` fails with it. Conversion is always available, so a mismatch itself
  is never an error, only a rate that cannot be converted.
- **Clock drift needs no special handling.** The device clock and the peer's
  clock are separate oscillators, so even at identical nominal rates they
  drift. A fixed-ratio converter is sufficient: the regulator's adaptive
  tolerance absorbs the slow drift, as it does today.
- **Cost.** Two sinc resamplers at ~130 KB of tables each, allocated on the main
  thread at worklet build, only when the rates differ. Measured delay
  per direction while converting is 0.7–1.4 ms (rubato's `output_delay`: 32–128
  output frames across 48↔96 kHz and 44.1→48 kHz).

### Phase 5 — fpp fitting + peer-fpp adoption
*Delivers: 5-channel links stop silently failing; a 256-fpp peer works instead
of having every packet rejected.*

Pure math in `protocol.rs`, reusing `audio_data_size` (`protocol.rs:73`) so
packet geometry has one definition:
`fit_fpp(max_bytes, channels, bit_depth, preferred) -> FppFit { fpp,
packet_bytes, limited_by }`. Largest power-of-two fpp ≤ preferred that fits
(power-of-two because `packets_in_past = HIST*(HIST_FPP/fpp)` truncates for
non-divisors, `regulator.rs:940-946`), clamped to `[MIN_FPP, MAX_FPP]`.

**One fpp range for both directions, in `limits.rs`: `MIN_FPP = 16`,
`MAX_FPP = 1024`.** 1024 is JackTrip's largest common buffer size; 16 is its
low-latency setting. Header overhead at small fpp is a cost, not a correctness
limit, so it sets no floor. In practice `fit_fpp` cannot reach the floor at the
8-channel wire ceiling (1200 B still fits 64 fpp). The deserializer's
`buffer_size` bound (`protocol.rs:309`, an unexplained `1..=4096` today) becomes
`[MIN_FPP, MAX_FPP]` and keeps its `InvalidBufferSize` error, so the send-side
fit, the wire check and the regulator share one definition.
`FppLimit::Floor` surfaces as an **error**, not a silently oversized packet.

`session.rs:412`'s single `buffer_size` splits into `send_fpp` and
`receive_fpp` — genuinely different things (`:449-454` configures receive via
`Regulator::with_params`, `:1061` send via `audio_buffer_config`). Because the regulator adopts the
peer's fpp (below), `receive_fpp` is only the regulator's pre-adoption geometry:
its initial tolerance and its startup silence. **No serializer change is needed**: `serialize_samples_into` derives
`header.buffer_size = samples.len()/send_channels` (`protocol.rs:401`), so send
fpp is implicit in how many samples a transport reads per packet, and both
transports already use `AudioBufferConfig.buffer_size` as their chunk size
(`webrtc.rs:388`, `webtransport_worker.rs:608`).

Add `fn max_packet_bytes(&self) -> Option<usize> { None }` to the `Transport`
trait (`transport.rs`, near `connect` at `:230`).

- **WebTransport — discovered.** In `worker_connect`
  (`webtransport_worker.rs:491`) right after `ready()` resolves (`:505`), read
  `transport.datagrams().max_datagram_size()` (available in the pinned web-sys,
  **no Cargo change**) and `fit_fpp`. A narrow `WorkerState::set_send_fpp`
  resizes only the scratch buffers `configure()` sizes (`:393-397`),
  deliberately *not* re-running `configure()`. Post `{type:"fpp", …}`
  **before** `post_message_to_main("connected")` (`:1009`); since
  `connect_worker` awaits that (`webtransport.rs:533`), the value is
  guaranteed present when `bring_up` publishes the transport
  (`session.rs:977-982`). `webtransport_worker.js` needs no change.
- **WebRTC — conservative rail now, probe as diagnostic.** The fitting input is
  **not** `maxMessageSize`: browsers report ≥64 KiB, which would accept packets
  that fragment across many SCTP chunks, and the channel is `ordered:false,
  maxRetransmits:0` (`webrtc.rs:236-244`, `:1054-1057`), so fragmentation
  multiplies loss. The fitting limit is therefore the same fragmentation-free
  MTU floor as QUIC, known synchronously. Separately, inside the existing
  `on_open` closure (`webrtc.rs:1074-1081`) — which fires exactly when the SCTP
  association is up, so `pc.sctp` is non-null — do the `js_sys::Reflect` probe
  (pattern from `devices.rs:145-158`, `engine.rs:577`) and warn loudly if
  the browser's limit is below our chosen packet. Because fitting no longer
  depends on it, arriving late is harmless.

**Peer-fpp adoption: first packet only, exactly like channel adoption.**
The regulator is split into `Shared` (borrowed only through `&`) and the
consumer-owned `ConsumerState`. `Shared::fpp` is a plain field fixed at
construction, and everything derived from fpp (`packets_in_past`, `up_to_now`,
`beyond_now`, fade ramps, `burg`, both `TimingStats` windows, per-channel
`ChannelState`) lives in `ConsumerState`, which the producer cannot write
soundly. Channel adoption (`Shared::adopt_channel_count`) already solves the
same problem: it runs on the producer only while `last_seq_in == SEQ_NONE`,
writes slot payloads and the relaxed `num_channels` before the Release of
`last_seq_in`, and the consumer picks the width up after its Acquire, against
state that is clean because it is fresh or was cleared by a quiesced
`reset()`. fpp follows the same path:

- **Where the value comes from.** `deliver_received_packet` (`transport.rs`)
  passes `header.buffer_size`. `Regulator::push_at` gains an `fpp` parameter.
- **Producer, first packet** (`last_seq_in == SEQ_NONE`):
  - If fpp is outside `[MIN_FPP, MAX_FPP]`, reject it as
    `PushOutcome::UnsupportedFpp { got }`.
  - `adopt_channel_count` becomes `adopt_stream_shape(channels, fpp)`. When
    the shape changes, it stores `fpp` and `num_slots` (below) in new relaxed
    atomics next to `num_channels`, resizes and zeroes the first `num_slots`
    slot payloads, and truncates the rest. All of this happens before the
    Release, so the existing soundness argument and doc comment carry over.
- **Producer, later packets.**
  - A packet whose fpp differs from the adopted one is rejected as
    `PushOutcome::FppChanged { adopted, got }`, counted in `packets_rejected`,
    and gets a throttled warning, like `ChannelCountChanged`.
  - `WrongPacketSize` stays as a defensive check
    (`samples.len() != channels * fpp`).
- **Consumer.**
  - `ConsumerState::observe_stream(sh)` runs right after the Acquire that
    shows a started stream, in both `pop_frames` and `pull_packet`, before
    the `in_startup` check.
  - If `sh.fpp()` differs from the consumer's current fpp, it calls
    `configure_fpp(fpp)`. That recomputes `packets_in_past`, `up_to_now` and
    `beyond_now`, refills the fade prefixes, sets the Burg size and both
    `TimingStats` windows, reconfigures every `ChannelState`, resets the
    auto-mode initial tolerance (`fpp × AUTO_INIT_VAL_FACTOR`, as `reset()`
    computes it), and flushes the FIFO (which is at the old packet size).
  - All of that stays inside capacity preallocated at construction, so the
    consumer never allocates.
  - The check is idempotent, so two call sites are harmless. Nothing has run
    `process_burg` before a stream starts, so there is no data to migrate.
- **Readers switch to the adopted value.** `fpp_duration_ms`, `latency_ms`,
  `update_headroom` and `Regulator::fpp()` read the adopted fpp, so `fpp()`
  also works as a diagnostic.

No new concurrency argument is needed: adoption only happens at a stream's
start, and `reset()`'s quiescence contract (below) already governs getting back
there.

**Preallocation, independent of fpp.**

- **`ChannelState`:** `packet_ring` and `predicted_past` become flat
  `Vec<f32>`s of `UP_TO_NOW_MAX` samples, used as `packets_in_past` chunks of
  `fpp`. Since `packets_in_past × fpp == up_to_now`, their memory is bounded
  by history length, not packet count.
  - `ring_buffer_push` writes chunk `wptr`.
  - `real_past_into_prediction` copies two slices.
  - `predicted_past.rotate_left(1)` becomes `[..up_to_now].rotate_left(fpp)`.
  - Per-packet buffers are `MAX_FPP` long and used as `[..fpp]`.
  - `prediction` is `UP_TO_NOW_MAX + 2·MAX_FPP`.
  - `UP_TO_NOW_MAX = max(2·2·HIST_FPP, MAX_FPP) = 1024`. The
    `packets_in_past` rule gives 256 for fpp ≤ 128, 2·fpp up to 256, and fpp
    above that.
- **`BurgAlgorithm`:** gains `set_size(size)`, which stays within its
  construction capacity. It sets `order` and `input_size` and re-initializes
  the `ak_reset` prefix.
- **`TimingStats`:** `data` is preallocated for the `MIN_FPP` window
  (`sample_rate / MIN_FPP / WINDOW_DIVISOR`). `set_window` resets it within
  that capacity.
- **Cost:** about 50 KB per channel, about 400 KB at 8 channels. State this in
  the commit.

**Constant-horizon slot ring.** With a fixed 4096 slots, memory is
`4096 × fpp × channels × 4 B`: 16 MiB at 128 × 8 ch, but 128 MiB at 1024 × 8 ch.

- `NUM_SLOTS` is replaced by a budget of `4096 × HIST_FPP` frames.
- `num_slots(fpp)` is the largest power of two with `num_slots × fpp ≤ budget`,
  capped at `MAX_SLOTS = 16384`:
  - **Power of two** so `seq % num_slots` stays continuous across the `u16`
    wrap.
  - **The cap** keeps the ring at or under a quarter of sequence space, inside
    `seq_is_newer`'s half-space window.
- The horizon is about 10.9 s at any fpp ≥ 32 (5.5 s at 16, set by the cap),
  and memory stays ≤ 16 MiB.
- The slot array is preallocated at `MAX_SLOTS` entries with empty payloads
  (about 640 KB of headers). Only the first `num_slots` are ever sized.
- `slot_index` and `find_best_packet`'s `new_pkts > NUM_SLOTS` resync check
  take the adopted `num_slots`, read after the Acquire.
- Rewrite the module header's "Sequence space vs. slot space" section (it cites
  4096 slots / ~10.9 s at fpp=128) in terms of the budget.

**Mirrors.** The regulator keeps its "no `crate::` imports" rule. `MIN_FPP` and
`MAX_FPP` are local `pub(crate)` mirrors, pinned against `limits.rs` by the same
native test that pins `MAX_CHANNELS == MAX_WIRE_CHANNELS`.

`reset()` (`regulator.rs:1068`) is sound only because the session quiesces
both halves first: `release` awaits `AudioContext.close` and skips the reset
if that times out (`session.rs:1017-1045`, WEB-53 Bug 7 / WEB-66). Any new
adoption path must keep that requirement.

**Frame FIFO sizing uses `MAX_FPP`.** The regulator's frame FIFO and pull
target are preallocated for the largest fpp a peer may announce, not the
construction-time `fpp`: `with_callback_capacity` sizes them from `MAX_FPP`.
That is what lets an adopted peer fpp (any value up to `MAX_FPP`) flow through
`pop_frames_at` with no render-thread allocation. Adoption flushes the FIFO,
which is at the old stride/packet size.

**Ring-buffer invariant:** `RING_BUFFER_SIZE = 4096` (`ring_buffer.rs:15`) is a
hard ceiling on `fpp × send_channels`, not a sizing nit — `write` rejects
oversized writes (`:155-158`) while both send paths wait for
`available >= fpp*channels`, so any excess is a **permanent silent stall**.
Raise it to hold ≥2 packets at the wire ceiling and add a `debug_assert!` in
`set_audio_buffers`.

### Phase 6 — Per-connection stream buffers; retirement replaces reset
*Delivers: every connection gets its own `Regulator` and `RingBuffer`, and
`Regulator::reset` is deleted. Ships with connect and the engine still
coupled. It exists so Phase 7's split needs no regulator quiescence.*

**The problem this solves.** `Regulator::reset` is `unsafe` and requires both
halves quiesced. Today the only consumer-side proof is the `AudioContext`
close inside `release` (`session.rs:926`). Once audio outlives a connection
(Phase 7), a network-only `disconnect()` leaves the worklet popping, so
nothing can prove quiescence and the regulator cannot be reset.

**Decision (settled with the user): retire, never reset.** Each connection
allocates fresh stream buffers. A disconnected connection's buffers are
*retired*. They are freed only after the render thread confirms it has let go
of them. The regulator's concurrency model is untouched: it stays strictly
SPSC with its existing single-atomic publication. Rejected alternatives:

- *An in-band reset request run by the consumer.* This adds a new protocol
  inside the regulator. `Shared::reset` zeroes up to ~16 MiB of slot payload,
  which is not real-time safe. A suspended context never pops, so the next
  connect would wait forever.
- *Closing and recreating the `AudioContext` per disconnect.* It causes an
  audible gap and loses output-device routing. iOS needs a new user gesture
  per context.

**The quiescence proof: a worklet `'stopped'` acknowledgement.** A worklet's
`port.onmessage` and `process()` both run on the render thread and never
overlap. `worklet.js`'s `'stop'` handler sets `this.stopped`. `process()`
checks that flag before calling `this.processor.render(...)`. So once the
handler has run, that processor never touches its pointers again. The handler
now also posts `'stopped'` back on the port. Receiving it on the main thread
is the proof, with no context close. `teardown_worklet_node`
(`engine.rs:413`) returns a future (or registers a callback) that settles on
that acknowledgement.

- **`Connection { id, transport, regulator: Box<Regulator>, ring:
  Box<RingBuffer>, wire, state }`** is in `webtrip-web`, because it holds a
  transport. The session holds `Vec<Connection>`. `connect()` enforces
  `len <= 1` by **returning an error naming the limit**, never by silently
  ignoring the call.
  - The session's `local_to_network_buffer` / `network_to_local_buffer`
    fields (`session.rs:283-284`) and their `ManuallyDrop` go away.
  - `audio_buffer_config` and `buffer_ptrs` read from the connection.
- **The processor's stream list is immutable for the life of a worklet node.**
  The node is built pointing at one connection's buffers, or at none
  (`AudioProcessor::new`, the empty `net_in`/`net_out` case). Adding or
  removing a connection while an engine runs rebuilds the node through
  `build_and_connect_worklet_node` (`engine.rs:343`). The output-device change
  already uses and exercises that path.
  - In this phase connect still brings the engine up after the dial, so the
    first node is simply built against the new connection. The rebuild path
    is wired and tested here, and becomes the hot path in Phase 7.
- **Retirement.**
  - On disconnect, the transport is closed first, so the producer is
    quiesced.
  - The connection's buffers then move into a session-owned retired list,
    keyed by the node they were attached to.
  - That entry is freed when the node's `'stopped'` arrives.
  - If the context closes first (`stopAudio`, or the session `Drop` path),
    the successful close frees every retired entry.
  - If the close times out, the entries stay in the list, as `release`
    already keeps the regulator un-reset on timeout today.
  - If no acknowledgement ever arrives (for example, a suspended context
    might not deliver port messages), the entry waits for the context close.
    That is sound and bounded to one buffer pair per missing
    acknowledgement.
  - The session `Drop` path (`session.rs:1177`) uses the same retirement
    rule in place of its hand-built buffer tuple.
- **Delete `Regulator::reset`** with its `Shared::reset` / `ConsumerState::reset`
  helpers and its `unsafe` contract.
  - Re-adoption after a reconnect is now construction, since a fresh
    regulator starts at `SEQ_NONE`.
  - Delete the reset-specific tests (scrubbing, re-adoption after `reset()`,
    the Phase 3 FIFO-flush-on-reset case, the Phase 5 "re-adoption of a
    different fpp after `reset()`" case). Don't port them.
  - Update the doc comments and module header that cite the reset/quiescence
    contract (WEB-53 Bug 7 / WEB-66) to describe retirement.
  - `release` no longer resets anything. Its `AudioContext.close` timeout
    handling stays, because it still governs freeing.
- **`AudioParams::send_channels` is removed from `AudioParams` entirely.** It
  is per-connection wire state, read every callback (`processor.rs:172`) from
  a leaked `'static`. The wire format moves into `RingBuffer`
  (`set_wire_format`/`wire_channels`). The ring is already the
  per-connection send-side object, the render thread already reaches it
  through `SharedPtr`, and the stride of its contents is its own property.
  `AudioParams` is then purely device and metering state.
- **Stats.** `SessionStats` splits into `ConnectionStats` plus an aggregate.
  `build_session_stats` (`session_stats.rs:59`) splits into a pure
  `build_connection_stats` plus a trivial aggregate. The existing
  `build_session_stats_maps_all_fields` test ports onto the former.
- **Cost, stated in the commit.** Each connect allocates a regulator on the
  main thread, off the audio thread. Connecting or disconnecting while an
  engine runs swaps the worklet node, which is the same brief glitch an
  output-device change already causes.
- **Homes.** `Connection`, the retired list and the acknowledgement plumbing
  are `webtrip-web`. `ConnectionStats` and its builder are `webtrip-core`.

### Phase 7 — Device↔network separation
*Delivers: audio-without-network, and network-without-audio on **both**
transports. Last, because it depends on Phase 6's retirement. A network-only
`disconnect()` retires the connection's buffers while the worklet keeps
running.*

Today engine bring-up is a step inside connect. `bring_up` (`session.rs:829`)
dials the transport, then runs `start_engine_unless_aborted` (`:998`). That
spawns `bring_up_engine` (`:201`) and races it against the connect's abort
promise, so `disconnect()` is never blocked on `getUserMedia` (WEB-69). On
success it publishes the transport and engine and starts the network loop
with no await in between. Teardown is `disconnect` (`:635`) →
`stop_network_loop` → `release` (`:926`).

- **Split the engine half out of `bring_up`/`release`.**
  - Public `startAudio` runs the engine bring-up
    (`start_engine_unless_aborted`). It keeps the abort race and the cancel
    flag, so a `stopAudio()`/`disconnect()` during a pending permission
    prompt still settles promptly.
  - `stopAudio` runs the engine half of `release`: `AudioContext.close`,
    which also frees the retired entries.
  - `connect` keeps the dial, the failure bridge and publishing the
    transport. When an engine is running, it then rebuilds the node against
    the new connection (Phase 6).
  - `disconnect` closes the transport, then rebuilds the node with no
    network stream if an engine is running, and retires the buffers.
  - `connect_to_studio` keeps its exact signature (Demo.tsx is out of scope)
    and becomes `start_audio(...)` then `connect(...)`. This reverses today's
    order, which dials first and then starts the engine.
- **Tick wiring stays as is.** The tick closure already ticks the shared
  `TransportSlot` (`Rc<RefCell<Option<Box<dyn Transport>>>>`, `session.rs:87`,
  set up in `start_network_loop` at `:1093`). There is no self-pointer and no
  `unsafe`, and `Rc`/`RefCell` stay at the `web_sys` callback boundary. The
  `TickClock` below must drive this same slot, which becomes the
  connection's transport.
- **State split.**
  - `SessionState` becomes the *network* state.
  - Add `AudioState { Stopped, Running }`.
  - Session-level state becomes a pure
    `aggregate_network_state(&[NetworkState]) -> SessionState`. It is
    natively testable, and it is the seam where 1→N generalises.
  - `SessionState` and the connect/disconnect gate live in `Lifecycle`
    (`lifecycle.rs`, one atomic epoch+state word). The network state and
    `AudioState` must be expressed in that state machine, or in a sibling
    with the same epoch discipline, not as a separate flag alongside it.
- **The WebRTC tick-clock asymmetry, addressed not papered over.**
  - WebRTC's tick is driven by `Atomics.waitAsync` on the ring's has-data
    flag. Only `RingBuffer::write` from the worklet sets that flag, so with
    no worklet there are no ticks and WebRTC cannot *receive* either.
    Packets pile up in `receive_queue` (`webrtc.rs:227`), which is drained
    only inside `tick`.
  - Introduce a session-owned
    `TickClock { None, Atomics(AudioCallbackLoop), Interval(IntervalTickLoop) }`
    with `ensure_running_for(audio_running, any_connection_live)`.
  - `IntervalTickLoop` is a small sibling of `AudioCallbackLoop`
    (`audio_callback_loop.rs:142`) with the same stop-on-Drop discipline.
  - A network-only connection's regulator has no consumer. It fills, and its
    slot ring overwrites, exactly as when a worklet stalls today. When audio
    starts later, the node is built against it and `find_best_packet`'s
    resync handles the backlog.
- **Homes.** `TickClock` and `IntervalTickLoop` are `webtrip-web`.
  `AudioState` and `aggregate_network_state` are `webtrip-core`.

**Non-negotiable sequencing:** 0a → 0b → 0c → 0d, all before Phase 1, so
later phases create new files in their final crate.

- 2 before 3: the regulator's frame output feeds the mixer's `net_in`.
- 3 before 5: adopting a peer fpp without the frame FIFO breaks playback
  immediately, because `remote_interleaved` would be `fpp*channels` while the
  mixer expects `frames*channels`.
- 4 after 3: it changes the frame↔packet arithmetic that phase establishes.
  Phase 4 is independent of 5 in either order.
- 6 before 7: a network-only disconnect needs retirement, because nothing can
  prove the consumer quiescent while the worklet keeps running.
- 6 after 5: it deletes `reset()`, so Phase 5's reset-based re-adoption test
  never needs porting.
- Phase 1 is otherwise independent and can ship at any point after 0d.

## Testing

Per AGENTS.md: extend the existing matrices rather than adding point tests —
`devices.rs` channel-resolution boundaries, `worklet.rs`'s five
`render_clamps_*`, `processor.rs`'s two mapping matrices, `regulator.rs`'s
adoption/rejection tests, `session.rs`'s transport-state matrix. Tests pinning
the literal 8 that must retarget: `session.rs:1307-1312`,
`protocol.rs:1038-1068` and `:1072-1104`, `params.rs:304-316`,
`devices.rs:372-396`.

Genuinely new tests, each able to fail on a plausible bug:

- `pop_frames_at` continuity over an (fpp × callback-frames × channels) matrix:
  a known ramp arrives sample-exact with no gaps, repeats or drops, and one
  packet is consumed per packet of output.
- Pull-clock integrity: across the same ratios (including callback < packet and
  a non-divisor) the pull statistics see exactly one tick per callback at the
  callback interval, and a steady peer's auto tolerance stays near the slower
  of the two clocks.
- Latency and reset: frames queued in the FIFO reach `latency_ms()`, and
  `reset()` discards them. (The reset half is deleted with `reset()` in
  Phase 6.)
- `fit_fpp` boundary matrix including its `limited_by` reason.
- Peer-fpp adoption, extending the regulator's adoption/rejection test:
  - first-packet adoption across `{MIN_FPP, 64, 128, 256, MAX_FPP}` ×
    channels, with `fpp()` and `latency_ms()` reflecting the adopted value;
  - both sides of `MIN_FPP` and `MAX_FPP` (`UnsupportedFpp`);
  - a mid-stream `FppChanged`;
  - re-adoption of a different fpp after `reset()` (deleted with `reset()` in
    Phase 6; the reconnect test there replaces it).

  The `pop_frames_at` continuity matrix gains an adopted fpp that differs from
  the construction fpp, including 1024 against a 128-frame callback.
- A loss burst at an adopted fpp ≠ 128 conceals with the reconfigured Burg
  size. This catches a stale `up_to_now` or `beyond_now` slicing the wrong
  prefix.
- `test_loss_burst_wider_than_ring_resyncs_read_pointer` is parameterized over
  adopted fpp, so the resync fires at `num_slots(fpp)`, plus one sequence
  wraparound case at a non-default `num_slots`.
- `test_deserialize_buffer_size_boundaries` retargets to `MIN_FPP - 1`,
  `MIN_FPP`, `MAX_FPP` and `MAX_FPP + 1`. Protocol tests that use
  out-of-range sizes (e.g. `buffer_size = 4`) move into range.
- Rate-mismatch matrix (device rate × wire rate, e.g. 48/48, 96/48, 44.1/48):
  per callback the pop count and the converted frame count are right, a known
  tone survives conversion at the right pitch in both directions, and — the
  regression this phase exists for — a steady stream at a 2:1 rate ratio
  produces **no** concealment and leaves tolerance/headroom unchanged. Plus a
  test that unconverted mismatched rates make `connect` fail with both rates
  named.
- A browser test that a connect-only session with no worklet still ticks a
  `MockTransport` — the exact bug the WebRTC clock asymmetry causes.
- Retirement (browser): a connection's buffers are not freed until the
  replaced node's `'stopped'` acknowledgement arrives. The test holds the
  acknowledgement back and asserts the buffers are still live, then delivers
  it and asserts they are freed. When no acknowledgement arrives, retired
  buffers are freed by a fulfilled `AudioContext.close` and kept on a close
  timeout.
- A disconnect-then-reconnect with audio running adopts the second peer's
  channel count and fpp. This catches a stale regulator carried across
  connections, which is the bug `reset()` used to prevent.
- `connect()` with one connection live returns an error naming the limit.

No test should merely restate 32, 8 or 1200.

The crate split itself adds no tests: renames and attribute moves have no
logic that could fail. Its checks are the 0b grep acceptance, `check:core`,
and a diff showing `pkg/webtrip.d.ts` from `npm run build:wasm` is
byte-identical before 0a and after 0d (run in the phase, not committed).
Tests extracted in 0b move with their code.

## Verification

0. `npm run check:core` (added in 0c) — native check of `webtrip-core`,
   `webtrip-api` and the `webtrip` facade with no features; fails if any reaches `web-sys`/`js-sys`.
1. `npm run check` — first gate; catches missed renames and any const
   assertions. Never bare `cargo check` (`web_sys` unstable gating).
2. `npm run test` — native units across the workspace (`--workspace`): `session.rs`, `protocol.rs`, `params.rs`,
   `devices.rs`, `regulator.rs`, `processor.rs`, plus the new `dsp`/`mixer` tests
   and the regulator's frame-pop tests.
3. `npm run test:wasm` — the **only** step exercising `worklet.js`, so the only
   proof the Rust→JS handoff works. Watch `buffer_setters_feed_capture_path`
   (`engine.rs:1172`) and `worklet_port_tracks_capture_lifecycle` (`:952`).
4. `npm run build:wasm` — release build; `debug_assert!`s do not run there, so
   the `worklet.rs:62-63` clamps are the real safety net.
5. **Manual, needs hardware:** attach a >8-channel interface and confirm
   `getInputDeviceChannels` reports its true width. Chrome's fake device reports
   a narrow count, so no automated test can distinguish "ceiling raised" from
   "ceiling still 8" — and that is the user's actual goal.

## Follow-ups (explicitly not in this work)

- **Burg AR order is 255** because `BurgAlgorithm::new` sets `order = size - 1`
  (`regulator.rs:161`) with `size = up_to_now`, pinned at 256 by
  `HIST * (HIST_FPP/fpp)`. Typical PLC uses 16-64. This is what actually gates
  wide channel counts and low fpp, and it affects today's 8-channel path too.
  Needs measurement and listening tests.
- **Lower fpp does not hurt concealment quality** — the training window is in
  frames and stays at 256, so smaller packets mean smaller gaps against
  undiminished history. It costs ~4.5× more Burg CPU/sec at a constant
  per-packet loss rate. CPU and quality move in opposite directions.
- `webrtc.rs:339` and `:1140` allocate `16 + samples*4` while
  `webtransport_worker.rs:397` uses `HEADER_SIZE + samples*2` for the same
  16-bit format; the WebRTC one also hardcodes `16` instead of `HEADER_SIZE`.
- `tests/integration/run.mjs:159` maps `mock: m.TransportType.Mock`, a variant
  removed in `260231e`; `ttByName.mock` is `undefined` so a `mock` run throws.

## Critical files

Paths below are post-Phase-0d. Pre-move, all of `src/audio/` (plus
`src/session.rs`, `src/lifecycle.rs`, `src/api/`, `src/models/`).

`crates/core/src/`: `audio/processor.rs`, `stream/regulator.rs`,
`stream/ring_buffer.rs`, `network/transport.rs`, `network/protocol.rs`.

`crates/web/src/`: `audio/engine.rs`, `audio/worklet.rs`,
`audio/worklet.js`, `network/webtransport_worker.rs`, `network/webrtc.rs`,
`session.rs`.

New in `crates/core/src/`: `limits.rs` (both ceilings — shared by
`audio/` and `network/`, so it sits above both), `audio/dsp.rs`,
`audio/mixer.rs`, `network/signaling_message.rs`,
`session_stats.rs`.

Config to update: root `Cargo.toml` (workspace) and the four crate manifests (including the
`crates/webtrip` facade),
`package.json` scripts, the CI workflow (add `check:core`), `AGENTS.md`
(*Build Commands*, *Key Modules*), `README.md`.

Docs to update: `docs/ARCHITECTURE.md` (final crate/module layout, in Phase
7's wrap-up), `docs/CHANNEL_MODEL.md` (asserts the 8-channel limit at
`:432-441`; `:179-182` and `:428-431` describe the settings-only capture
behaviour that Phase 1 fixes), `docs/WASM_TESTING.md:491`.
