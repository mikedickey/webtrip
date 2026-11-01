# Decouple the audio interface from the network protocol

## Context

The request that started this was narrow: now that wire and device channel
counts are configured independently (#86), make WebTrip *see* every channel a
multichannel interface offers. Auditing that found the Web Audio plumbing
already correct — `channelCount: {ideal: N}` capture (`engine.rs:86-92`),
explicit/discrete destination (`engine.rs:265-272`), `outputChannelCount` on the
worklet node (`worklet.rs:126-135`). The plumbing was never the limit.

What the audit exposed is that **one constant, one frame count, and one
connection are each doing the work of two independent concepts**. The channel
ceiling is the most visible symptom, not the disease:

1. **`MAX_CHANNELS = 8` (`protocol.rs:60`) is simultaneously the wire limit and
   the device limit.** Its own doc concedes 8 is "the protocol's own, tighter
   limit" — a WebTrip choice, not a JackTrip constraint. An 18-channel
   interface is addressed at its first 8.
2. **Network frames-per-packet is welded to the audio callback size.**
   `session.rs:292` hardcodes `buffer_size = 128` with no setter;
   `processor.rs:506` asserts `fpp == frames`; `regulator.rs:825-829` rejects
   any packet not exactly `channels * self.fpp` long. The regulator *adopts* a
   peer's channel count but not its fpp. **A peer at 64/256/1024 fpp has every
   packet rejected today** — silence, `packets_rejected` climbing, no
   diagnosis.
3. **Packet size is bounded by nothing.** A packet is `16 + fpp*channels*2`
   bytes. WebTransport ships these as QUIC datagrams
   (`webtransport_worker.rs:507-536`) with no size check; QUIC forbids IP
   fragmentation (RFC 9000 §14) and a DATAGRAM frame must fit one packet
   (RFC 9221), so the practical cap is ~1200 bytes — already exceeded at 5
   channels × 128 fpp. `protocol.rs:315` accepts a legal **65,552-byte**
   packet.
4. **The session owns exactly one device and one connection, inseparably.**
5. **The device sample rate is assumed to equal the wire sample rate, and
   nothing checks it.** `AudioContext` is created with no `sampleRate`
   (`engine.rs:146`), so the browser runs it at the output device's native
   rate, while the regulator and wire are fixed at 48 kHz (`session.rs:292-293`,
   `sample_rate = 48000`). No code compares the two (`get_sample_rate`,
   `engine.rs:167`, has no non-test caller). Observed failure: microphone at
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
groundwork for N connections.

**Out:** `website/src/pages/demo/Demo.tsx` (stays at 1-2 channel capture
modes); changing PLC/Burg concealment behaviour; actually supporting N>1
connections. Pre-release project — **no backward-compatibility shims, rename in
place** (AGENTS.md).

## Target architecture

### Module layout

Everything network-related currently lives under `src/audio/` — `transport.rs`,
`webrtc.rs`, `webtransport*.rs`, `signaling.rs`, `protocol.rs` — which is the
conflation this work exists to undo. `src/audio/mod.rs` already groups its
modules into commented blocks (audio · buffers · wire protocol · network ·
signaling); the directory simply hasn't caught up with how the code is already
thought about.

```
src/audio/    devices, engine, worklet(+js), processor, params, dsp, mixer
src/network/  protocol, transport, webrtc, webtransport, webtransport_worker, signaling
src/stream/   shared_ptr, ring_buffer, regulator, frame_reader   (the bridge primitives)
```

**The deliverable is the dependency rule, not the folders:** `network/` never
imports `audio/`; `audio/` never imports `network/`; both may use `stream/` and
`dsp`. `session.rs` composes them. `regulator.rs` already has **zero**
`wasm_bindgen` references, so the portable core is demonstrably reachable.

### New modules

Four new portable modules, layered acyclically `dsp ← mixer ← processor`:

- **`src/audio/limits.rs`** — `MAX_WIRE_CHANNELS = 8` (renamed in place from
  `protocol.rs:60`, carrying the rail's reason) and `MAX_DEVICE_CHANNELS = 32`.
- **`src/audio/dsp.rs`** — the pure DSP math moved verbatim from
  `processor.rs:15-242` (`map_to_output`, `conform_interleaved_channels`,
  `interleave_planar`, `downmix_to_mono`, `max_channel_rms`, `apply_gain`) with
  its tests. Makes the portability claim structural: `mixer.rs` then has no path
  to `params.rs`'s `#[wasm_bindgen]` types.
- **`src/audio/mixer.rs`** — `trait MixEngine: Send` + `SimpleMixer`.
- **`src/audio/frame_reader.rs`** — `FrameReader`, the frame-granular adapter.

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
`build_and_connect_worklet_node` (`engine.rs:287`) with a type parameter at the
`#[wasm_bindgen]` boundary for no measurable gain; an enum would need editing
for every future engine — the "reshaped boundary" this work exists to avoid.

## Phases

Each phase compiles alone and keeps all three npm checks green.

### Phase 0 — Module reorganization
*Pure file moves and import fixups. No logic change, no test change beyond
paths. First, so every later phase puts its new files in the right home.*

`git mv` the network modules out of `src/audio/` into `src/network/`, and the
bridge primitives into `src/stream/`, per the layout above. Update
`src/lib.rs:2` (`pub mod audio;`) to declare all three, split `src/audio/mod.rs`'s
re-export blocks (`:49-69`) across the three new `mod.rs` files, and fix imports
crate-wide. `dependent_module!` (`src/dependent_module.rs`, used by
`worklet.rs:98` to inline `worklet.js`) must keep resolving — verify with
`npm run test:wasm`, not just `check`.

State the dependency rule in each new `mod.rs` header so the next person
inherits the intent rather than the folder alone.

Reviewable as a diff of pure renames: no line of logic should change in this
phase.

### Phase 1 — Channel-ceiling split + capture-width fix
*Delivers: an 18-channel interface addressed at 32; the mono-narrowing capture
bug fixed. Independent of every later phase.*

- New `limits.rs` with both ceilings. Retarget the **six device-side call
  sites** to `MAX_DEVICE_CHANNELS`: `devices.rs:122`, `engine.rs:90`,
  `engine.rs:267`, `worklet.rs:62-63`, `worklet.rs:87`, `worklet.js:46`.
  `params.rs:64` clamps `send_channels` and correctly stays on the wire ceiling.
- `regulator.rs:68` stays a **deliberate mirror** (that module has no `crate::`
  imports so a native host can reuse it) but becomes `pub(crate)` so a native
  test can pin `regulator::MAX_CHANNELS == MAX_WIRE_CHANNELS`.
- **Delete both `worklet.js:3-4` constants** in favour of new
  `ProcessorHandle::channel_capacity()` (derived from
  `input_scratch.len() / RENDER_QUANTUM_FRAMES`) and `frames_per_quantum()`,
  read in `ensureViews()`. This eliminates the JS duplicate outright rather than
  centralising it — a stale `worklet.js` is a `TypeError` inside the render
  callback that kills the AudioWorkletProcessor and stops all audio.
- Capture fix: make `capability_max_channels` (`devices.rs:145`) `pub(crate)`
  and extract the shared "resolve a track's width" step so `engine.rs:218-219`
  and `get_input_device_channels` (`devices.rs:174-178`) cannot drift. Reuse
  `resolve_input_channels` (`devices.rs:128`).

### Phase 2 — `dsp.rs` extraction + `MixEngine`/`SimpleMixer`
*Pure refactor. Groundwork for everything else.*

`SimpleMixer::render` reproduces `processor.rs:330-423` exactly, composing the
`dsp.rs` functions and never reimplementing them: gain → `max_channel_rms` →
`interleave_planar` **once** → conform to wire width → `map_to_output` for
remote → `map_to_output` for monitor → sum/scale/clamp.

Acceptance: `test_process_stereo_capture_feeds_ring_and_monitor`
(`processor.rs:1012`) and `test_process_conforms_mono_capture_to_stereo_wire_width`
(`processor.rs:1079`) pass **untouched**.

Known cost to state in the commit: the matched-width send case gains one
`frames × channels` memcpy (today it writes straight to the ring,
`processor.rs:467`). Mitigate by passing the preallocated wire staging buffer
*as* `net_out[0].interleaved`, bounding it to one ≤4 KB copy per callback.

Also raise the `processor.rs:294-299`/`:315-320` preallocation basis off the
bare `128`, so decoupled fpp can never trigger a `resize` on the audio thread
(`processor.rs:509-511`).

### Phase 3 — `FrameReader` + `Regulator::pop_at`
*Delivers: callback size and packet size fully independent, both directions.*

`Regulator::pop` must stay packet-granular: `interleave_output`
(`regulator.rs:1054-1063`) drops the tail on a short output and leaves stale
audio on a long one; `last_seq_out` advances once per pop (`:990`);
`pull_stats` ticks once per pop (`:1016-1019`); and the tolerance controller is
packet-denominated throughout (`:1181`, `:1227`, `:1232`, `:1262`).

One pure refactor: split `pop_at(output, now_ms)` out of `pop()`
(`regulator.rs:1012-1023`), with `pop()` delegating. Hoist
`interleave_output`'s per-sample bound check into one up-front
`debug_assert_eq!(output.len(), samples_per_packet)` plus a clamp outside the
loop — the reader always hands an exactly-sized buffer, so tail-drop and
stale-tail become **unreachable by construction** rather than patched.

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
(`regulator.rs:854-857`), so no packet is judged late by an adapter artifact.

Sizing: `(max_callback_frames + max_fpp) * MAX_WIRE_CHANNELS`, allocated once in
`prepare()` from `build_and_connect_worklet_node` — main thread, ~200 KB.
Startup needs no special case (`pop_internal` already returns silence before the
first packet, `regulator.rs:960-963`). Mid-stream format change flushes the FIFO
(queued frames are at the old stride) and reports it; capacity is at the ceiling
so it never allocates.

Delete `processor.rs:506`'s `debug_assert_eq!(fpp, frames)`. Add
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
  `prepare`). Replace the hardcoded 48000 at `session.rs:293` with the wire
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
non-divisors, `regulator.rs:694-700`), clamped to `[MIN_FPP, MAX_FPP]`.
`MIN_FPP = 32` recorded with its reason (below that the 16-byte header exceeds
25% overhead); `FppLimit::Floor` surfaces as an **error**, not a silently
oversized packet.

