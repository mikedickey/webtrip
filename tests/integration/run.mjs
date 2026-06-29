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
//   INTEGRATION_COVERAGE    if set, dump LLVM coverage to coverage/integration.profraw
//                           (requires a `build:wasm:coverage` build)
//   PUPPETEER_EXECUTABLE_PATH  Chrome/Chromium binary (auto-detected otherwise)
//
// Failure-case mode (opt-in, kept OFF by default so it can't flake the
// happy-path assertion run — see the T30 guardrail):
//   INTEGRATION_FAILURE_CASE=1   run ONLY the connect-failure check (and skip
//                                the happy path; no live JackTrip server needed)
//   FAILURE_TRANSPORT            transport to drive in failure mode (default webrtc)
//   FAILURE_HOST / FAILURE_PORT  the unreachable/garbage endpoint
//                                (default 127.0.0.1 : 1)

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

// Opt-in connect-failure mode (see the env-knobs header). Gated so the primary
// run is unaffected: when set, we run ONLY the failure check.
const FAILURE_CASE = ["1", "true", "yes"].includes(
  (process.env.INTEGRATION_FAILURE_CASE || "").toLowerCase(),
);
const FAILURE_TRANSPORT = process.env.FAILURE_TRANSPORT || "webrtc";
const FAILURE_HOST = process.env.FAILURE_HOST || "127.0.0.1";
// A normal, almost-certainly-closed high port: connection is refused fast (and,
// unlike Chrome's "unsafe" low ports e.g. 1, the wss:// attempt actually runs
// and closes with code 1006, so the connect rejects promptly rather than only
// via the outer timeout).
const FAILURE_PORT = parseInt(process.env.FAILURE_PORT || "48462", 10);
// The unreachable endpoint refuses fast; cap the wait well under the happy-path
// budget so a hung connect surfaces as a failure rather than stalling the job.
const FAILURE_CONNECT_TIMEOUT_MS = 20_000;

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
 * Connect-failure check (opt-in). Drives one transport at an unreachable/garbage
 * endpoint via the same `inPageDrive` and asserts the connect REJECTS within the
 * timeout (so the UI surfaces an error instead of hanging). Reuses the served
 * page; needs no live JackTrip server. Sets a non-zero exit code on a wrong
 * outcome (connect unexpectedly resolved / reported connected).
 */
async function runFailureCase(page) {
  console.log(
    `\n── failure-case (${FAILURE_TRANSPORT} → ${FAILURE_HOST}:${FAILURE_PORT}) ─────`,
  );
  let result;
  try {
    result = await page.evaluate(inPageDrive, {
      transportName: FAILURE_TRANSPORT,
      host: FAILURE_HOST,
      port: FAILURE_PORT,
      connectTimeoutMs: FAILURE_CONNECT_TIMEOUT_MS,
      sendPollMs: 0,
    });
  } catch (err) {
    // Expected: the connect rejected (or timed out) — the failure path works.
    console.log(`✅ failure-case: connect rejected as expected: ${err.message || err}`);
    return;
  }

  if (result && result.skipped) {
    fail(`failure-case: ${FAILURE_TRANSPORT} could not run (${result.reason})`);
  } else if (result && result.connected) {
    fail(
      `failure-case: expected connect to ${FAILURE_HOST}:${FAILURE_PORT} to fail, ` +
        `but the transport reported connected`,
    );
  } else {
    fail(
      `failure-case: expected connect to ${FAILURE_HOST}:${FAILURE_PORT} to reject, ` +
        `but it resolved without connecting: ${JSON.stringify(result)}`,
    );
  }
  throw new Error("failure-case did not reject as expected");
}

/**
 * Runs inside the page. Serializes the WASM module's accumulated LLVM coverage
 * counters as `.profraw` bytes, base64-encoded for a JSON-safe transfer back to
 * Node. The counters live in shared linear memory, so this single main-thread
 * call also captures the WebTransport worker's execution. Requires a build with
 * the `coverage` feature (see `build:wasm:coverage`).
 */
async function inPageDumpCoverage() {
  const m = await import("/pkg/webtrip.js");
  if (typeof m.__coverageDump !== "function") {
    return { error: "module was not built with the `coverage` feature" };
  }
  const bytes = m.__coverageDump();
  let bin = "";
  for (let i = 0; i < bytes.length; i++) bin += String.fromCharCode(bytes[i]);
  return { base64: btoa(bin), length: bytes.length };
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

    // The failure-case run points at an unreachable endpoint on purpose, so it
    // must NOT wait for a live JackTrip server (there may be none).
    if (!FAILURE_CASE) {
      console.log(`▶ verifying JackTrip server at ${JT_HOST}:${JT_PORT}`);
      await waitForPort(JT_HOST, JT_PORT, 30_000);
    }

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

    // Opt-in: run ONLY the connect-failure check, then stop (kept separate from
    // the happy-path assertions so it can't destabilize them).
    if (FAILURE_CASE) {
      await runFailureCase(page);
      console.log("\n✅ failure-case passed");
      return;
    }

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

    // Pull coverage out of the live module before tearing the page down. Done
    // regardless of pass/fail so a partial run still yields the lines it hit.
    if (process.env.INTEGRATION_COVERAGE) {
      console.log("\n▶ dumping WASM coverage");
      const cov = await page.evaluate(inPageDumpCoverage);
      if (cov.error) {
        anyFailed = true;
        fail(`coverage dump: ${cov.error}`);
      } else {
        const outFile = path.join(REPO_ROOT, "coverage", "integration.profraw");
        fs.mkdirSync(path.dirname(outFile), { recursive: true });
        fs.writeFileSync(outFile, Buffer.from(cov.base64, "base64"));
        console.log(`   wrote ${cov.length} bytes → ${outFile}`);
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
