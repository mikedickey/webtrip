# Independent wire and device channel configuration

## Context

Turning off the demo's Stereo toggle makes the hub send us only 1 channel, even when the output device has 2.

**Root cause.** `AudioPacket::serialize_samples_into` (`src/audio/protocol.rs`) writes the one send count into both header bytes. Both fields are named from the sender's perspective (JackTrip `src/PacketHeader.cpp`, `DefaultHeader::fillHeaderCommonFromAudio`):
- `num_incoming_channels` (byte 14) is how many channels the sender receives from the network — the count it wants back.
- `num_outgoing_channels` (byte 15) is how many channels the sender transmits — this packet's payload width. `0` means "same as byte 14", `255` means no audio. JackTrip receivers size the payload by it (`JackTrip::getPeerNumOutgoingChannels`), and a hub configures a client from its first packet: byte 14 becomes what the hub sends, byte 15 what it expects to receive (`JackTripWorker::processPeerSettings`).

Because the two values are always equal, byte 15 always encodes "symmetric" (`0`). So send width and receive width are the same thing.

**Second latent bug.** `total_packet_size_out()` and `serialize_into` size and stride the payload by `num_outgoing_channels`, but the receiver (`deserialize_into`) and `deliver_received_packet` read it by `num_incoming_channels`. This only works while the two are equal.

