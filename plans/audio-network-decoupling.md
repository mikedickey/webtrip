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
(`webtrip-core`, `webtrip-api`, `webtrip-web`).

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
crates/webtrip-core/src/          portable; no web-sys / js-sys, ever
  limits.rs
  audio/    processor, params, dsp, mixer
  network/  protocol, transport (trait + deliver_received_packet + state mapping),
            signaling_message (SignalingMessage, HubConnectionState,
            parse_ice_candidate_json)
  stream/   shared_ptr, ring_buffer, regulator, frame_reader   (the bridge primitives)
  lifecycle.rs, session_stats.rs (SessionStats, build_session_stats, tick_decision)
crates/webtrip-api/src/           portable; today's api/ + models/
crates/webtrip-web/src/           the cdylib; the only crate that touches the browser
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

### New modules

Four new portable modules, all in `webtrip-core`, layered acyclically
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
- **`stream/frame_reader.rs`** — `FrameReader`, the frame-granular adapter.

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
into three steps so the pure renames stay reviewable apart from the real
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
  uses today (`js_sys::Date::now()`). Delete `Regulator::now_ms` and its `cfg`
  branches. `deliver_received_packet` gains
  a `now_ms` parameter.
- **Bindings:** move every `#[wasm_bindgen]`/`tsify` on core and api types
  behind `cfg_attr(feature = "wasm", ...)`, with the JS names unchanged. The
  generated `pkg/webtrip.d.ts` must be byte-identical before and after.
- **Transport trait:** `set_on_state_change` takes
  `Box<dyn Fn(TransportState)>`; `connect` returns a portable
  `TransportError`. Web converts to `JsValue` / `named_error` at its
  `#[wasm_bindgen]` boundary, and `notify_transport_state`'s
  `js_sys::Function` call (`transport.rs:173`) moves to web.
- **Extract the portable logic AGENTS.md already names** out of the browser
  modules into core: `SessionStats`/`build_session_stats`
  (`session.rs:72,153`), `tick_decision` and `parse_ice_candidate_json`
  (`webrtc.rs:93,59`), `SignalingMessage`/`HubConnectionState`
  (`signaling.rs`). Their tests move with them, never duplicated.
- **Acceptance:** `grep -rE 'web_sys::|js_sys::' src/webtrip_core src/webtrip_api`
  is empty, and nothing under those two trees imports a web module
  (`crate::session`, `crate::audio`, `crate::network`, `crate::test_support`). This is what makes 0c mechanical.

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
  and extract the shared "resolve a track's width" step so `engine.rs:259-260`
  (which passes `None` for the capability today) and
  `get_input_device_channels` (`devices.rs:174-178`) cannot drift. Reuse
  `resolve_input_channels` (`devices.rs:128`).

### Phase 2 — `dsp.rs` extraction + `MixEngine`/`SimpleMixer`
*Pure refactor. Groundwork for everything else. `dsp.rs` and `mixer.rs` are
created in `webtrip-core/src/audio/`.*

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

Also raise the `processor.rs:294-299`/`:315-320` preallocation basis off the
bare `128`, so decoupled fpp can never trigger a `resize` on the audio thread
(`processor.rs:503-506`).

### Phase 3 — `FrameReader` + `Regulator::pop_at`
*Delivers: callback size and packet size fully independent, both directions.*

`Regulator::pop` must stay packet-granular: `interleave_output`
(`regulator.rs:1434-1438`, via `interleave_channel` `:384`, whose `zip`
truncates silently) drops the tail on a short output and leaves stale audio
on a long one; `last_seq_out` advances once per pop (`:1384`); `pull_stats`
ticks once per pop (`:1327-1330`); and the tolerance controller is
packet-denominated throughout (`:1553`, `update_tolerance` `:1565-1621`).

`pop_at(output, now_ms)` already exists from Phase 0b's clock injection; this
phase builds on it rather than introducing it. Add one up-front
`debug_assert_eq!(output.len(), samples_per_packet)` to `interleave_output`:
the reader always hands an exactly-sized buffer, so tail-drop and stale-tail
become **unreachable by construction** rather than patched.

**The statistics-integrity mechanism is the heart of this.** Per callback,
compute the pop count once, then space each pop's timestamp exactly one packet
period apart, anchored so the last lands on `now`:

