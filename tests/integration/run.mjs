// Integration test harness: drive the real WASM client against a live JackTrip
// server, for BOTH the WebRTC and WebTransport transports.
//
// Why a served-app + browser-driver harness (not `wasm-bindgen-test`): the
// WebTransport worker loads the wasm module from `{origin}/pkg/webtrip.js`
// (see `src/audio/webtransport.rs::wasm_module_url`), which only resolves when
// the page is served at the site root with `pkg/` present. So we serve the real
// app via `server.js`, point a headless browser at it, and run each transport
// through the actual exported session API
// (`createAudioParams` → `WebTripSession` → `connectToStudio`).
//
// The page is served over plain HTTP on `localhost` — a "secure context" in
// Chrome — so with the COOP/COEP headers `server.js` already sets we still get
// `crossOriginIsolated` (SharedArrayBuffer) and WebTransport. No cert is needed
// for the page server. The only thing that needs the trusted `*.miked.io` cert
// is the JackTrip server itself: the browser validates it on the
// `wss://`/WebTransport connection to JACKTRIP_TEST_HOST (that check cannot be
// bypassed). When running the server via the bundled compose file, the cert is
// supplied to *that* container, not to this script.
//
// Prerequisites:
//   - `npm run build` has produced `pkg/` (and `dist/`).
//   - A live JackTrip server presenting a browser-trusted cert is reachable at
//     JACKTRIP_TEST_HOST:port (see tests/integration/docker-compose.integration.yml).
//
// Env knobs (all optional):
//   APP_HOST                page-server host (default localhost)
//   APP_PORT                page-server port (default 3000)
//   JACKTRIP_TEST_HOST      JackTrip host (default localhost.miked.io)
//   JACKTRIP_TEST_PORT      JackTrip port (default 4464)
//   INTEGRATION_TRANSPORTS  comma list (default "webrtc,webtransport")
//   PUPPETEER_EXECUTABLE_PATH  Chrome/Chromium binary (auto-detected otherwise)

import net from "node:net";
import fs from "node:fs";
import path from "node:path";
import { spawn, execSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "..", "..");

const APP_HOST = process.env.APP_HOST || "localhost";
const APP_PORT = parseInt(process.env.APP_PORT || "3000", 10);
const JT_HOST = process.env.JACKTRIP_TEST_HOST || "localhost.miked.io";
const JT_PORT = parseInt(process.env.JACKTRIP_TEST_PORT || "4464", 10);
const TRANSPORTS = (process.env.INTEGRATION_TRANSPORTS || "webrtc,webtransport")
  .split(",")
  .map((s) => s.trim())
  .filter(Boolean);

// Per-transport budgets (ms).
const CONNECT_TIMEOUT_MS = 30_000;
const SEND_POLL_MS = 10_000;

/** Print an error and mark the process as failed (without exiting immediately). */
function fail(msg) {
  console.error(`\n❌ ${msg}`);
  process.exitCode = 1;
}

/**
 * Resolve a Chrome/Chromium executable: an explicit env path first, then common
 * binaries on PATH, then the default macOS location. Throws if none are found.
 */
function resolveChrome() {
  const explicit = process.env.PUPPETEER_EXECUTABLE_PATH || process.env.CHROME_BIN;
  if (explicit) return explicit;
  for (const c of ["chromium", "chromium-browser", "google-chrome", "google-chrome-stable"]) {
    try {
      const p = execSync(`command -v ${c}`, { stdio: ["ignore", "pipe", "ignore"] })
        .toString()
        .trim();
      if (p) return p;
    } catch {
      /* not found, try next */
    }
  }
  const mac = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
  if (fs.existsSync(mac)) return mac;
  throw new Error(
    "No Chrome/Chromium found. Install one or set PUPPETEER_EXECUTABLE_PATH.",
  );
}

