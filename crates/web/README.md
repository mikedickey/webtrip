# webtrip-web

The browser host for [WebTrip](https://webtrip.dev): lossless, low-latency JackTrip
audio in a web page.

This crate wires [`webtrip-core`](https://docs.rs/webtrip-core) to browser APIs: an
AudioWorklet audio engine, WebRTC data-channel and WebTransport (QUIC datagram)
transports, and hub signaling. [`WebTripSession`](session::WebTripSession) composes
them. It is built with wasm-pack into the [`webtrip`](https://www.npmjs.com/package/webtrip)
npm package, which the [demo](https://webtrip.dev/demo) also uses. The package also
exports the [`webtrip-api`](https://docs.rs/webtrip-api) client (`ApiClient`) for
JackTrip Virtual Studio.

## Using it from JavaScript

```bash
npm install webtrip
```

```js
// A dynamic import keeps the package in its own chunk, which its AudioWorklet
// and worker re-import by URL. See getting started for bundler notes.
const webtrip = await import("webtrip");
await webtrip.default(); // instantiate the module, once per page
webtrip.init();

// Shared audio parameters (volume, gain, meters); never freed, so create once.
const session = new webtrip.WebTripSession(webtrip.createAudioParams());
session.setOnStateChange((state) => console.log("network:", state));

// Asks for microphone access, so device labels are filled in.
const { inputDevices, outputDevices } = await webtrip.getAudioDevices();

joinButton.onclick = async () => {
  // From a click, so the browser lets the AudioContext run.
  await session.startAudio(inputDevices[0]?.deviceId, false, false, false);
  await session.connectNetwork("hub.example.com", 4464, "me");
  // Or both in one call:
  // await session.connect("hub.example.com", 4464, undefined, false, false, false, "me");
};

leaveButton.onclick = async () => {
  await session.disconnectNetwork();
  await session.stopAudio();
  // session.free() once you no longer need the session.
};
```

The full contract of each call (state changes, rejections, cancellation) is on
[`WebTripSession`](session::WebTripSession).

## Page requirements

- **Cross-origin isolation** (`Cross-Origin-Opener-Policy: same-origin` and
  `Cross-Origin-Embedder-Policy: require-corp`), so `SharedArrayBuffer` exists.
- **A secure context**: HTTPS, or `localhost` in development. Hub signaling uses
  `wss://`, and `getUserMedia` and WebTransport need a secure context.
- **A user gesture** to start audio: call `startAudio()` or `connect()` from a click
  handler, or the browser may keep the `AudioContext` suspended. Check
  `isAudioSuspended()` and call `resumeAudio()` from a gesture if it is.
- **The microphone permission**: `getAudioDevices()`, `startAudio()` and `connect()`
  ask for it.
- **The package served unbundled**, as an ES module next to `webtrip_bg.wasm`: the
  AudioWorklet and the WebTransport worker import the package's JavaScript from its
  own `import.meta.url` and use its exports by their real names.

See [getting started](https://webtrip.dev/docs/getting-started) and
[browser support](https://webtrip.dev/docs/browser-support).

## Using it from Rust

The browser host only works on `wasm32-unknown-unknown` with threads, and it needs
a nightly toolchain to rebuild `std` with atomics. Build with:

```bash
RUSTFLAGS="--cfg=web_sys_unstable_apis -Ctarget-feature=+atomics \
  -Clink-args=--shared-memory -Clink-args=--max-memory=1073741824 \
  -Clink-args=--import-memory -Clink-args=--export=__wasm_init_tls \
  -Clink-args=--export=__tls_size -Clink-args=--export=__tls_align \
  -Clink-args=--export=__tls_base -Clink-args=--export=__heap_base" \
cargo build --target wasm32-unknown-unknown -Zbuild-std=std,panic_abort
```

A `cdylib` that depends on this crate must be bound with
`wasm-bindgen --target web` (an ES module), and must keep these exports under their
own names, since the worklet and worker glue call them: `initSync`, the default
`init`, `ProcessorHandle` and `handleWorkerMessage`. See
[toolchain requirements](https://github.com/mikedickey/webtrip/blob/main/docs/contributing/toolchain.md)
for why each flag is needed.

```rust,ignore
use webtrip_core::audio::params::create_audio_params;
use webtrip_web::WebTripSession;

// Inside an async fn on the page's main thread.
let session = WebTripSession::new(&create_audio_params())?;
session
    .connect("hub.example.com".into(), Some(4464), None, false, false, false, Some("me".into()))
    .await?;
// ...
session.disconnect().await;
```

## Features

- `coverage` — exports an LLVM profile dump for the integration-test harness. Never
  enable it in a shipped build.