```
pops = ceil_div(frames_needed.saturating_sub(queued_frames), fpp)
for i in 0..pops:
    reg.pop_at(&mut packet, now_ms - ((pops-1-i) as f64) * packet_period_ms)
```

This holds the invariant *`pull_stats` ticks exactly once per packet consumed,
at one packet period of spacing, for every fpp/callback ratio*: at `fpp ==
frames` it is bit-identical to today; at callback > packet it reports the
device's true steady clock instead of manufacturing phantom jitter from
zero-length intervals; at callback < packet most callbacks pop zero times and
tick not at all. Anchoring at `now` (not running a virtual clock forward) keeps
timestamps from ever running ahead of the push side's `Self::now_ms()`
(`regulator.rs:1033`), so no packet is judged late by an adapter artifact.

Sizing: `(max_callback_frames + max_fpp) * MAX_WIRE_CHANNELS`, allocated once in
`prepare()` from `build_and_connect_worklet_node` — main thread, ~200 KB.
Startup needs no special case (`pop_internal` already returns silence before the
first packet, `regulator.rs:1333-1335`). Mid-stream format change flushes the FIFO
(queued frames are at the old stride) and reports it; capacity is at the ceiling
so it never allocates.

Delete `processor.rs:501`'s `debug_assert_eq!(fpp, frames)`. Add
`reader.queued_frames()` into `regulator_latency_ms`, which otherwise
under-reports by up to `fpp-1` frames.

### Phase 3b — Sample-rate conversion (device rate ↔ wire rate)
*Delivers: a 48 kHz link works regardless of the device's native rate (e.g.
48 kHz mic + 96 kHz speakers) with no spurious PLC. Builds on Phase 3: the
`FrameReader` and the pop-count formula both assume device frames and wire
frames are the same unit, and that assumption is what this phase removes.*

Requirement: the device rate and the wire rate are independent values, and
every place that converts between "frames the device needs" and "packets the
wire supplies" accounts for the ratio.

- **Make the rates explicit.** Read the actual `AudioContext` rate after
  creation and carry it, with the wire rate, in `MixLayout` (the mixer's
  `prepare`). Replace the hardcoded 48000 at `session.rs:439` with the wire
  rate as a named value. The regulator's `sample_rate` stays the **wire** rate
  (it derives packet duration and tolerance windows from it); it must never
  see the device rate.
- **Receive direction:** convert the regulator's wire-rate output to the
  device rate before the mixer's `net_in`. Phase 3's pop count then uses
  `frames_needed` converted into wire-rate frames:
  `ceil_div(frames_needed * wire_rate / device_rate - queued_wire_frames, fpp)`,
  so `pull_stats` still ticks exactly once per packet consumed, one packet
  period apart.
- **Send direction:** convert captured device-rate frames to the wire rate
  before they reach the ring buffer, so packets carry wire-rate audio and the
  header's rate field (`protocol.rs`) is truthful.
- **Where the conversion lives is a decision to settle at the start of this
  phase, not assumed here.** The candidates:
  1. Create the `AudioContext` at the wire rate (`sampleRate: 48000`) and let
     the browser resample at the device edge. Smallest change, but the
     quality and latency are the browser's, the rate is only a *request* the
     browser may not honour, and a native host (see AGENTS.md portability)
     would have no equivalent.
  2. Convert in our own portable code (a resampler in `dsp.rs`, used by the
     mixer), with the context left at the device rate. More work, but it is
     testable natively and reusable by non-browser hosts.
  3. Both: request the wire rate, and fall back to our own conversion when
     the browser does not give it.

  Under the core/web split, option 1 lives entirely in `webtrip-web`; only
  options 2 and 3 put a converter in `webtrip-core` where a native host can
  reuse it. That is a fact to weigh, not a decision made here.
- **Fail loudly in every case.** If the final device rate and wire rate
  differ and no conversion is active, `connect` must return an error naming
  both rates — never run silently at the wrong speed. This is the same
  "diagnosable rail" rule as the channel and packet-size limits.
- **Open question:** the device clock and the peer's clock are separate
  oscillators, so even at identical nominal rates they drift. Whether a
  fixed-ratio converter is enough, or an adaptive ratio is needed, is not
  decided here; the regulator's adaptive tolerance absorbs slow drift today.

