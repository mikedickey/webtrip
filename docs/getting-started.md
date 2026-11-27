# Getting Started

WebTrip ships as an npm package ([`webtrip`](https://www.npmjs.com/package/webtrip),
the wasm-pack output in `pkg/`) and as a set of Rust crates. This page covers
installing it, building from source, running the demo, and the shape of a minimal
session in JavaScript.

## Install from npm

```bash
npm install webtrip
```

The package includes TypeScript declarations. Two things to know about bundlers:

- **Load it with a dynamic `import("webtrip")`.** The AudioWorklet and the
  WebTransport worker import the package's JavaScript from its own URL and expect its
  exports under their real names. A dynamic import keeps it as a separate chunk with
  those names intact. A static import lets the bundler merge it into your app's chunk,
  and audio fails to start.
- **With Vite, exclude it from dependency pre-bundling** so the dev server serves it
  next to its `.wasm` file:

  ```js
  // vite.config.js
  export default { optimizeDeps: { exclude: ["webtrip"] } };
  ```

The page also has to meet the [hosting requirements](#hosting-requirements) below.

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (via `rustup`)
- [Node.js](https://nodejs.org/) (via `nvm` or direct install)
- [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/) — install with
  `cargo install wasm-pack`

The pinned nightly toolchain and required components install automatically from
`rust-toolchain.toml` the first time you build. The browser build needs a few
unusual flags; the npm scripts pass them for you (see
[toolchain requirements](contributing/toolchain.md) for why).

## Build

```bash
npm install
npm run build        # WASM module + website
```

Or build each half separately:

```bash
npm run build:wasm   # Rust → pkg/webtrip.js + pkg/webtrip_bg.wasm
npm run build:site   # React site → website/dist
```

## Run the demo

```bash
npm run serve
```

Then open <http://localhost:3000/demo> (or <https://localhost:8443/demo> when serving
with TLS). The demo asks for microphone access when the page loads.

For website development with hot reload, run `npm run dev` inside `website/`; the
demo works there too once `npm run build:wasm` has produced `pkg/`.

## Hosting requirements

WebTrip runs audio on real threads over a shared WebAssembly memory, so the page
must be **cross-origin isolated** for `SharedArrayBuffer` to exist. Serve it with:

```
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

`website/server.js` does this for the demo; with Vite, set the same headers under
`server.headers` and `preview.headers`. See [browser support](browser-support.md) for
the browser features WebTrip needs.

The page must also be a **secure context** (HTTPS, or `localhost` in development):
hub signaling uses `wss://`, and microphone access and WebTransport are only
available in secure contexts. Start audio (`startAudio()` or `connect()`) from a user
gesture such as a click; otherwise the browser may keep the `AudioContext`
suspended (see `isAudioSuspended()` / `resumeAudio()`).

## A minimal session

```javascript
const webtrip = await import("webtrip");
await webtrip.default(); // instantiate the WASM module (once per page)
webtrip.init();          // install the panic hook

// Shared audio parameters (volume, gain, meters) live in WASM memory so the
// AudioWorklet can read them; the session shares them. Never freed, so
// create them once per page.
const params = webtrip.createAudioParams();
const session = new webtrip.WebTripSession(params);

session.setOnStateChange((state) => console.log("network:", state));

// Start audio and connect to a JackTrip hub in one call...
await session.connect(
  "hub.example.com", // server host (a studio's serverHost)
  4464,              // signaling port
  undefined,         // input device id (default device)
  false,             // auto gain control
  false,             // echo cancellation
  false,             // noise suppression
  "my-client",       // client name shown to other participants
);

// ...and tear both down again.
await session.disconnect();
```

Audio and network are independent: `startAudio()` / `stopAudio()` run the audio
engine without a connection (for monitoring or device selection), and
`connectNetwork()` / `disconnectNetwork()` manage the connection without touching
audio. `connect()` / `disconnect()` do both. The full contract of each call
(cancellation, `AbortError`, timeouts) is in the
[`WebTripSession` API reference](https://docs.rs/webtrip-web/latest/webtrip_web/session/struct.WebTripSession.html).

Other useful entry points:

- `getAudioDevices()` lists input and output devices.
- `session.setTransportType(...)` chooses WebRTC data channels or WebTransport;
  `WebTripSession.isWebTransportAvailable()` reports whether the browser supports
  the latter.
- `session.setSendChannels()` / `setReceiveChannels()` / `setInputChannels()` /
  `setOutputChannels()` configure channel counts before connecting — see
  [channel model](channel-model.md).

## Talking to JackTrip Virtual Studio

The same package exports `ApiClient`, a typed client for the JackTrip Virtual Studio
REST API (studios, users, devices, recordings, billing, …):

```javascript
const client = new webtrip.ApiClient();
const ping = await client.system().ping();

client.setBearerToken(token);
const studios = await client.studios().listStudios();
```

From Rust, the same client is the `webtrip-api` crate (or `webtrip::api`). See the
[`webtrip-api` reference](https://docs.rs/webtrip-api) for every endpoint.

## Next steps

- [Architecture](architecture.md) — threads, data flow, and why it is built this way
- [Channel model](channel-model.md) — how channel counts are negotiated and mapped
- [API reference](https://docs.rs/webtrip) — rustdoc for every crate
