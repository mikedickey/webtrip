// Runtime loader for the wasm-pack output (pkg/ at the repo root, served at
// /pkg/). The module is deliberately NOT bundled by Vite: the WebTransport
// worker re-imports the glue from `{origin}/pkg/webtrip.js` at runtime (see
// src/audio/webtransport.rs::wasm_module_url), so the main thread must load
// that same URL rather than a Vite-hashed copy. Routing the specifier through
// a variable keeps TypeScript and Vite from resolving it at build time; types
// come from the wasm-pack-generated declarations instead.
import type { DeviceInfo, WebTripSession } from "../../../pkg/webtrip";

export type WebtripModule = typeof import("../../../pkg/webtrip");
export type { DeviceInfo, WebTripSession };

export interface AudioDevices {
  inputDevices: DeviceInfo[];
  outputDevices: DeviceInfo[];
}

export type SessionState =
  | "idle"
  | "connecting"
  | "negotiating"
  | "connected"
  | "error";

export interface DemoEngine {
  m: WebtripModule;
  paramsPtr: number;
  session: WebTripSession;
}

const WEBTRIP_PKG_URL = "/pkg/webtrip.js";

// One engine per document: the wasm module can only be instantiated once
// against its shared memory, and AudioParams allocations are never freed, so
// remounting the demo route reuses the same session (which supports repeated
// connect/disconnect cycles).
let enginePromise: Promise<DemoEngine> | null = null;

export function getDemoEngine(): Promise<DemoEngine> {
  enginePromise ??= (async () => {
    try {
      const m = (await import(/* @vite-ignore */ WEBTRIP_PKG_URL)) as WebtripModule;
      await m.default();
      m.init();
      const paramsPtr = m.createAudioParams();
      return { m, paramsPtr, session: new m.WebTripSession(paramsPtr) };
    } catch (error) {
      // Don't cache a rejected promise: a transient failure (e.g. fetching
      // /pkg/webtrip.js) would otherwise poison every future load attempt.
      enginePromise = null;
      throw error;
    }
  })();
  return enginePromise;
}

/** Detect iOS / iPadOS (including iPad on iOS 13+ which reports as "Macintosh"). */
export function isIOS(): boolean {
  return (
    /iPad|iPhone|iPod/.test(navigator.userAgent) ||
    (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1)
  );
}