### Phase 4 — fpp fitting + peer-fpp adoption
*Delivers: 5-channel links stop silently failing; a 256-fpp peer works instead
of having every packet rejected.*

Pure math in `protocol.rs`, reusing `audio_data_size` (`protocol.rs:73`) so
packet geometry has one definition:
`fit_fpp(max_bytes, channels, bit_depth, preferred) -> FppFit { fpp,
packet_bytes, limited_by }`. Largest power-of-two fpp ≤ preferred that fits
(power-of-two because `packets_in_past = HIST*(HIST_FPP/fpp)` truncates for
non-divisors, `regulator.rs:940-946`), clamped to `[MIN_FPP, MAX_FPP]`.
`MIN_FPP = 32` recorded with its reason (below that the 16-byte header exceeds
25% overhead); `FppLimit::Floor` surfaces as an **error**, not a silently
oversized packet.

`session.rs:412`'s single `buffer_size` splits into `send_fpp` and
`receive_fpp` — genuinely different things (`:449-454` configures receive via
`Regulator::with_params`, `:1061` send via `audio_buffer_config`). **No serializer change is needed**: `serialize_samples_into` derives
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

**Peer-fpp adoption:** `deliver_received_packet` (`transport.rs:40-41`) already
has `header.buffer_size` and simply doesn't pass it — add it. Today
`adopt_channel_count` (`regulator.rs:1236`) runs on the producer, before the
`Release` store of `last_seq_in` (`:1195`), and is sound because it touches
only producer-side state (slot payloads, `num_channels`); its doc comment
(`:1211-1233`) carries that argument.

**Open — the adoption mechanism must be redesigned before this phase starts.**
The regulator is now split into a producer-owned `Shared` and a
consumer-owned `ConsumerState` (`regulator.rs:766-851`). `fpp` and
`sample_rate` are plain fields of `Shared`, fixed at construction (`:782-783`),
and everything fpp-derived — `packets_in_past`, `up_to_now`, `beyond_now`,
fade ramps, `burg`, both `TimingStats`, per-channel states — lives in
`ConsumerState` (`:825-851`), which the producer cannot write soundly. So
"extend `adopt_channel_count` to recompute the fpp-derived fields on the
producer" is no longer possible as written. A replacement has to publish the
peer fpp through the same `last_seq_in` Release/Acquire and let the consumer
rebuild its own derived state on observing it, with that state preallocated
at the `MAX_FPP` ceiling so the consumer never allocates. Settle the shape
with the user first.

`reset()` (`regulator.rs:1068`) is sound only because the session quiesces
both halves first: `release` awaits `AudioContext.close` and skips the reset
if that times out (`session.rs:1017-1045`, WEB-53 Bug 7 / WEB-66). Any new
adoption path must keep that requirement.

**Ring-buffer invariant:** `RING_BUFFER_SIZE = 4096` (`ring_buffer.rs:15`) is a
hard ceiling on `fpp × send_channels`, not a sizing nit — `write` rejects
oversized writes (`:155-158`) while both send paths wait for
`available >= fpp*channels`, so any excess is a **permanent silent stall**.
Raise it to hold ≥2 packets at the wire ceiling and add a `debug_assert!` in
`set_audio_buffers`.

### Phase 5 — Device↔network separation
*Delivers: audio-without-network, and network-without-audio on **both**
transports. Independent of 2-4; easier after 2.*

Today engine bring-up is a step inside connect. `bring_up`
(`session.rs:920`) dials the transport, then runs
`start_engine_unless_aborted` (`:1079`), which spawns `bring_up_engine`
(`:304`) and races it against the connect's abort promise so `disconnect()`
is never blocked on `getUserMedia` (WEB-69). On success it publishes the
transport and engine and starts the network loop with no await in between
(`:977-982`). Teardown is `disconnect` (`:733`) → `stop_network_loop` →
`release` (`:1017`), which closes the transport, awaits
`AudioEngine::stop_capture`, and resets the regulator only if the
`AudioContext` close proved the worklet quiescent.

- **Split the engine half out of `bring_up`/`release`.** Public `startAudio`
  runs the engine bring-up (`start_engine_unless_aborted`), keeping the
  abort race and the cancel flag so a `stopAudio()`/`disconnect()` during a
  pending permission prompt still settles promptly. `stopAudio` runs the
  engine half of `release`. `connect` keeps the dial, the failure bridge,
  and publishing the transport. `connect_to_studio` keeps its exact
  signature (Demo.tsx is out of scope) and becomes `start_audio(...)` then
  `connect(...)` — note that this reverses today's order (dial first, then
  engine).
