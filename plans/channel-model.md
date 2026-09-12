# Channel model: capture width, peer negotiation, playback mapping

## Context

The request started as "use the first packet to determine the peer's channel count, the same way we
use it for frames-per-packet." Tracing it turned up two things worth stating before the plan.

**There is no first-packet fpp negotiation to mirror.** `Regulator::with_params`
(`src/audio/regulator.rs:650`) fixes fpp *and* channels at construction; `configure()` (`:746`) is
`*self = Self::with_params(...)`. Both numbers come from the local session, never from the wire.

**Today one scalar drives everything, and it is the wrong scalar.** The demo's "Stereo" button
(`website/src/pages/demo/Demo.tsx:309-313`, default on) calls `session.setChannels(2|1)`
(`src/session.rs:383-396`), which sets `AudioParams::output_channels`, reconfigures the regulator's
*receive* width, and — only if clicked before connect — sizes the *send* packets via
`AudioBufferConfig` (`session.rs:574-579`). It does not affect capture at all. The node graph is
hard-mono in both directions:

- `worklet.rs:64-65` — `channelCount: 1`, `channelCountMode: Explicit`, so the `source → worklet`
  connection (`engine.rs:208`) speaker-folds a stereo interface to `0.5*(L+R)` before Rust sees it.
- `worklet.js:29,32` — reads `inputs[0][0]`, writes `outputs[0][0]`, one 128-sample mono slice each way.
- `worklet.rs:68` — `outputChannelCount: [1]`; the browser then up-mixes that one channel to the
  destination's default 2.
- `getUserMedia` requests no `channelCount` at all (`engine.rs:51-79`).

So "Stereo" means *send two identical copies of a mono mic* (`processor.rs:226-241`), which the
receive side averages straight back to mono (`processor.rs:281-286`). It doubles wire bandwidth for
zero information.

Since commit 917eeca the regulator rejects any packet whose `samples.len() != fpp * channels`
(`regulator.rs:771`), and **both call sites discard the verdict** (`webtransport_worker.rs:158`,
`webrtc.rs:457`). A peer whose channel count differs from the local toggle therefore produces
permanent silence with no stat and no log. That is a live bug, not a hypothetical.

**Target behavior** (as specified):

- *Capture* is a local choice bounded by the input device: 1 input channel → force Mono and disable
  the button; ≥2 → Mono captures channel 1 only, Stereo captures channels 1-2.
- *Playback* is a function of the peer's channel count `P` and the output device's channel count `N`:
  - `N == 1` → mix all `P` peer channels down into the single output channel
  - `P == 1`, `N >= 2` → copy to the first two output channels
  - `P == 2`, `N > 2` → map to the first two output channels
  - otherwise → 1:1 into the first `min(P, N)`, dropping any peer channels beyond `N`
- The local monitor mix uses the same mapping from capture channels to output channels.

Three phases, in dependency order. Phase 1 stands alone and fixes the silent-audio bug.

---

## Phase 1 — The regulator adopts the peer's channel count

### `src/audio/regulator.rs`

Replace `push`'s `bool` with a `#[must_use]` verdict, and take the peer's count explicitly:

```rust
#[must_use]
pub enum PushOutcome {
    Stored,
    UnsupportedChannelCount { got: usize },
    ChannelCountChanged { adopted: usize, got: usize },
    WrongPacketSize { expected: usize, got: usize },
}

pub fn push(&mut self, sequence: u16, channels: usize, samples: &[f32]) -> PushOutcome
fn push_internal(&mut self, seq: u16, channels: usize, samples: &[f32], now_ms: f64) -> PushOutcome
fn adopt_channel_count(&mut self, channels: usize)   // private
```

Explicit `channels`, not inferred from `samples.len() / fpp`: 4ch×64 and 2ch×128 are both 256
samples, and inference would silently adopt the wrong stride. Not `&PacketHeader` either — the
regulator is the one `audio/` module with no `crate::` imports, and a native host will reuse it
against different framing.

`push_internal` body order, which *is* the soundness argument:

1. reject `channels == 0 || channels > MAX_CHANNELS`
2. reject `samples.len() != channels * self.fpp` (this also validates the peer's `buffer_size`
   against local fpp — see below)