`session.rs:255`'s single `buffer_size` splits into `send_fpp` and
`receive_fpp` — genuinely different things (`:311` configures receive, `:650`
send). **No serializer change is needed**: `serialize_samples_into` derives
`header.buffer_size = samples.len()/send_channels` (`protocol.rs:401`), so send
fpp is implicit in how many samples a transport reads per packet, and both
transports already use `AudioBufferConfig.buffer_size` as their chunk size
(`webrtc.rs:383`, `webtransport_worker.rs:467`).

Add `fn max_packet_bytes(&self) -> Option<usize> { None }` to the `Transport`
trait (`transport.rs`, near `:225`).

- **WebTransport — discovered.** In `worker_connect` right after `ready()`
  resolves (`webtransport_worker.rs:370-382`), read
  `transport.datagrams().max_datagram_size()` (available in the pinned web-sys,
  **no Cargo change**) and `fit_fpp`. A narrow `WorkerState::set_send_fpp`
  resizes only the three scratch buffers (`:262-269`), deliberately *not*
  re-running `configure()`. Post `{type:"fpp", …}` **before**
  `post_message_to_main("connected")` (`:827`); since `connect_worker` awaits
  that (`webtransport.rs:486`), the value is guaranteed present by
  `finalize_connection`. `webtransport_worker.js` needs no change.