- **Open — regulator reset without engine teardown.** `Regulator::reset`
  requires the consumer to be quiescent, and today the only proof is the
  `AudioContext` close inside `release`. Once audio outlives a connection,
  a network-only `disconnect()` leaves the worklet popping, so `release`
  can no longer reset the regulator the way it does now. Phase 6's
  per-connection regulator plus worklet-node rebuild is one way to provide
  quiescence; deciding how Phase 5 handles it before Phase 6 is a question
  for the user.
- **Tick wiring stays as is.** The tick closure already ticks the shared
  `TransportSlot` (`Rc<RefCell<Option<Box<dyn Transport>>>>`,
  `session.rs:190`, closure at `:1212-1220`): no self-pointer, no `unsafe`,
  `Rc`/`RefCell` only at the `web_sys` callback boundary. The `TickClock`
  below must drive this same slot. It becomes a `Vec` only when N>1 actually
  ships.
- **State split:** `SessionState` becomes the *network* state; add
  `AudioState { Stopped, Running }`. Session-level state becomes a pure
  `aggregate_network_state(&[NetworkState]) -> SessionState` — natively
  testable, and the seam where 1→N generalises. `SessionState` and the
  connect/disconnect gate live in `Lifecycle` (`src/lifecycle.rs`, one atomic
  epoch+state word); the network state and `AudioState` must be expressed in
  that state machine (or a sibling with the same epoch discipline), not as a
  separate flag alongside it.
- **The WebRTC tick-clock asymmetry, addressed not papered over.** Its tick is
  driven by `Atomics.waitAsync` on the ring's has-data flag, set only by
  `RingBuffer::write` from the worklet — so no worklet means no ticks and
  WebRTC cannot *receive* either; packets pile up in `receive_queue`
  (`webrtc.rs:294`, drained only inside `tick` at `:464`). Introduce a
  session-owned
  `TickClock { None, Atomics(AudioCallbackLoop), Interval(IntervalTickLoop) }`
  with `ensure_running_for(audio_running, any_connection_live)`.
  `IntervalTickLoop` is a small sibling of `AudioCallbackLoop`
  (`audio_callback_loop.rs:142`) with the same stop-on-Drop discipline.
- **Homes:** `TickClock`, `IntervalTickLoop` and the `TransportSlot` wiring are
  `webtrip-web`; `AudioState` and `aggregate_network_state` are
  `webtrip-core`.

### Phase 6 — Per-connection restructuring
*Groundwork only. Last, because it touches everything.*

Introduce `Connection { id, transport, regulator, reader, ring, wire, state }`;
session holds `Vec<Connection>` and `connect()` enforces `len <= 1` by
**returning an error naming the limit**, not silently ignoring.

- **`AudioParams::send_channels` is removed from `AudioParams` entirely** — it is
  per-connection wire state in a leaked `'static` read every callback
  (`processor.rs:465`). The wire format moves into `RingBuffer`
  (`set_wire_format`/`wire_channels`): already the per-connection send-side
  object, already reached by the render thread via `SharedPtr`, and the stride
  of its contents is genuinely its own property. `AudioParams` is then purely
  device/metering state.
- One `Regulator` and one `RingBuffer` **per connection** — the single-consumer
  problem dissolves rather than being solved, each ring staying strictly SPSC.
- Concurrency rule, stated rather than hand-waved: **the processor's stream list
  is immutable for the life of a worklet node.** Adding/removing a connection
  rebuilds the node — machinery `engine.rs:332-398` and `teardown_worklet_node`
  (`:399`) already provide and exercise for output-device changes.