3. if `last_seq_in == SEQ_NONE` and `channels != self.num_channels` → `adopt_channel_count(channels)`
4. else if `channels != self.num_channels` → reject `ChannelCountChanged`
5. existing slot store, then the existing `Release` store of `last_seq_in`

`adopt_channel_count` rebuilds only channel-derived state: `num_channels`, `samples_per_packet`,
`channels: Vec<ChannelState>` (via `ChannelState::new(fpp, up_to_now, packets_in_past)`), and each
`slots[i].data` resized **and zeroed** — the reasoning in `reset()`'s comment at `:1310` applies
verbatim. Untouched: `burg`, `up_to_now`/`beyond_now`, the fade ramps, `push_stats`/`pull_stats`,
and the whole tolerance/auto-headroom policy. That last point is why this is not `configure()`:
`configure()` is `*self = with_params(...)`, which would reset the jitter policy on the first packet
of every stream.

**Concurrency.** Adoption resizes 4096 slot `Vec`s on the network thread while the audio thread may
be in `pop`. It is safe *only* because it happens strictly before the `Release` store of
`last_seq_in`, and `pop_internal`'s `SEQ_NONE` early return (`:837-840`) touches none of the rebuilt
state — it reads `start_time_ms`/`tolerance_ms`, fills zeros, and returns. Anything past that early
return is reached only after an `Acquire` load observed a published write pointer. Document that
edge on `adopt_channel_count`. Document the residual hazard too, in the `SharedPtr::as_mut` house
style (`src/audio/shared_ptr.rs:104-127`): a `pop` still in the *previous* stream's real path when
`reset()` re-arms `SEQ_NONE` and a new peer adopts a different count would read freed memory.
`WebTripSession::disconnect` (`session.rs:704-736`) orders `close().await` → `stop_capture()` →
`reset()`, but the worklet's final render call is not synchronously joinable. Point at the same
tracked concurrency redesign rather than claiming soundness the code does not have.

Also: `packets_rejected: u64` on the regulator and on `RegulatorStats`, surfaced as
`regulator_packets_rejected` through `build_session_stats` (`session.rs:193-227`).

**Bundled one-line fix:** `latency_ms()` (`:1359-1363`) multiplies by `samples_per_packet` where it
means `fpp`, so it reports 2× at stereo. It is inside the touched surface and gets materially more
confusing once the width is peer-driven. `regulator.rs:1893-1896` reproduces the same formula and
must be corrected with it.

### `src/audio/protocol.rs`

`pub const MAX_CHANNELS: usize = 8;` — the wire contract owns the limit. Replaces the hardcoded `8`
at `protocol.rs:282,288`, `session.rs:114`, and `params.rs:66`.

### `src/audio/transport.rs` — one shared receive helper

```rust
pub(crate) fn deliver_received_packet(
    regulator: &mut Regulator, data: &[u8], samples: &mut Vec<f32>,
) -> Result<PushOutcome, ProtocolError>
```

Deserializes via `AudioPacket::deserialize_into`, calls
`regulator.push(header.sequence_number, header.num_incoming_channels as usize, samples)`, and emits
a throttled `console::warn` on any non-`Stored` outcome — reusing the `HIGH_RATE_WARN_INTERVAL`
pattern from `webtransport_worker.rs:110`, moved here so it is defined once. This file already hosts
the cross-transport helpers (`log_audio_buffers_set`, `transport_state_str`).

Call sites: delete `deserialize_datagram` (`webtransport_worker.rs:84-87`, it existed only to throw
the header away); `webrtc.rs:454-462` switches from `AudioPacket::deserialize` — which allocates a
`Vec<f32>` per packet on the network path — to the shared helper with a reusable `received_samples`
field, mirroring the existing `audio_to_send_buffer`.

### `src/session.rs`