- **WebRTC — conservative rail now, probe as diagnostic.** The fitting input is
  **not** `maxMessageSize`: browsers report ≥64 KiB, which would accept packets
  that fragment across many SCTP chunks, and the channel is `ordered:false,
  maxRetransmits:0` (`webrtc.rs:236-246`, `:1039-1044`), so fragmentation
  multiplies loss. The fitting limit is therefore the same fragmentation-free
  MTU floor as QUIC, known synchronously. Separately, inside the existing
  `on_open` closure (`webrtc.rs:1060-1067`) — which fires exactly when the SCTP
  association is up, so `pc.sctp` is non-null — do the `js_sys::Reflect` probe
  (pattern from `devices.rs:145-158`, `engine.rs:465-483`) and warn loudly if
  the browser's limit is below our chosen packet. Because fitting no longer
  depends on it, arriving late is harmless.

**Peer-fpp adoption:** `deliver_received_packet` (`transport.rs:40-41`) already
has `header.buffer_size` and simply doesn't pass it — add it. Rename
`adopt_channel_count` → `adopt_stream_format(channels, fpp)`, extending it to
recompute `fpp`, `packets_in_past`, `up_to_now`, `beyond_now`, `burg`, fade
ramps, both `TimingStats` and per-channel states — **field-wise, never
`*self = …`**, and strictly before the `Release` store at `regulator.rs:870`, so
the existing soundness argument at `:893-904` carries over verbatim. Do not
claim the `reset()` race at `:906-914` is closed; it is not.

