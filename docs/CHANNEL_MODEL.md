# WebTrip Channel Model

This document explains how WebTrip decides, discovers, and reconciles audio
**channel counts** — how many channels the microphone actually captures, how
many the network wire carries, how many the peer sends, and how many the
speakers/headphones can play. It covers startup and channel discovery, and
the complete flow of audio from the input device through the network to the
output device.

For the broader threading model (why there's an AudioWorklet thread, a main
thread, and sometimes a WebTransport worker thread, and how they share
memory) see [ARCHITECTURE.md](ARCHITECTURE.md) — this document assumes that
context and focuses specifically on channels. For the original design
rationale and phased implementation history, see
[`plans/channel-model.md`](../plans/channel-model.md) — but treat that as
historical background, not current documentation; per
[`plans/AGENTS.md`](../plans/AGENTS.md) the `plans/` directory isn't
guaranteed to reflect the code as it stands today. This document is.

---

## Table of Contents

1. [The Core Mental Model: Four Channel Counts](#the-core-mental-model-four-channel-counts)
2. [Startup and Channel Discovery](#startup-and-channel-discovery)
3. [Send Path: Microphone → Network](#send-path-microphone--network)
4. [Receive Path: Network → Speakers](#receive-path-network--speakers)
5. [The Channel-Mapping Policy](#the-channel-mapping-policy)
6. [Key Invariants and Gotchas](#key-invariants-and-gotchas)
7. [File Map](#file-map)

---

## The Core Mental Model: Four Channel Counts

The single most important thing to understand about this codebase is that
**"how many channels" means four different, independently-tracked things**,
and confusing them is the source of most bugs in this area. None of them are
required to be equal to each other, and the code is written to tolerate all
of them disagreeing at once.

| # | What it means | Where it lives | When it's set |
|---|---|---|---|
| 1 | **Requested / wire width** — what the app *asks* the browser for, and the fixed channel count the network wire is framed at for the whole connection | `WebTripSession.channels` (`session.rs`) / `AudioParams::capture_channels` (`params.rs`) | At session construction (`2`, "default to stereo"), or by `setChannels()` — **only while `Idle`** |
| 2 | **Granted capture width** — what the browser actually delivers from the microphone this callback | `in_channels` parameter threaded through the worklet ABI and `AudioProcessor::process` | Every render callback (~2.7ms), from what the browser's `AudioWorkletNode` input actually contains |
| 3 | **Peer's send width** — how many channels the *other end* of the connection is sending | `Regulator`'s adopted channel count (`regulator.channels()`) | From the peer's first received packet (see Phase 1 of the plan; `Regulator::push`) |
| 4 | **Output width** — how many channels the local speakers/headphones support | `AudioEngine::output_channels()` / `WebTripSession::getOutputChannels()` | Synchronously, from `AudioContext.destination.maxChannelCount`, whenever the worklet node is (re)built |

Why aren't these unified into one number? Because they come from genuinely
different sources with different timing:

- **#1 is a local UI choice**, fixed for the lifetime of a connection because
  the network transports (WebRTC data channel, WebTransport datagrams) frame
  every packet at a single, agreed-upon width — there's no per-packet
  channel-count renegotiation.
- **#2 is discovered asynchronously** via `getUserMedia`, and the browser is
  free to grant fewer channels than requested (a mono-only mic, or Chrome
  forcing mono when echo cancellation is on) — see
  [Startup and Channel Discovery](#startup-and-channel-discovery).
- **#3 is controlled by the *other* participant**, not this session at all —
  see `plans/channel-model.md`'s Phase 1 for how the jitter buffer adopts it.
- **#4 is a property of the local audio hardware**, read synchronously (no
  negotiation, no permission prompt) the moment an `AudioContext` exists.

Two pure functions exist specifically to reconcile mismatches between these:
[`map_to_output`](#the-channel-mapping-policy) (planar output, used for
peer-stream and monitor mixing) and
[`conform_interleaved_channels`](#the-channel-mapping-policy) (interleaved,
used for the outbound wire). Both implement the *same* mapping policy; they
differ only in memory layout.

---

## Startup and Channel Discovery

### Before connecting

- `WebTripSession::new()` defaults `channels = 2` and syncs it to
  `AudioParams::capture_channels` (`session.rs`).
- `setChannels(n)` lets the UI change the requested/wire width (#1 above) —
  but only while `state == Idle`. Calling it while connected just logs a
  warning and no-ops. This is deliberate: changing it mid-connection would
  desync the `RingBuffer` producer (the worklet, writing at the old width)
  from the consumer (the transport's send loop, framing packets at
  whatever `AudioBufferConfig.channels` it was handed at connect time).
- The demo's Stereo toggle (`Demo.tsx`) is the UI for this. Nothing about
  microphone capability is known yet at this point — no `getUserMedia` call
  has happened, so the toggle is enabled by default and simply reflects the
  user's *preference*, not a confirmed capability.

### `connectToStudio()`

`WebTripSession::connect_to_studio` snapshots the current requested width
into `AudioBufferConfig.channels` (`session.rs`) before building the
transport. **This snapshot is the wire's channel count for the entire
connection** — nothing after this point changes it, even if capture later
turns out narrower.

### `AudioEngine::start_capture` — the discovery sequence

This is where channels #1 and #2 actually meet the browser (`engine.rs`):

1. Read the requested width from `AudioParams::get_capture_channels()`.
2. Call `getUserMedia` with an **`ideal`** (not `exact`) `channelCount`
   constraint (`AudioConstraints::to_js`) — `ideal` means the browser does
   its best but won't fail the whole request if it can't match exactly.
3. Once the stream resolves, read the **granted** width from
   `MediaStreamTrack.getSettings().channelCount`, and the device's **max**
   capability from `.getCapabilities().channelCount.max` — falling back to
   the granted value when capabilities aren't available. Both are clamped
   to `[1, MAX_CHANNELS]` (`MAX_CHANNELS = 8`, `protocol.rs`).
4. Store both as `Option<u32>` fields on `AudioEngine`
   (`granted_input_channels`, `max_input_channels`) — **`None` until this
   step actually runs**, not merely "an `AudioEngine` exists." This
   distinction matters: `AudioEngine::create_with_network` (step before
   this) returns before any of the above happens, so there's a real window,
   mid-`start_capture`, where an engine exists but discovery hasn't
   completed yet. `Option` (rather than a `0`-means-unknown sentinel) is
   what makes that window safely distinguishable from a real answer — a
   sentinel value silently collided with the engine's construction default
   here in an earlier version of this code.
5. Build the `AudioWorkletNode` at the **granted** input width (not the
   requested width) — `build_and_connect_worklet_node(granted, output_channels)`
   — and read the **output** width synchronously via
   `configure_destination()` (`AudioContext.destination.maxChannelCount`,
   capped to `MAX_CHANNELS`) in the same step.

`WebTripSession` exposes all three discovered values to JavaScript,
symmetric to each other, all `Option<u32>` (`number | undefined` in
TypeScript) with the same "`None`/`undefined` = not yet known" contract:

- `getGrantedInputChannels()`
- `getMaxInputChannels()`
- `getOutputChannels()` — note there's no "requested vs. granted" pair for
  output; there's no negotiation on that side, so one value is all there is.

### The Stereo toggle's gate

The demo disables the Stereo toggle and forces its display to "Mono" once
`getMaxInputChannels()` reports `1` — but this can only happen **after**
`connectToStudio()` resolves (discovery is part of `start_capture`, which
`connectToStudio` awaits internally). `Demo.tsx` reads the value right after
that promise resolves, not from the `sessionState === "connected"` React
effect alone — the session reports `Connected` *before* capture starts, so
relying on that transition alone would always see the "not yet known" value.

Critically, forcing the toggle to "Mono" is **display-only**. It does not
call `setChannels(1)`, both because that call would be rejected (the session
is no longer `Idle`) and because it doesn't need to: the send path already
adapts to the real captured width every callback (see below). What it does
*not* automatically fix is the requested/wire width (#1) for a *future*
connection — see the next section.

### `disconnect()` — narrowing for next time

Without any correction, a disconnect leaves `AudioParams::capture_channels`
exactly where it was. If a mono-only device was discovered during that
connection, the *next* `connectToStudio()` would request `channelCount:
{ideal: 2}` again and the wire would again be framed at 2 channels — wasting
bandwidth via the duplication described in the next section, on every single
future connection to that device.

`WebTripSession::disconnect()` fixes this: it captures the just-discovered
`max_input_channels` before tearing down the `AudioEngine`, and — once back
in `Idle` — calls the ordinary `set_channels()` if the device's max is
*narrower* than the currently-configured width. This is one-directional: it
**never widens**. A user's explicit narrower choice (e.g. manually picking
Mono) is never silently overridden back up just because a *different* device
turns out to support more. The decision itself is a pure, unit-tested
function — `narrowed_channel_count` in `session.rs`.

---

## Send Path: Microphone → Network

```
 getUserMedia stream                AudioWorklet thread                  Main thread / Worker
        │                                    │                                    │
        │ ① live MediaStreamTrack(s)         │                                    │
        ├───────────────────────────────────>│                                    │
        │                            worklet.js process():                        │
        │                       copies EVERY input channel                        │
        │                       plane into inputViews[ch]                         │
        │                                    │                                    │
        │                            ② ProcessorHandle::render(in_channels, ...)   │
        │                               → AudioProcessor::process(...)             │
        │                                    │                                    │
        │                            ③ apply_gain over the planar                  │
        │                               gained_buffer (in_channels × frames)      │
        │                                    │                                    │
        │                            ④ level meter: max_channel_rms                │
        │                               (loudest channel, not the average)        │
        │                                    │                                    │
        │                            ⑤ interleave_planar → captured_interleaved   │
        │                               (ONE conversion; feeds both ⑥ and the     │
        │                                monitor mix on the receive side)         │
        │                                    │                                    │
        │                            ⑥ send_local_to_network:                     │
        │                               conform_interleaved_channels if           │
        │                               in_channels != wire_channels, then        │
        │                               RingBuffer::write()                       │
        │                                    │                                    │
        │                                    │      (shared WASM memory)          │
        │                                    │ ──────────────────────────────────>│
        │                                    │                                    │
        │                                    │                         ⑦ tick() (WebRTC) or
        │                                    │                            send_loop (WebTransport)
        │                                    │                            reads RingBuffer,
        │                                    │                            frames a PacketHeader
        │                                    │                            with num_incoming_channels
        │                                    │                            = wire_channels (fixed),
        │                                    │                            sends over the network
```

Key points:

- **Step ①-⑤ happen on the AudioWorklet thread**, once per ~2.7ms render
  quantum (`RENDER_QUANTUM_FRAMES = 128` samples, `worklet.rs`).
- **`in_channels` is the real, browser-granted width this callback** — not
  the requested width, not a fixed session-level constant. It flows all the
  way from `worklet.js`'s `inputs[0].length` through `ProcessorHandle::render`
  into `AudioProcessor::process(input, in_channels, output, out_channels)`
  (`processor.rs`).
- **Step ⑤ is the one and only planar→interleaved conversion per callback.**
  `captured_interleaved` is a shared buffer consumed by both the send path
  (below) and the local monitor mix (receive-path section) — this is
  intentional, not an oversight: writing it twice would duplicate work and
  risk the two consumers drifting out of sync.
- **Step ⑥ is where the wire's fixed width (#1) and the real captured width
  (#2) get reconciled**, if they differ. `send_local_to_network` reads
  `wire_channels = AudioParams::get_capture_channels()` — the value
  snapshotted at connect time, unrelated to what the mic is actually
  producing right now — and only pays the conform cost when
  `in_channels != wire_channels`. The conform itself follows the [same
  mapping policy](#the-channel-mapping-policy) as playback. This is how a
  mono-only mic with the Stereo toggle still requested ends up sending
  *duplicated* mono over the wire rather than corrupting the packet framing:
  correctness is guaranteed; the bandwidth cost of the mismatch is not
  eliminated by this step alone (see
  [`disconnect()`'s narrowing](#disconnect--narrowing-for-next-time) for how
  it's avoided on *future* connections).
- **Step ⑦ happens on the main thread (WebRTC) or a dedicated worker
  (WebTransport)** — see [ARCHITECTURE.md](ARCHITECTURE.md) for why network
  I/O can't happen directly on the AudioWorklet thread. The packet's
  `num_incoming_channels` header field (`protocol.rs`, `PacketHeader`,
  16-byte `HEADER_SIZE`) is always `wire_channels` — the fixed width from
  `AudioBufferConfig`, read directly off the config struct
  (`webrtc.rs::tick`, `AudioPacket::serialize_samples_into(..., buffers.channels, ...)`),
  never off what the worklet actually captured.

---

## Receive Path: Network → Speakers

```
   Network                    Main thread / Worker              AudioWorklet thread          Output device
      │                                │                                  │                        │
      ├─> ⑧ packet arrives             │                                  │                        │
      │      deliver_received_packet   │                                  │                        │
      │      (transport.rs)            │                                  │                        │
      │                                │                                  │                        │
      │            ⑨ Regulator::push(seq, header.num_incoming_channels,   │                        │
      │               samples) — ADOPTS the peer's channel count from     │                        │
      │               their first packet (Phase 1); rejects a mid-stream  │                        │
      │               change instead of silently reinterpreting it        │                        │
      │                                │                                  │                        │
      │                                │       (shared WASM memory)       │                        │
      │                                │ <────────────────────────────────│                        │
      │                                │                                  │                        │
      │                                │                        ⑩ AudioProcessor::process:          │
      │                                │                           regulator.channels()/fpp()       │
      │                                │                           → regulator.pop()                │
      │                                │                           into remote_interleaved           │
      │                                │                                  │                        │
      │                                │                        ⑪ map_to_output(remote_interleaved,  │
      │                                │                           remote_channels, remote_mapped,   │
      │                                │                           out_channels)                    │
      │                                │                                  │                        │
      │                                │                        ⑫ map_to_output(captured_interleaved,│
      │                                │                           in_channels, monitor_mapped,      │
      │                                │                           out_channels) — SAME policy,      │
      │                                │                           applied to local capture for the  │
      │                                │                           "hear yourself" monitor mix       │
      │                                │                                  │                        │
      │                                │                        ⑬ mix: remote_mapped +               │
      │                                │                           monitor_mapped × monitor_volume,  │
      │                                │                           × output_volume, clamp            │
      │                                │                                  │                        │
      │                                │                        ⑭ worklet.js copies each             │
      │                                │                           outputViews[ch] into              │
      │                                │                           outputs[0][ch]                    │
      │                                │                                  ├───────────────────────>│
```

Key points:

- **`out_channels` is the destination's width (#4 above)**, configured once
  by `configure_destination()`/`build_and_connect_worklet_node` and passed
  into every `process()` call — it does not vary per-callback the way
  `in_channels` does, since the output device's capability doesn't change
  mid-callback the way a peer's stream identity can.
- **Step ⑨ is Phase 1's regulator adoption**, not a Phase 3 concept — but it
  determines `remote_channels` for step ⑪, so it belongs in this diagram.
  See `plans/channel-model.md`'s Phase 1 section and `Regulator::push`'s doc
  comment for the full soundness argument (it involves rebuilding
  channel-derived jitter-buffer state on the network thread while the audio
  thread may concurrently be mid-`pop`).
- **Steps ⑪ and ⑫ use the exact same function** — `map_to_output` — applied
  to two different sources (the peer's decoded stream, and the local
  capture, for self-monitoring). This is why the mapping policy is
  documented once, centrally, rather than per call site — see next section.
- **The destination is configured `Explicit`/`Discrete`**
  (`ChannelCountMode`/`ChannelInterpretation`, set in both
  `configure_destination` and `create_worklet_node_with_flag`) specifically
  so the browser never does its own channel folding/upmixing — all channel
  mapping is `map_to_output`'s job, not the Web Audio graph's.

---

## The Channel-Mapping Policy

Two pure functions in `processor.rs` implement **the same four-branch
policy** for reconciling a source channel count against a destination
channel count — they differ only in memory layout (one planar, one
interleaved), because the two consumers need different layouts:

| Function | Source layout | Destination layout | Used for |
|---|---|---|---|
| `map_to_output(src, src_channels, out, out_channels)` | interleaved | **planar** (`out[ch*frames+frame]`) | Peer stream → output; local capture → monitor mix |
| `conform_interleaved_channels(src, src_channels, out, out_channels)` | interleaved | **interleaved** (`out[frame*out_channels+ch]`) | Real captured width → the wire's fixed width |

The policy itself (both functions implement it identically, just writing to
a different destination shape):

1. **`out_channels == 1`** → average every source channel down to one
   (delegates to `downmix_to_mono` in the planar case).
2. **`src_channels == 1 && out_channels >= 2`** → copy the single source
   channel to destination channels 0 and 1; everything past channel 1 is
   silent. (This is the "duplicate mono to stereo" behavior — now a special
   case of a general rule, not its own code path.)
3. **`src_channels == 2 && out_channels > 2`** → copy source channels 0/1 to
   destination channels 0/1; the rest silent.
4. **Otherwise** → 1:1 for `min(src_channels, out_channels)`; extra source
   channels are dropped, extra destination channels are silent.

There's also `interleave_planar` (planar → interleaved, the inverse layout
transform used once per callback to build `captured_interleaved`) and
`max_channel_rms` (the level meter: the *loudest* of the capture channels,
not their average — a single hot channel among quiet ones must still show on
the meter).

---

## Key Invariants and Gotchas

- **The wire's channel count never tracks the real captured width
  mid-connection.** It's fixed at `connectToStudio()` time
  (`AudioBufferConfig.channels`) and only ever changes via `disconnect()`'s
  narrowing, applied to the *next* connection. This was a deliberate,
  smaller-blast-radius choice over restructuring the connect sequence to
  discover capture width before configuring the transport — see
  `plans/channel-model.md`'s Phase 3 section for the reasoning.
- **`AudioParams::capture_channels` does double duty**: it's both the
  `channelCount: {ideal: n}` constraint sent to `getUserMedia` *and* the
  wire's fixed framing width. They happen to be the same field today; if a
  future change ever needs to request one width but frame packets at
  another, this field would need to split.
- **`None`/`undefined` genuinely means "not yet known," never "zero
  channels."** The discovery code in `engine.rs` clamps both granted and max
  to `[1, MAX_CHANNELS]` — a literal `0` reported by a browser would be
  floored to `1`, not surfaced as an error. A live `MediaStreamTrack`
  reporting zero channels isn't something current browsers produce in
  practice (an input with no channel capability fails `getUserMedia` outright
  rather than resolving with a degenerate track), so this hasn't needed
  further handling — but it means a real "device reports 0" scenario, if one
  ever surfaced, is currently silently coerced to `1` rather than reported.
- **Chrome forces mono capture when echo cancellation is on.** This is a
  browser policy, not a WebTrip bug — if stereo capture looks unavailable
  during testing, check the AGC/Echo/Noise toggles first.
- **`RENDER_QUANTUM_FRAMES` (128) and `MAX_CHANNELS` (8) bound the worklet's
  planar scratch buffers** (`ProcessorHandle::input_scratch`/`output_scratch`
  in `worklet.rs`, sized `RENDER_QUANTUM_FRAMES * MAX_CHANNELS`). Every
  per-callback buffer in `processor.rs` (`gained_buffer`,
  `captured_interleaved`, `wire_conformed`, `remote_interleaved`,
  `remote_mapped`, `monitor_mapped`) is preallocated at `128 * MAX_CHANNELS`
  so a widening resize never allocates on the real-time render thread.
- **A device with more than `MAX_CHANNELS` inputs is addressed at its first
  8 channels only** — both the input and output discovery paths clamp to
  `MAX_CHANNELS`, matching the wire protocol's own limit
  (`protocol.rs::MAX_CHANNELS`).

---

## File Map

| Concern | File |
|---|---|
| Session-level channel state, `setChannels`, discovery-exposing getters, disconnect narrowing | `src/session.rs` |
| Atomic shared `capture_channels` field | `src/audio/params.rs` |
| `getUserMedia` constraints, granted/max discovery, destination configuration, worklet node (re)building | `src/audio/engine.rs` |
| Worklet ABI (planar scratch buffers, `ProcessorCallback`/`ProcessorHandle::render`) | `src/audio/worklet.rs` |
| Worklet JS bridge (copies each channel plane in/out of WASM memory) | `src/audio/worklet.js` |
| Gain, metering, planar↔interleaved conversion, channel-mapping policy, send/receive wiring | `src/audio/processor.rs` |
| Wire protocol header (`num_incoming_channels`, `MAX_CHANNELS`) | `src/audio/protocol.rs` |
| Peer channel-count adoption (jitter buffer) | `src/audio/regulator.rs` |
| Per-transport packet framing at the fixed wire width | `src/audio/webrtc.rs`, `src/audio/webtransport_worker.rs` |
| Demo UI: Stereo toggle, gating, display sync | `website/src/pages/demo/Demo.tsx` |