`set_channels` stops calling `network_to_local_buffer.configure(...)`. The receive width is the
peer's to declare, and dropping the call also removes the `-500.0 → -1.0` headroom-policy clobber
(constructor at `:296` selects variable headroom; `set_channels` at `:394` silently downgrades it and
never restores it). It also stops being a mid-session footgun: the button has no `disabled` guard,
so a click while connected currently resets the regulator under the audio thread *and* leaves the
transport's snapshotted `AudioBufferConfig.channels` stale, after which producer and consumer
disagree about chunk size across the channel-agnostic `RingBuffer`. Reject `set_channels` unless
`state == Idle` and say so in the doc comment (which already claims "Must be called before
connecting").

### `src/audio/processor.rs`

`receive_from_network` derives the pop width from `regulator.channels()` and `regulator.fpp()`
instead of `params.get_output_channels()`, and downmixes through a new pure free function beside
`apply_gain`/`compute_rms`:

```rust
pub(crate) fn downmix_to_mono(interleaved: &[f32], channels: usize, mono_out: &mut [f32])
```

Output stays mono in Phase 1. Rename `stereo_receive_buffer` → `remote_interleaved` and preallocate
it at `128 * MAX_CHANNELS` in both constructors so the resize never allocates on the render thread.
This also fixes a bug that exists today: the `>= 2` branch hardcodes `*2` strides, so a 3+ channel
regulator is popped into a half-sized buffer.

### Phase 1 tests

- Extend `test_push_rejects_packets_that_are_not_exactly_one_packet_long` (`:2226`) with the new
  rejection axes rather than adding point tests: `channels = 0` and `MAX_CHANNELS + 1`; a
  post-adoption packet claiming a different count; and the ambiguity case that pins the
  explicit-count decision (adopted 2ch at fpp 128, then `channels = 4` with 256 samples → rejected).
- New `test_first_packet_adopts_peer_channel_count_and_plays_back_at_that_stride`: `with_params(2,
  32, 48_000, 5.0)`, push a 1-channel ramp, assert `Stored`, pop into an `fpp * 1` buffer and assert
  the ramp returns. Only passes if the deinterleave stride, the `ChannelState` count, and the slot
  length all moved together. Assert `fpp()`/`tolerance_ms()` are unchanged — that is what
  distinguishes `adopt_channel_count` from `configure`.
- Extend `test_reset_clears_state_after_active_stream` (`:1698`): run at 2ch, `reset()`, then adopt
  1ch — pins that reconnect re-arms adoption.
- `handle_datagram_valid_pushes_to_regulator_and_counts` (`webtransport_worker.rs:904`): change the
  fixture to `with_params(2, 128, ...)` and keep the mono datagram, so it fails on exactly the
  production bug. Add a follow-up datagram with a different count asserting `packets_rejected` moved
  and `last_seq_received` did not.
- New `downmix_to_mono` matrix over 1..=8 channels with per-channel-distinguishable input.
- No new WebRTC test — its receive branch becomes one call to the helper, covered once.
- ~20 `push_internal(...)` calls in `regulator.rs` tests gain a `channels` argument matching the
  regulator they were built with. `plant_packet` (`:1384`) needs no signature change.

---

## Phase 2 — Multichannel playback

### Worklet ABI: shared-memory scratch buffers

`ProcessorHandle` exposes pointers to preallocated planar scratch buffers plus a render entry point:

```rust
pub fn input_ptr(&self) -> usize          // frames * MAX_CHANNELS
pub fn output_ptr(&self) -> usize         // frames * MAX_CHANNELS
pub fn render(&mut self, in_channels: usize, out_channels: usize, frames: usize) -> bool
```

`worklet.js` builds `Float32Array` views over `memory.buffer` once, `.set()`s each
`inputs[0][ch]` into its plane, calls `render`, then `.set()`s each output plane into
`outputs[0][ch]`. Views are rebuilt when `memory.buffer` detaches after growth — the file already
does exactly that for its `Int32Array` (`worklet.js:38-40`), so the same guard applies. This removes
the per-callback copy wasm-bindgen does today for the `&[f32]` argument.

Planar, not interleaved, in the scratch buffers: it matches `.set()`-per-channel on the JS side and
the regulator's per-channel state. Interleaving stays where the wire needs it.

### Node and destination configuration

- `create_worklet_node_with_flag` (`worklet.rs:56`) takes `input_channels` / `output_channels`; sets
  `channelCount`, `ChannelCountMode::Explicit`, and `ChannelInterpretation::Discrete` so channels are
  not speaker-folded, and `outputChannelCount: [output_channels]`.
- `engine.rs` near `:207-209`: read `ctx.destination().max_channel_count()`, then
  `destination.set_channel_count(n)` with explicit/discrete before connecting.
- `outputChannelCount` is construction-time only, so a change means **recreating the worklet node**.
  `setSinkId` (`engine.rs:320-338`) can change `maxChannelCount`, so `route_output_sink` must
  re-read it and rebuild the node when it differs.
- `Cargo.toml` web-sys features: add `ChannelInterpretation`.

### The mapping itself

One pure function in `processor.rs`, used for both the peer stream and the monitor mix:

```rust
pub(crate) fn map_to_output(src: &[f32], src_channels: usize, out: &mut [f32], out_channels: usize)
```

- `out_channels == 1` → average all `src_channels` into it
- `src_channels == 1`, `out_channels >= 2` → copy to channels 0 and 1; the rest silent
- `src_channels == 2`, `out_channels > 2` → channels 0 and 1; the rest silent
- otherwise → 1:1 for `min(src_channels, out_channels)`; extra peer channels dropped, extra output
  channels silent

`process` gains a channel dimension: the mix loop (`processor.rs:178-190`) runs per output channel,
mixing the mapped peer stream with the mapped monitor, and the resize at `:148-151` keys off
`frames * channels` rather than `input.len()`. The level meter takes the max RMS across capture
channels (document the choice). `downmix_to_mono` from Phase 1 becomes the `out_channels == 1` arm
of `map_to_output` — one implementation, not two.

`AudioParams::output_channels` is renamed to `capture_channels`: after this phase it governs only the
send path. Note in passing that `auto_gain_control`/`echo_cancellation`/`noise_suppression` on
`AudioParams` (`params.rs:36-38`) plus their six wasm-bindgen accessors are dead code — the real
values travel via `PendingCaptureParams`. Worth deleting while renaming the neighbouring field.

### Phase 2 tests

A `map_to_output` matrix over `src_channels × out_channels` in `1..=8` with per-channel-distinct
input, asserting each rule branch including the drop and the silence. That single table is what
would have caught today's 3+-channel bug. No test of `receive_from_network` itself — it becomes a
thin wrapper. The worklet ABI change is browser-only: one `npm run test:wasm` case driving `render`
through the scratch buffers and asserting the output planes.

---

## Phase 3 — Real multichannel capture

- `AudioConstraints` (`engine.rs:23-31`) gains `channel_count`; `to_js` emits
  `channelCount: { ideal: n }`. Chrome forces mono when `echoCancellation` is on, so stereo capture
  requires the processing toggles off — the UI must say so rather than silently yielding mono.
- After `getUserMedia`, read `track.getSettings().channelCount` for what was actually granted (the
  true capture width) and `getCapabilities().channelCount.max` for the UI gate, falling back to
  settings where capabilities are unavailable. `Cargo.toml`: add `MediaTrackSettings`,
  `MediaTrackCapabilities`, `MediaTrackConstraints`.
- Worklet input side becomes `channelCount = capture_channels`, explicit/discrete, so channel 2 is
  no longer folded into channel 1.
- `send_local_to_network` (`processor.rs:213-246`) interleaves the real N capture channels instead of
  duplicating one — the `stereo_buffer` field and its "mono duplicated to both channels" comment go
  away.
- `WebTripSession` exposes the granted and maximum input channel counts to JS.
- `Demo.tsx`: `ToggleButton` (`:68-85`) has no `disabled` prop today — add one, gate the Stereo
  button on max input channels, and force Mono when it is 1.

---

## Verification

Never bare `cargo` — the `web_sys_unstable_apis` gate makes it fail.

- `npm run check` after each phase's signature changes ripple.
- `npm run test` for the native unit tests (regulator, processor mapping tables, worker, session).
- `npm run test:wasm` for the worklet ABI and any `SessionStats` surface change.
- `npm run serve` for the end-to-end checks that the tests cannot reach:
  - *Phase 1:* set the local toggle to Mono against a stereo peer (and the reverse). Audio must play
    and `regulator_packets_rejected` must stay at 0 — both combinations are silent today.
  - *Phase 2:* against a multichannel output device, confirm a mono peer lands on the first two
    channels and a stereo peer maps 1:1; switch output devices mid-session and confirm the node is
    rebuilt at the new width.
  - *Phase 3:* with a 1-in interface the Stereo button is disabled and forced to Mono; with a 2-in
    interface, Stereo carries two genuinely different channels (verify L and R differ on the wire,
    not just in the meter).
