# Cursor Cloud Agent Instructions

**Audience:** Cursor Cloud agents only. Local IDE agents and other tooling should not read this file — general project guidance lives in [AGENTS.md](../AGENTS.md).

This is a single web app (the WebTrip Demo). The standard commands (`npm run build`,
`npm run check`, `npm run test`, `npm run test:wasm`, `npm run serve`) are documented
in [AGENTS.md](../AGENTS.md), [README.md](../README.md), and [WASM_TESTING.md](WASM_TESTING.md). The toolchain (Rust nightly,
`wasm-pack`, version-matched `wasm-bindgen-cli`, Node, a Chrome-matched `chromedriver`)
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