/** Resolve once `host:port` accepts a TCP connection, or reject after `timeoutMs`. */
function waitForPort(host, port, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  return new Promise((resolve, reject) => {
    const tryOnce = () => {
      const sock = net.connect({ host, port }, () => {
        sock.destroy();
        resolve();
      });
      sock.on("error", () => {
        sock.destroy();
        if (Date.now() > deadline) reject(new Error(`timeout waiting for ${host}:${port}`));
        else setTimeout(tryOnce, 250);
      });
    };
    tryOnce();
  });
}

/**
 * Runs inside the page. Drives one transport end-to-end through the exported
 * session API (init → connect → resume → poll send path → disconnect) and
 * returns plain JSON for the Node side to assert on.
 */
async function inPageDrive({ transportName, host, port, connectTimeoutMs, sendPollMs }) {
  const m = await import("/pkg/webtrip.js");

  // The served app (dist/app.js) normally initializes the wasm module. If it
  // hasn't (e.g. dist not built), initialize it here. Probe before init to avoid
  // a redundant second instantiation.
  let ready = false;
  try {
    m.createAudioParams();
    ready = true;
  } catch {
    /* not initialized yet */
  }
  if (!ready) {
    if (typeof m.default === "function") await m.default();
    if (typeof m.init === "function") m.init();
  }

  const ttByName = {
    webrtc: m.TransportType.WebRTC,
    webtransport: m.TransportType.WebTransport,
    mock: m.TransportType.Mock,
  };
  const tt = ttByName[transportName];
  if (tt === undefined) throw new Error(`unknown transport: ${transportName}`);

  if (transportName === "webtransport" && !m.WebTripSession.isWebTransportAvailable()) {
    return { skipped: true, reason: "WebTransport unavailable in this browser" };
  }

  const ptr = m.createAudioParams();
  const session = new m.WebTripSession(ptr);
  session.setTransportType(tt);

  // Race the connect against a timeout, clearing the timer on settle so the
  // loser never fires a late (unhandled) rejection into the page.
  const connectPromise = session.connectToStudio(
    host, port, undefined, false, false, false, `webtrip-it-${transportName}`,
  );
  // If the timeout wins, connectPromise may still settle later; swallow it.
  connectPromise.catch(() => {});
  let timer;
  try {
    await Promise.race([
      connectPromise,
      new Promise((_, rej) => {
        timer = setTimeout(() => rej(new Error("connect timeout")), connectTimeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }

  // Resume the AudioContext so the worklet actually runs and feeds the send ring
  // buffer headless (paired with --autoplay-policy=no-user-gesture-required).
  try {
    await session.resumeAudio();
  } catch {
    /* best-effort */
  }

  const connected = session.isConnected();

  // Poll the send path: captured (fake-device) audio must reach the ring buffer.
  const deadline = Date.now() + sendPollMs;
  while (Date.now() < deadline) {
    if (Number(session.get_stats().ring_buffer_samples_written) > 0) break;
    await new Promise((r) => setTimeout(r, 200));
  }

  // Receive-path counters are reported for visibility but NOT asserted here.
  // This harness drives a SINGLE client against the hub (`-p 4`, see
  // docker-compose.integration.yml), so with no second participant the hub mixes
  // nothing back: `recvPlayed` stays 0 and `recvInitialized` stays false. Rather
  // than depend on a second live client (flaky, and the hub patch mode does not
  // reliably loop a lone/echo client back to itself), the inbound audio path —
  // the WebTransport worker's datagram read loop (`parse_read_result` →
  // `handle_datagram` → `Regulator::push`, plus the deserialize-error / high-
  // error-rate / connection-lost branches) and the WebRTC data-channel receive
  // body (`enqueue_channel_message`, both channel-creation directions, incl. the
  // non-ArrayBuffer rejection) — is covered by targeted browser unit tests
  // (`#[wasm_bindgen_test]` in src/audio/webtransport_worker.rs and
  // src/audio/webrtc.rs; see WEB-45). Those feed synthetic inbound data straight
  // into the receive handlers and assert on Regulator state + the stats counters.
  const s = session.get_stats();
  const result = {
    connected,
    sentSamples: Number(s.ring_buffer_samples_written),
    ringWrites: Number(s.ring_buffer_writes),
    recvPlayed: Number(s.regulator_packets_played),
    recvInitialized: s.regulator_initialized,
  };

  await session.disconnect();
  return result;
}

/**
 * Serve the app, drive each requested transport against the live JackTrip
 * server in a headless browser, and assert connect + send-path. Sets a non-zero
 * exit code (via `fail`/throw) on any failure.
 */
async function main() {
  if (TRANSPORTS.length === 0) {
    throw new Error("INTEGRATION_TRANSPORTS is empty — nothing to test");
  }
  if (!fs.existsSync(path.join(REPO_ROOT, "pkg", "webtrip.js"))) {
    throw new Error("pkg/webtrip.js missing — run `npm run build` first.");
  }

  // Serve the real app over plain HTTP on localhost (a secure context, so
  // server.js's COOP/COEP still yield crossOriginIsolated + WebTransport). No
  // cert needed here — only the JackTrip server needs a browser-trusted cert.
  // server.js reads PORT, so APP_PORT is honored.
  console.log(`▶ starting app server on http://${APP_HOST}:${APP_PORT}`);
  const server = spawn("node", ["server.js"], {
    cwd: REPO_ROOT,
    stdio: "inherit",
    env: { ...process.env, PORT: String(APP_PORT) },
  });
  let browser;
  try {
    await waitForPort("127.0.0.1", APP_PORT, 15_000);

    console.log(`▶ verifying JackTrip server at ${JT_HOST}:${JT_PORT}`);
    await waitForPort(JT_HOST, JT_PORT, 30_000);

    browser = await puppeteer.launch({
      executablePath: resolveChrome(),
      headless: true,
      args: [
        "--no-sandbox",
        "--disable-dev-shm-usage",
        "--use-fake-device-for-media-stream",
        "--use-fake-ui-for-media-stream",
        "--autoplay-policy=no-user-gesture-required",
      ],
    });

    const page = await browser.newPage();
    page.on("console", (msg) => console.log(`  [page:${msg.type()}] ${msg.text()}`));
    page.on("pageerror", (err) => console.log(`  [page:error] ${err.message}`));

    const appUrl = `http://${APP_HOST}:${APP_PORT}/`;
    console.log(`▶ loading ${appUrl}`);
    await page.goto(appUrl, { waitUntil: "load", timeout: 30_000 });

    let anyFailed = false;
    for (const transportName of TRANSPORTS) {
      console.log(`\n── ${transportName} ───────────────────────────────`);
      let result;
      try {
        result = await page.evaluate(inPageDrive, {
          transportName,
          host: JT_HOST,
          port: JT_PORT,
          connectTimeoutMs: CONNECT_TIMEOUT_MS,
          sendPollMs: SEND_POLL_MS,
        });
      } catch (err) {
        anyFailed = true;
        fail(`${transportName}: connect failed: ${err.message || err}`);
        continue;
      }

      if (result.skipped) {
        console.log(`⚠️  ${transportName} skipped: ${result.reason}`);
        anyFailed = true; // a requested transport that can't run is a failure
        fail(`${transportName} was requested but could not run (${result.reason})`);
        continue;
      }

      console.log(`   ${JSON.stringify(result)}`);
      if (!result.connected) {
        anyFailed = true;
        fail(`${transportName}: transport did not report a connected state`);
      } else if (!(result.sentSamples > 0)) {
        anyFailed = true;
        fail(`${transportName}: no captured audio reached the send ring buffer`);
      } else {
        console.log(
          `✅ ${transportName}: connected; sent ${result.sentSamples} samples ` +
            `(${result.ringWrites} writes); recv played=${result.recvPlayed} ` +
            `initialized=${result.recvInitialized}`,
        );
      }
    }

    if (anyFailed) throw new Error("one or more transports failed");
    console.log("\n✅ integration tests passed");
  } finally {
    if (browser) await browser.close().catch(() => {});
    server.kill("SIGTERM");
  }
}

main().catch((err) => {
  fail(err.message || String(err));
  process.exitCode = 1;
});