**Ring-buffer invariant:** `RING_BUFFER_SIZE = 4096` (`ring_buffer.rs:14`) is a
hard ceiling on `fpp × send_channels`, not a sizing nit — `write` rejects
oversized writes (`:141-146`) while both send paths wait for
`available >= fpp*channels`, so any excess is a **permanent silent stall**.
Raise it to hold ≥2 packets at the wire ceiling and add a `debug_assert!` in
`set_audio_buffers`.

### Phase 5 — Device↔network separation
*Delivers: audio-without-network, and network-without-audio on **both**
transports. Independent of 2-4; easier after 2.*

- `start_capture` (`session.rs:530`) becomes public `startAudio`; `stop_capture`
  becomes `stopAudio` **minus** the unconditional `set_state(Idle)` at `:597`.
- `finalize_connection` (`:757-776`) keeps only: register connection, mark
  network liveness, start the right clock. `connect_to_studio` keeps its exact
  signature (Demo.tsx is out of scope) and becomes `start_audio(...)` then
  `connect(...)`.
- **Replace `self as *mut WebTripSession` (`session.rs:828`)** with a shared
  `tick_targets: Rc<RefCell<Vec<Rc<RefCell<Box<dyn Transport>>>>>>`. Deletes the
  self-pointer and its `unsafe` entirely, generalises to N with no closure
  rebuild, and puts `Rc`/`RefCell` exactly where AGENTS.md permits it — at the
  `web_sys` callback boundary, main-thread-confined.
- **State split:** `SessionState` becomes the *network* state; add
  `AudioState { Stopped, Running }`. Session-level state becomes a pure
  `aggregate_network_state(&[NetworkState]) -> SessionState` — natively
  testable, and the seam where 1→N generalises.
- **The WebRTC tick-clock asymmetry, addressed not papered over.** Its tick is
  driven by `Atomics.waitAsync` on the ring's has-data flag, set only by
  `RingBuffer::write` from the worklet — so no worklet means no ticks and
  WebRTC cannot *receive* either; packets pile up in `receive_queue`
  (`webrtc.rs:460`). Introduce a session-owned
  `TickClock { None, Atomics(AudioCallbackLoop), Interval(IntervalTickLoop) }`
  with `ensure_running_for(audio_running, any_connection_live)`.
  `IntervalTickLoop` is a small sibling of `AudioCallbackLoop`
  (`audio_callback_loop.rs:142`) with the same stop-on-Drop discipline.

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
  rebuilds the node — machinery `engine.rs:287-335` and `teardown_worklet_node`
  (`:351`) already provide and exercise for output-device changes.
- `SessionStats` splits into `ConnectionStats` + aggregate;
  `build_session_stats` (`session.rs:202`) splits into a pure
  `build_connection_stats` plus a trivial aggregate, and the existing
  `build_session_stats_maps_all_fields` test (`:1082`) ports onto the former.

