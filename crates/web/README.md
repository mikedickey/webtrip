# webtrip-web

The browser host for [WebTrip](https://webtrip.dev): lossless, low-latency JackTrip
audio in a web page.

This crate wires [`webtrip-core`](https://docs.rs/webtrip-core) to browser APIs: an
AudioWorklet audio engine, WebRTC data-channel and WebTransport (QUIC datagram)
transports, and hub signaling. [`WebTripSession`](session::WebTripSession) composes
them. It is built with wasm-pack into the JavaScript package that the
[demo](https://webtrip.dev/demo) uses.

It only builds for `wasm32-unknown-unknown`, with threads enabled
(`-Ctarget-feature=+atomics`, shared memory, `-Zbuild-std`) and
`--cfg=web_sys_unstable_apis` for the WebTransport bindings. See
[toolchain requirements](https://github.com/mikedickey/webtrip/blob/main/docs/contributing/toolchain.md).

## Using it from JavaScript

```js
const webtrip = await import("/pkg/webtrip.js");
await webtrip.default();
webtrip.init();

const session = new webtrip.WebTripSession(webtrip.createAudioParams());
await session.connect("hub.example.com", 4464, undefined, false, false, false, "me");
// ...
await session.disconnect();
```

The page must be cross-origin isolated (COOP/COEP) for `SharedArrayBuffer`. See
[getting started](https://webtrip.dev/docs/getting-started) and
[browser support](https://webtrip.dev/docs/browser-support).

## Features

- `coverage` — exports an LLVM profile dump for the integration-test harness. Never
  enable it in a shipped build.