**Hub bug (fixed in JackTrip, assumed fixed here).** JackTrip's `WebRtcDataProtocol::run` sized the sender's packet buffer with `getReceivePacketSizeInBytes()`, so an asymmetric client over WebRTC got mis-sized packets from the hub (and overflowed the hub's buffer when the client asked for more channels than it sends). The fix uses `getSendPacketSizeInBytes()` for the sender, as the WebTransport and UDP protocols do. WebTrip applies no WebRTC-specific workaround.

**What's wrong with the model.** One scalar (`WebTripSession.channels` / `AudioParams::capture_channels`, default 2) drives four things:
- the wire send width
- the implied receive width
- the `getUserMedia` channel request
- a post-disconnect "narrowing" step

Device channel counts are only discovered inside `AudioEngine::start_capture`, which runs after the transport connects.

**Target model:**

- **Library, wire.** Send channels (default 1) and receive channels (default 2) are independent. They are set before connecting, fixed for the connection, and have nothing to do with the device. A client can connect with no device probing at all, using the system default devices.
- **Library, device.** A client can list devices and query each device's channel count before any connection. It can set how many input and output device channels to use. Defaults are input 2 and output 2, each clamped to what the device really has. With the default of 1 send channel, an unconfigured client therefore mixes the first two input channels down to mono. When device and wire counts differ, the existing mapping rules apply: `conform_interleaved_channels` on send, `map_to_output` on receive and monitor.
- **Demo, devices.** Always list devices, and query channels for the selected input and output devices. Do this on first render, when the selection changes, and on `devicechange`.
  - Output device channels are 2 if the device supports at least 2, otherwise 1.
  - Receive channels are always 2.
- **Demo, capture.** A 1-channel input device forces a locked "Mono". Inputs with 2 or more channels get a cycling 3-state button:

  | Mode | Device channels captured | Channels sent |
  |---|---|---|
  | Mix to Mono (default) | 2, averaged | 1 |
  | Stereo | 2 | 2 |
  | Mono | first channel only | 1 |

  All settings are applied before connecting and locked from connect start until back to idle.

`plans/channel-model.md` is the earlier, historical plan for this area and stays as is.

## Approach

### 1. Wire protocol: `src/audio/protocol.rs`
- **`serialize_samples_into`.**
  - Signature: `(seq, ts, samples, send_channels: u8, receive_channels: u8, buffer)`.
  - Sets `num_incoming_channels = receive_channels` (byte 14) and `num_outgoing_channels = send_channels` (byte 15).
  - Builds the header from `PacketHeader::new` and overrides both fields, dropping the old `channels == 1` branching.
- **Payload sizing.** `total_packet_size_out()`, `serialize_into` and `deserialize_into` all use `num_outgoing_channels` as the payload width, and `deliver_received_packet` (`src/audio/transport.rs`) pushes `num_outgoing_channels` into the regulator. This matches JackTrip.
- **Doc comments.** The module docs, field docs and byte-15 encoding docs say:
  - incoming is what the sender receives — the count it wants back;
  - outgoing is what the sender transmits — the payload width;
  - wire `255` means the sender transmits no audio.

### 2. Transports
- **`AudioBufferConfig`** (`src/audio/transport.rs`): `channels: u8` is replaced by `send_channels` and `receive_channels`. `log_audio_buffers_set` takes the config and logs both.
- **WebRTC** (`src/audio/webrtc.rs`):
  - `audio_to_send_buffer`, `packet_serialize_buffer` and `samples_needed` are sized from `send_channels`.
  - Both counts are passed to `serialize_samples_into`.
- **WebTransport main thread** (`src/audio/webtransport.rs`): the worker init JSON sends `sendChannels` and `receiveChannels` instead of `channels`.
- **WebTransport worker** (`src/audio/webtransport_worker.rs`):
  - The init arm parses both (defaults 1 and 2) and passes them to `worker_init`.
  - `WorkerState` has `send_channels` and `receive_channels`. `configure` sizes buffers from `send_channels`.
  - `build_next_packet` passes both counts.
  - The `handle_worker_message` contract doc lists the new fields.
- **Receive side.** `deliver_received_packet` reads the payload width from byte 15 (§1). `Regulator` is unchanged: it still adopts the count from the first packet and rejects changes afterwards. The hub should send `receive_channels`, and `map_to_output` handles any mismatch.

### 3. Session config: `src/session.rs`
- **Fields and API.**
  - `channels` is replaced by `send_channels` (default 1), `receive_channels` (default 2), `input_channels` (default 2) and `output_channels` (default 2).
  - JS API: `setSendChannels`/`getSendChannels`, `setReceiveChannels`/`getReceiveChannels`, `setInputChannels`/`getInputChannels`, `setOutputChannels`/`getOutputChannels`.
- **Setters.** All four are Idle-only and validated with `is_valid_channel_count`, through one shared private guard, `can_set_channel_count`, which logs why a change was refused.
  - `setSendChannels` also stores into `AudioParams::send_channels` (§4).
- **Regulator.** The constructor configures the regulator with `receive_channels`.
- **Connect.**
  - `connect_to_studio` builds `AudioBufferConfig` from `send_channels` and `receive_channels`.
  - `start_capture` passes `input_channels` and `output_channels` into the engine.
- **Deleted.**
  - `narrowed_channel_count`, its use in `disconnect`, and `narrowed_channel_count_matrix`.
  - The engine pass-through getters `getGrantedInputChannels`, `getMaxInputChannels` and the old engine-backed `getOutputChannels`.
  - The doc comments describing the old coupling.

### 4. Engine, params and processor
- **`src/audio/params.rs`.** `capture_channels` is renamed `send_channels`, with default 1 and the same `[1, MAX_CHANNELS]` clamp. The render thread reads it in `send_local_to_network`.
- **`AudioEngine::start_capture`** (`src/audio/engine.rs`) takes `input_channels` and `output_channels` and no longer reads a channel count from params.
  - **Capture request.** `AudioConstraints::to_js` always asks for `channelCount: { ideal: MAX_CHANNELS }`, so the browser opens the device at its native width instead of downmixing, and "input channels = N" reliably means the first N device channels.
  - **Worklet input width.** `min(input_channels, granted)`, where granted is the track's `getSettings().channelCount` (`resolve_input_channels(settings, None)`). The node's Explicit/Discrete `channelCount` drops extra source channels per the Web Audio spec, which is how Mono gets only channel 0.
    - If granted is less than requested (for example, Chrome forcing mono when echo cancellation is on), the narrower width is used and the send-side conform duplicates it.
  - **Engine state.** The `granted_input_channels`/`max_input_channels` fields and their `grantedInputChannels`/`maxInputChannels` getters are removed; capability discovery moves to the probe (§5). The engine instead stores `worklet_input_channels` (the width the node was built with) and `requested_output_channels`, so `set_output_device`'s rebuild keeps the clamped input width — rebuilding at the raw granted width would silently undo Mono — and re-clamps the requested output against the new sink.
  - **`AudioConstraints`.** The `channel_count: Option` parameter is gone. The struct, `resolve`, `to_js` and `route_output_sink` are `pub(crate)` so the probes reuse them.
- **`configure_destination(requested)`.** Sets the destination width to `clamp_channel_count(min(requested, destination.max_channel_count()))`. A width above `maxChannelCount` throws.
- **`src/audio/processor.rs`.** Mapping is unchanged; only the wire-width comments and names changed. The three demo modes fall out of `conform_interleaved_channels`:
  - 2 captured to 1 sent averages (Mix to Mono);
  - 2 to 2 is 1:1 (Stereo);
  - 1 to 1 is identity (Mono).

  `send_local_to_network` reads `get_send_channels()`.

### 5. Device probing: `src/audio/devices.rs`
Two exported async free functions. They are module-level, not session methods, so they avoid wasm-bindgen's re-entrancy check while a connect is in flight.

- **`getInputDeviceChannels(deviceId?: string) -> Promise<number>`.**
  - Calls `getUserMedia` with `AudioConstraints::resolve(deviceId, false, false, false)` — `deviceId: {exact}` when an id is given, processing toggles off, `channelCount: {ideal: MAX_CHANNELS}`.
  - Reads the first track's `getCapabilities().channelCount.max` (guarded for Safari, which lacks `getCapabilities`), falling back to `getSettings().channelCount`.
  - Stops the stream with `stop_media_stream`.
  - Value resolution is the pure `resolve_input_channels(settings: Option<u32>, capability_max: Option<u32>) -> u32`, clamped to `[1, MAX_CHANNELS]`.
- **`getOutputDeviceChannels(deviceId?: string) -> Promise<number>`.**
  - Creates a throwaway `AudioContext`. When an id is given, it routes with `route_output_sink` (graceful no-op without `setSinkId`).
  - Reads `destination.max_channel_count()` clamped to `[1, MAX_CHANNELS]`, then `close()`s the context, also on error.
  - Without `setSinkId` (Safari, Firefox) this reports the default output, which is the device those browsers play to anyway.
- **Shared helpers** (`pub(crate)`, used by both the probes and `AudioEngine::start_capture`):
  - `get_user_media_audio(media_devices, audio_constraint)` — the one audio-only `getUserMedia` call, also used by `request_audio_permission`;
  - `first_audio_track`, `settings_channels`, `clamp_channel_count`, and the private `capability_max_channels`.
- **Exports.** Both probes are exported to JS the same way as `getAudioDevices` (`#[wasm_bindgen(js_name = ...)]`).

### 6. Demo: `website/src/pages/demo/Demo.tsx`
- **State.**
  - `stereo`/`maxInputChannels` are replaced by `inputDeviceChannels: number | undefined` and `outputDeviceChannels: number | undefined`.
  - `captureMode: "mixToMono" | "stereo" | "mono"` (default `"mixToMono"`) holds the user's choice for multi-channel inputs.
  - A `CAPTURE_MODES` table maps each mode to its next mode, button text and `(inputChannels, sendChannels)` pair.
  - The mount-time `getChannels()` resync and the "reset on leaving connected" / "force mono" effects are removed.
- **Probing effects.**
  - One effect keyed on `inputDeviceId` and one on `outputDeviceId` (plus the probe nonce). Each sets the count to `undefined`, calls the probe, and ignores a stale result using the `let cancelled` cleanup pattern.
  - A failed probe logs a warning and assumes 2, so Connect never stays disabled; capture and playback still clamp to the real device.
  - A 1-channel input captures Mono without overwriting `captureMode`: `handleConnect` uses `"mono"` when the probed count is 1, so switching back to a multi-channel device keeps the user's choice (default Mix to Mono).
- **devicechange.**
  - A `navigator.mediaDevices` `devicechange` listener, registered in an effect and removed on cleanup.
  - It re-runs `getAudioDevices()`. If a selected id disappeared it falls back to the first device (`pickDevice`, shared with the mount path).
  - It always re-probes the selected devices, even if the id is unchanged, because "default" may now point at different hardware. A probe nonce in the effect dependencies forces this.
- **Capture button.**
  - Hidden until `inputDeviceChannels` is known.
  - Shows a locked "Mono" / "1 ch" when the count is 1.
  - Otherwise it is a cycling `ToggleButton`: Mix to Mono ("2 → 1 ch") → Stereo ("2 ch") → Mono ("1st ch").
  - Reuses `ToggleButton` and `.toggle-btn-compact`; no new CSS.
- **`handleConnect`.** Right before `connectToStudio`, after pending teardowns drain and while still Idle:
  - `setReceiveChannels(2)`
  - `setOutputChannels(outputDeviceChannels >= 2 ? 2 : 1)`
  - `setInputChannels` / `setSendChannels` from the mode's `CAPTURE_MODES` entry.

  Applying config at connect time avoids syncing the module-singleton session on remount.
- **Locking.**
  - The connection and device settings are disabled while `busy || inProgress || connected || sessionState === "error"`: host, port, client name, transport, input select, output select, AGC, Echo, Noise, and the capture button.
  - Connect also waits until both device channel counts are known.
  - Gain, volume and monitor sliders stay live, since they are runtime controls.
  - Changing the output select only updates state; the selected output id is applied in `handleConnect` via `setOutputDevice`.
- **`website/src/lib/webtrip.ts`.** Unchanged. `getAudioDevices` stays cast to `AudioDevices`; the probes return `number` from the generated `.d.ts`.

### 7. Docs
- **`docs/CHANNEL_MODEL.md`.** Rewritten to the new model:
  - configured (send/receive/input/output) vs. actual (capture, peer, output) channel counts;
  - the byte 14/15 semantics and how a hub configures a client from them;
  - input/output device counts with first-N semantics and clamping;
  - pre-connect probing;
  - the mapping rules at each boundary and the demo's capture modes;
  - no narrowing and no `capture_channels` "double duty" invariant.
- **`docs/WASM_TESTING.md`.** Lists the probe tests in the coverage list.

## Tests (per AGENTS.md testing guidelines)

- **protocol.rs, regression test for this bug.** `test_serialize_samples_into_then_deserialize_mono_roundtrip` sends 1 / receives 2 and asserts:
  - byte 14 == 2 (receive) and byte 15 == 1 (send);
  - total length == `HEADER_SIZE + 128 * 1 * 2`, sized by send;
  - samples round-trip.
- **protocol.rs, payload width.** `test_outgoing_channels_encoding_asymmetric` also round-trips an in=2/out=4 packet through `serialize_into` and `deserialize_into` at a 4-channel payload, pinning payload width to `num_outgoing_channels`.
- **Wire contract.** `webtransport.rs` `worker_init_message_has_expected_fields` pins `sendChannels`/`receiveChannels` and the absence of `channels`.
- **Worker.**
  - `handle_worker_message_init_resolves_ready` sends the new fields;
  - `configure_sizes_buffers_and_stores_pointers` uses send ≠ receive, with buffers sized by send;
  - `build_next_packet_serializes_full_frames_and_advances_counters` asserts the deserialized header's byte 14 carries the configured receive count, i.e. the worker threads it through.
- **WebRTC.** `webrtc_set_audio_buffers_resizes_internal_buffers` sizes by `send_channels` with send ≠ receive, so a swapped-field bug fails.
- **Session.** The wasm `session_mock_connect_disconnect_lifecycle`:
  - sets send/receive/input/output away from their defaults while Idle, and checks `AudioParams::send_channels`;
  - attempts changes while connected and asserts they are unchanged;
  - after disconnect, asserts the values are retained (no narrowing).

  `narrowed_channel_count_matrix` is deleted.
- **Devices.**
  - Native boundary test `test_resolve_input_channels_boundaries`: None/None → 1, capability-only, settings fallback, capability wins over settings, 0 → 1, 9 → 8.
  - Browser tests `get_input_device_channels_resolves_count_or_rejects` and `get_output_device_channels_resolves_count_or_rejects`: the default device resolves within `[1, MAX_CHANNELS]`, and an unknown device id rejects.
  - `stop_media_stream_ends_tracks` reuses `get_user_media_audio`.
- **Engine.** The constraints matrix drops the channel-count axis, and the `to_js_*` tests assert the fixed `ideal: MAX_CHANNELS`.
- **Processor.** `capture_channels` is renamed `send_channels` in the two existing tests. No new mapping tests; the matrices already cover Mix to Mono averaging.

## Verification
1. `npm run check`, `npm run test`, `npm run test:wasm`, and `npm run build`, which also type-checks the website.
2. `npm run test:integration`, a puppeteer run against the Docker hub. It doesn't depend on the removed session APIs.
3. `npm run serve` and open `/demo` in Chrome:
   - The capture button is hidden until probed. With a 2-input interface it cycles through three modes starting at Mix to Mono; with a mono mic it shows locked Mono.
   - Plugging or unplugging a device (`devicechange`) re-lists devices and re-probes channels.
   - Connect with Mono / Mix to Mono against a stereo peer or a hub loopback, over both transports (WebRTC needs a hub with the JackTrip fix above). The client sends 1 channel and still receives 2: watch `regulator` channels via stats/console, and confirm byte 14 == 2 and byte 15 == 1 in a logged outbound header. There must be no `BufferTooSmall` deserialize errors. This is the original bug.
   - Stereo sends 2. Mono sends only channel 0 of a 2-channel input; check with a signal on channel 1 only, which should be silent. Mix to Mono carries both.
   - All config controls are disabled from Connect until back to idle; sliders still work.
