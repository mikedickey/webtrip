# Cursor Cloud Agent Instructions

**Audience:** Cursor Cloud agents only. Local IDE agents and other tooling should not read this file — general project guidance lives in [AGENTS.md](../AGENTS.md).

This is a single web app (the WebTrip Demo). The standard commands (`npm run build`,
`npm run check`, `npm run test`, `npm run test:wasm`, `npm run serve`) are documented
in [AGENTS.md](../AGENTS.md), [README.md](../README.md), and [WASM_TESTING.md](WASM_TESTING.md). The toolchain (Rust nightly,
`wasm-pack`, version-matched `wasm-bindgen-cli`, `cargo-llvm-cov` + the
`llvm-tools-preview` component, `clang`, Node, a Chrome-matched `chromedriver`)
is pre-installed in the VM image; the startup update script only runs `npm install`.

## Non-obvious caveats

- **Headless WASM tests** (`npm run test:wasm`) drive Chrome through `chromedriver`,
  which must match the installed Google Chrome version (run `google-chrome --version`,
  then install the matching driver from Chrome for Testing). A working `chromedriver`
  is on `PATH` at `/usr/local/bin/chromedriver`; if `wasm-pack` cannot find it, set
  `CHROMEDRIVER=/usr/local/bin/chromedriver`. Headless flags come from `webdriver.json`.
- **The app requires microphone access**, and the VM has no physical microphone, so a
  normally-launched Chrome shows only the "Microphone Access Required" error. To load
  the full UI, launch Chrome with `--use-fake-device-for-media-stream
  --use-fake-ui-for-media-stream` (gives a synthetic mic tone and auto-grants
  permission). When the computer-use Chrome is already running, relaunching with these
  flags requires first stopping the existing Chrome (it reuses a fixed user-data-dir).
- **To exercise the audio pipeline end-to-end without a real JackTrip server**, select
  the **Mock** transport in the Studio Connection section, then click "Connect to
  Studio". The status becomes "Connected (Mock)" and the Regulator/Quality stats panel
  and Level meter update live.
- `npm run serve` listens on `http://localhost:3000` and sets the COOP/COEP headers
  required for `SharedArrayBuffer`/threading.
- **Coverage** (`npm run coverage` → `lcov.info`, `npm run coverage:wasm` →
  `lcov.wasm.info`) runs fully in this VM: `cargo-llvm-cov` + `llvm-tools-preview`
  + a wasm-capable `clang` (for minicov's C runtime) are in the image, and
  `coverage:wasm` drives the same headless Chrome as `test:wasm` (set
  `CHROMEDRIVER=/usr/local/bin/chromedriver` if `wasm-pack` can't find it).
- **Integration tests** (`npm run test:integration`, `npm run coverage:integration`)
  use `puppeteer-core` (installed) to drive `google-chrome` against a live
  `jacktrip/jacktrip:edge` hub presenting a browser-trusted `*.miked.io` cert at
  `localhost.miked.io:4464` (which resolves to `127.0.0.1` via public DNS). This
  works in the VM, but with two non-obvious caveats:
  - **Docker is pre-installed but `dockerd` is not auto-started** — run
    `sudo dockerd &` once per session (its `daemon.json` pins the
    `fuse-overlayfs` storage driver and disables the `containerd-snapshotter`
    feature, both required for Docker 29 to work in this nested VM).
  - **The bundled compose file (`tests/integration/docker-compose.integration.yml`)
    does not work here**: the image boots via `systemd` (`/sbin/init`), and
    `systemd` cannot create its `init.scope` cgroup in this Firecracker VM
    (`Failed to allocate manager object: Structure needs cleaning`). Instead run
    the hub's two processes directly with a custom entrypoint (the JACK server
    uses the hardware-free `dummy` backend):
    ```sh
    sudo docker run -d --name jacktrip --network host --privileged --shm-size=512M \
      --entrypoint sh -v "$PWD/certs:/certs:ro" jacktrip/jacktrip:edge -c '
        export JACK_NO_AUDIO_RESERVATION=1 JACK_NO_START_SERVER=1
        /usr/local/bin/jackd -d dummy -C 0 -P 0 --rate 48000 --period 128 &
        /usr/local/bin/jack_wait -w -t 5
        exec /usr/local/bin/jacktrip -S -D -I 1 -p 4 --bufstrategy 3 -q auto --udprt \
          --certfile /certs/star.miked.io.chained.crt --keyfile /certs/star.miked.io.key'
    ```
  - **Certs** live in the gitignored `certs/` dir as `star.miked.io.chained.crt`
    (full chain) + `star.miked.io.key`. The key comes from the `MIKED_TLS_KEY`
    secret (`printf '%s\n' "$MIKED_TLS_KEY" > certs/star.miked.io.key`); the cert
    exceeds the 4096-char secret limit so it is provided out-of-band (persisted in
    `certs/` here). Verify the pair matches via `openssl` modulus if in doubt.
  - Then run the harness (the running `npm run serve` already owns port 3000, so
    pass a different `APP_PORT`):
    `PUPPETEER_EXECUTABLE_PATH=$(command -v google-chrome) APP_PORT=3200 npm run test:integration:run`.
    Expect `✅ webrtc:` and `✅ webtransport:` each `connected; sent … samples`.
  - `coverage:integration` rebuilds an instrumented `--dev` pkg (`build:wasm:coverage`);
    the receive/teardown path logs `panicked … attempt to subtract with overflow` /
    `memory access out of bounds` in the browser console *after* the coverage dump —
    this is instrumented-build teardown noise, not a test failure; it still writes
    `lcov.integration.info`. Rebuild the optimized pkg (`npm run build:wasm`)
    afterward so `serve`/`test:integration` use the release build again.
  - To smoke-test just the harness plumbing without the hub, run with
    `INTEGRATION_TRANSPORTS=mock` and a dummy TCP listener on `127.0.0.1:4464`
    (the `mock` transport connects without touching the network).