**Non-negotiable sequencing:** 0 first (so later phases place new files
correctly); 2 before 3 (the reader feeds the mixer's `net_in`); 3 before 4
(adopting a peer fpp without the frame adapter breaks playback immediately —
`remote_interleaved` would be `fpp*channels` while the mixer expects
`frames*channels`); 3b after 3 (it changes the frame↔packet arithmetic that
phase establishes). Phase 3b is independent of 4 in either order. Phase 1 is
independent and can ship at any point.

## Testing

Per AGENTS.md: extend the existing matrices rather than adding point tests —
`devices.rs` channel-resolution boundaries, `worklet.rs`'s five
`render_clamps_*`, `processor.rs`'s two mapping matrices, `regulator.rs`'s
adoption/rejection tests, `session.rs`'s transport-state matrix. Tests pinning
the literal 8 that must retarget: `session.rs:969-973`,
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

## Verification

1. `npm run check` — first gate; catches missed renames and any const
   assertions. Never bare `cargo check` (`web_sys` unstable gating).
2. `npm run test` — native units: `session.rs`, `protocol.rs`, `params.rs`,
   `devices.rs`, `regulator.rs`, `processor.rs`, plus the new `dsp`/`mixer`/
   `frame_reader` tests.
3. `npm run test:wasm` — the **only** step exercising `worklet.js`, so the only
   proof the Rust→JS handoff works. Watch `buffer_setters_feed_capture_path`
   (`engine.rs:769`) and `worklet_port_tracks_capture_lifecycle` (`:706`).
4. `npm run build:wasm` — release build; `debug_assert!`s do not run there, so
   the `worklet.rs:62-63` clamps are the real safety net.
5. **Manual, needs hardware:** attach a >8-channel interface and confirm
   `getInputDeviceChannels` reports its true width. Chrome's fake device reports
   a narrow count, so no automated test can distinguish "ceiling raised" from
   "ceiling still 8" — and that is the user's actual goal.

## Follow-ups (explicitly not in this work)

- **Burg AR order is 255** because `BurgAlgorithm::new` sets `order = size - 1`
  (`regulator.rs:149`) with `size = up_to_now`, pinned at 256 by
  `HIST * (HIST_FPP/fpp)`. Typical PLC uses 16-64. This is what actually gates
  wide channel counts and low fpp, and it affects today's 8-channel path too.
  Needs measurement and listening tests.
- **Lower fpp does not hurt concealment quality** — the training window is in
  frames and stays at 256, so smaller packets mean smaller gaps against
  undiminished history. It costs ~4.5× more Burg CPU/sec at a constant
  per-packet loss rate. CPU and quality move in opposite directions.
- `webrtc.rs:1126` allocates `16 + samples*4` while
  `webtransport_worker.rs:267` uses `HEADER_SIZE + samples*2` for the same
  16-bit format; the WebRTC one also hardcodes `16` instead of `HEADER_SIZE`.
- `tests/integration/run.mjs:159` maps `mock: m.TransportType.Mock`, a variant
  removed in `260231e`; `ttByName.mock` is `undefined` so a `mock` run throws.

## Critical files

Paths below are post-Phase-0. Pre-move, all of `src/audio/`.

`src/audio/processor.rs`, `src/audio/engine.rs`, `src/audio/worklet.rs`,
`src/audio/worklet.js`, `src/stream/regulator.rs`, `src/stream/ring_buffer.rs`,
`src/network/transport.rs`, `src/network/protocol.rs`,
`src/network/webtransport_worker.rs`, `src/network/webrtc.rs`, `src/session.rs`.

New: `src/limits.rs` (both ceilings — shared by `audio/` and `network/`, so it
sits above both), `src/audio/dsp.rs`, `src/audio/mixer.rs`,
`src/stream/frame_reader.rs`.

Docs to update: `docs/CHANNEL_MODEL.md` (asserts the 8-channel limit at
`:432-441`; `:179-182` and `:428-431` describe the settings-only capture
behaviour that Phase 0 fixes), `docs/WASM_TESTING.md:491`.