- `SessionStats` splits into `ConnectionStats` + aggregate;
  `build_session_stats` (`session.rs:153`) splits into a pure
  `build_connection_stats` plus a trivial aggregate, and the existing
  `build_session_stats_maps_all_fields` test (`session.rs:1364` today, in
  core's `session_stats.rs` after 0b) ports onto the former.
- **Homes:** `Connection` is `webtrip-web` (it holds a transport);
  `ConnectionStats` and its builder are `webtrip-core`.

**Non-negotiable sequencing:** 0a → 0b → 0c, all before Phase 1 (so later
phases create new files in their final crate); 2 before 3 (the reader feeds the mixer's `net_in`); 3 before 4
(adopting a peer fpp without the frame adapter breaks playback immediately —
`remote_interleaved` would be `fpp*channels` while the mixer expects
`frames*channels`); 3b after 3 (it changes the frame↔packet arithmetic that
phase establishes). Phase 3b is independent of 4 in either order. Phase 1 is
otherwise independent and can ship at any point after 0c.

## Testing

Per AGENTS.md: extend the existing matrices rather than adding point tests —
`devices.rs` channel-resolution boundaries, `worklet.rs`'s five
`render_clamps_*`, `processor.rs`'s two mapping matrices, `regulator.rs`'s
adoption/rejection tests, `session.rs`'s transport-state matrix. Tests pinning
the literal 8 that must retarget: `session.rs:1307-1312`,
`protocol.rs:1038-1068` and `:1072-1104`, `params.rs:304-316`,
`devices.rs:372-396`.

Genuinely new tests, each able to fail on a plausible bug:

- `FrameReader` continuity over an (fpp × callback-frames × channels) matrix: a
  known ramp arrives sample-exact with no gaps, repeats or drops, and the pop
  count is as expected.
- Pull-stats integrity: tolerance does not collapse at k pops/callback vs 1:1.
- `fit_fpp` boundary matrix including its `limited_by` reason.
- Rate-mismatch matrix (device rate × wire rate, e.g. 48/48, 96/48, 44.1/48):
  per callback the pop count and the converted frame count are right, a known
  tone survives conversion at the right pitch in both directions, and — the
  regression this phase exists for — a steady stream at a 2:1 rate ratio
  produces **no** concealment and leaves tolerance/headroom unchanged. Plus a
  test that unconverted mismatched rates make `connect` fail with both rates
  named.
- A browser test that a connect-only session with no worklet still ticks a
  `MockTransport` — the exact bug the WebRTC clock asymmetry causes.

No test should merely restate 32, 8 or 1200.

The crate split itself adds no tests: renames and attribute moves have no
logic that could fail. Its checks are the 0b grep acceptance, `check:core`,
and a diff showing `pkg/webtrip.d.ts` from `npm run build:wasm` is
byte-identical before 0a and after 0c (run in the phase, not committed).
Tests extracted in 0b move with their code.

## Verification

0. `npm run check:core` (added in 0c) — native check of `webtrip-core` and
   `webtrip-api` with no features; fails if either reaches `web-sys`/`js-sys`.
1. `npm run check` — first gate; catches missed renames and any const
   assertions. Never bare `cargo check` (`web_sys` unstable gating).
2. `npm run test` — native units across all three crates (`--workspace`): `session.rs`, `protocol.rs`, `params.rs`,
   `devices.rs`, `regulator.rs`, `processor.rs`, plus the new `dsp`/`mixer`/
   `frame_reader` tests.
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

Paths below are post-Phase-0c. Pre-move, all of `src/audio/` (plus
`src/session.rs`, `src/lifecycle.rs`, `src/api/`, `src/models/`).

`crates/webtrip-core/src/`: `audio/processor.rs`, `stream/regulator.rs`,
`stream/ring_buffer.rs`, `network/transport.rs`, `network/protocol.rs`.

`crates/webtrip-web/src/`: `audio/engine.rs`, `audio/worklet.rs`,
`audio/worklet.js`, `network/webtransport_worker.rs`, `network/webrtc.rs`,
`session.rs`.

New in `crates/webtrip-core/src/`: `limits.rs` (both ceilings — shared by
`audio/` and `network/`, so it sits above both), `audio/dsp.rs`,
`audio/mixer.rs`, `stream/frame_reader.rs`, `network/signaling_message.rs`,
`session_stats.rs`.

Config to update: root `Cargo.toml` (workspace) and the three crate manifests,
`package.json` scripts, the CI workflow (add `check:core`), `AGENTS.md`
(*Build Commands*, *Key Modules*), `README.md`.

Docs to update: `docs/CHANNEL_MODEL.md` (asserts the 8-channel limit at
`:432-441`; `:179-182` and `:428-431` describe the settings-only capture
behaviour that Phase 1 fixes), `docs/WASM_TESTING.md:491`.
