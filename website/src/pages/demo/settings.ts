import type { AudioDevices, DeviceInfo } from "../../lib/webtrip";

export type TransportChoice = "auto" | "webrtc" | "webtransport";
export type CaptureMode = "mixToMono" | "stereo" | "mono";

// Server host, port, client name and transport: restored whenever saved.
export interface ConnectionSettings {
  serverHost: string;
  serverPort: string;
  clientName: string;
  transportChoice: TransportChoice;
}

// Device selection plus everything tuned for those devices: restored only
// when both saved devices are still present, otherwise all reset to defaults.
export interface DeviceSettings {
  inputDeviceId: string;
  outputDeviceId: string;
  agc: boolean;
  echo: boolean;
  noise: boolean;
  captureMode: CaptureMode;
  inputGain: number;
  outputVolume: number;
  monitorVolume: number;
}

export type Settings = ConnectionSettings & DeviceSettings;

const STORAGE_KEY = "webtrip.demo.settings";

const TRANSPORT_CHOICES: readonly TransportChoice[] = ["auto", "webrtc", "webtransport"];
const CAPTURE_MODE_CHOICES: readonly CaptureMode[] = ["mixToMono", "stereo", "mono"];

// Keep the current selection if the device is still present, otherwise fall
// back to the first listed device.
export function pickDevice(devices: DeviceInfo[], currentId: string): string {
  return devices.some((d) => d.deviceId === currentId)
    ? currentId
    : (devices[0]?.deviceId ?? "");
}

export function defaultSettings(devices: AudioDevices): Settings {
  return {
    serverHost: window.location.hostname || "localhost",
    serverPort: "4464",
    clientName: "",
    transportChoice: "auto",
    inputDeviceId: pickDevice(devices.inputDevices, ""),
    outputDeviceId: pickDevice(devices.outputDevices, ""),
    agc: false,
    echo: false,
    noise: false,
    captureMode: "mixToMono",
    inputGain: 0,
    outputVolume: 100,
    monitorVolume: 0,
  };
}

// Copies the fields of `raw` whose type matches the corresponding field of
// `fallback` (and, for enums, whose value is allowed); everything else keeps
// the fallback. Guards against hand-edited or stale stored values.
function sanitize(raw: unknown, fallback: Settings): Settings {
  const r = (typeof raw === "object" && raw !== null ? raw : {}) as Record<string, unknown>;
  const str = (k: keyof Settings) => (typeof r[k] === "string" ? (r[k] as string) : (fallback[k] as string));
  const bool = (k: keyof Settings) => (typeof r[k] === "boolean" ? (r[k] as boolean) : (fallback[k] as boolean));
  const num = (k: keyof Settings) =>
    typeof r[k] === "number" && Number.isFinite(r[k]) ? (r[k] as number) : (fallback[k] as number);
  const oneOf = <T extends string>(k: keyof Settings, allowed: readonly T[]) =>
    allowed.includes(r[k] as T) ? (r[k] as T) : (fallback[k] as T);
  return {
    serverHost: str("serverHost"),
    serverPort: str("serverPort"),
    clientName: str("clientName"),
    transportChoice: oneOf("transportChoice", TRANSPORT_CHOICES),
    inputDeviceId: str("inputDeviceId"),
    outputDeviceId: str("outputDeviceId"),
    agc: bool("agc"),
    echo: bool("echo"),
    noise: bool("noise"),
    captureMode: oneOf("captureMode", CAPTURE_MODE_CHOICES),
    inputGain: num("inputGain"),
    outputVolume: num("outputVolume"),
    monitorVolume: num("monitorVolume"),
  };
}

// Browsers that cannot enumerate output devices (iOS Safari) list none, so the
// only selection that can have been saved there is the empty one.
function deviceAvailable(list: DeviceInfo[], id: string): boolean {
  return list.length === 0 ? id === "" : list.some((d) => d.deviceId === id);
}

// Settings for the first render of a visit: saved connection settings always
// apply; the device-dependent group applies only if both saved devices are
// present now, otherwise it is entirely defaults.
export function resolveSettings(saved: unknown, devices: AudioDevices): Settings {
  const defaults = defaultSettings(devices);
  if (saved == null) return defaults;
  const s = sanitize(saved, defaults);
  const devicesRestored =
    deviceAvailable(devices.inputDevices, s.inputDeviceId) &&
    deviceAvailable(devices.outputDevices, s.outputDeviceId);
  if (devicesRestored) return s;
  return {
    ...defaults,
    serverHost: s.serverHost,
    serverPort: s.serverPort,
    clientName: s.clientName,
    transportChoice: s.transportChoice,
  };
}

// Storage can be unavailable or throw (private windows, blocked site data), so
// every access is best-effort: the demo works the same without persistence.
export function loadSavedSettings(): unknown {
  try {
    const text = localStorage.getItem(STORAGE_KEY);
    return text === null ? null : JSON.parse(text);
  } catch {
    return null;
  }
}

export function saveSettings(settings: Settings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // Persistence is best-effort.
  }
}
