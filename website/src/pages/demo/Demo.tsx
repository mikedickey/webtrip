import { useEffect, useRef, useState } from "react";
import {
  getDemoEngine,
  isIOS,
  type AudioDevices,
  type DemoEngine,
  type DeviceInfo,
  type SessionState,
  type WebTripSession,
} from "../../lib/webtrip";
import LevelMeter from "./LevelMeter";
import Slider from "./Slider";
import StatsPanel from "./StatsPanel";
import "./demo.css";

type Phase = "loading" | "ready" | "mic-error" | "engine-error";
type TransportChoice = "auto" | "webrtc" | "webtransport";
type CaptureMode = "mixToMono" | "stereo" | "mono";

const SESSION_STATE_LABELS: Record<SessionState, string> = {
  idle: "Not Connected",
  connecting: "Connecting to Server...",
  negotiating: "Negotiating WebRTC...",
  connected: "Connected",
  error: "Connection Error",
};

// The capture button cycles through these in order. Each mode fixes how many
// input device channels are captured and how many are sent; the library's
// send-side channel mapping does the rest (2 captured → 1 sent averages them,
// and 1 captured keeps only the device's first channel).
const CAPTURE_MODES: Record<
  CaptureMode,
  { next: CaptureMode; line1: string; line2: string; inputChannels: number; sendChannels: number }
> = {
  mixToMono: { next: "stereo", line1: "Mix to Mono", line2: "2 → 1 ch", inputChannels: 2, sendChannels: 1 },
  stereo: { next: "mono", line1: "Stereo", line2: "2 ch", inputChannels: 2, sendChannels: 2 },
  mono: { next: "mixToMono", line1: "Mono", line2: "1st ch", inputChannels: 1, sendChannels: 1 },
};

// The demo always asks the peer for stereo; the library maps whatever the
// peer actually sends onto the output device.
const RECEIVE_CHANNELS = 2;

// Wrap connectToStudio in a 45-second timeout so the UI never hangs forever.
// The most common cause on iOS is AudioContext.resume() waiting for a user
// gesture that never comes — the fire-and-forget fix in engine.rs prevents the
// hang, but the timeout is kept as a safety net for any other async path that
// might stall.
const CONNECT_TIMEOUT_MS = 45_000;

// The session is a module-level singleton (see getDemoEngine), so a
// disconnect can still be tearing down when the next connect is attempted.
// Reconnecting before teardown quiesces races the regulator's shared
// sequence-number state (see WebTripSession::disconnect), so these live at
// module scope — matching the session's lifetime — and handleConnect awaits
// pendingDisconnect before dialing.
let pendingDisconnect: Promise<void> | null = null;

// The last connectToStudio call (always stored pre-caught). connectToStudio
// is not cancellable, so every teardown chains after the connect settles —
// otherwise the link could finish (and deferred capture start) behind a
// racing disconnect, or with no demo UI mounted to tear it down.
let connectInFlight: Promise<void> | null = null;

// Synchronous re-entrancy guard for handleConnect: the React `busy` state
// disables the button only after a re-render, so two rapid clicks could both
// dial the singleton session (connect_to_studio has no in-flight guard).
let dialing = false;

// Every teardown goes through here: it chains behind any uncancellable
// in-flight connect (instant when none is pending or it already settled) and
// behind any teardown already running (disconnect() calls on the session must
// not overlap), so the next connect can await the whole chain via
// pendingDisconnect.
function beginDisconnect(session: WebTripSession) {
  const priorConnect = connectInFlight; // stored pre-caught
  const prior = pendingDisconnect;
  pendingDisconnect = (async () => {
    if (priorConnect) await priorConnect;
    if (prior) await prior.catch(() => {});
    await session.disconnect();
  })();
  pendingDisconnect.catch(() => {});
}

// Keep the current selection if the device is still present, otherwise fall
// back to the first listed device.
function pickDevice(devices: DeviceInfo[], currentId: string): string {
  return devices.some((d) => d.deviceId === currentId)
    ? currentId
    : (devices[0]?.deviceId ?? "");
}

function ToggleButton({
  active,
  line1,
  line2,
  onClick,
  disabled = false,
}: {
  active: boolean;
  line1: string;
  line2: string;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      className={`toggle-btn-compact${active ? " active" : ""}`}
      onClick={onClick}
      disabled={disabled}
    >
      <span className="toggle-line1">{line1}</span>
      <span className="toggle-line2">{line2}</span>
    </button>
  );
}

export default function Demo() {
  const [phase, setPhase] = useState<Phase>("loading");
  const [engine, setEngine] = useState<DemoEngine | null>(null);
  const [devices, setDevices] = useState<AudioDevices | null>(null);
  const [sessionState, setSessionState] = useState<SessionState>("idle");
  const [busy, setBusy] = useState(false);

  const [serverHost, setServerHost] = useState(window.location.hostname || "localhost");
  const [serverPort, setServerPort] = useState("4464");
  const [clientName, setClientName] = useState("");
  const [transportChoice, setTransportChoice] = useState<TransportChoice>("auto");
  const [activeTransport, setActiveTransport] = useState<"webrtc" | "webtransport">("webrtc");
  const [inputDeviceId, setInputDeviceId] = useState("");
  const [outputDeviceId, setOutputDeviceId] = useState("");

  // Channel counts of the selected devices, probed before connecting.
  // undefined means "not yet known" (a probe is in flight).
  const [inputDeviceChannels, setInputDeviceChannels] = useState<number | undefined>(undefined);
  const [outputDeviceChannels, setOutputDeviceChannels] = useState<number | undefined>(undefined);
  // Bumped on `devicechange` so the probes re-run even when the selected ids
  // are unchanged: "default" may now point at different hardware.
  const [probeNonce, setProbeNonce] = useState(0);

  const [agc, setAgc] = useState(false);
  const [echo, setEcho] = useState(false);
  const [noise, setNoise] = useState(false);
  // The user's choice for multi-channel inputs. A 1-channel input always
  // captures Mono without overwriting this, so switching back to a
  // multi-channel device restores the choice.
  const [captureMode, setCaptureMode] = useState<CaptureMode>("mixToMono");

  const [inputGain, setInputGain] = useState(0);
  const [outputVolume, setOutputVolume] = useState(100);
  const [monitorVolume, setMonitorVolume] = useState(0);

  const [showIosPrompt, setShowIosPrompt] = useState(false);

  const engineRef = useRef<DemoEngine | null>(null);

  // Lets async handlers skip user-facing work after navigating away
  // mid-operation: state setters are no-ops on an unmounted tree, but
  // alert() is not, and post-connect setup would be wasted on a session the
  // unmount cleanup is already tearing down.
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  // Bumped by the retry button on the engine-error screen; getDemoEngine()
  // clears its cache on failure, so re-running the effect starts a fresh load.
  const [loadAttempt, setLoadAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      let eng: DemoEngine;
      try {
        eng = await getDemoEngine();
      } catch (error) {
        console.error("Failed to load WebTrip engine:", error);
        if (!cancelled) setPhase("engine-error");
        return;
      }
      if (cancelled) return;
      // Expose the engine to the unmount cleanup before any further awaits,
      // so navigating away during the device permission prompt still chains
      // the teardown (disconnect on an idle session is a no-op).
      engineRef.current = eng;
      eng.session.set_on_state_change((state: string) => {
        // Ignore stale regressions that can arrive from late transport
        // callbacks after we've already reached connected.
        setSessionState((prev) =>
          prev === "connected" && (state === "connecting" || state === "negotiating")
            ? prev
            : (state as SessionState),
        );
      });
      // The singleton session may still be connected (or mid-teardown) from a
      // previous mount; sync the UI to its real state instead of assuming idle,
      // including which transport the live session is actually using.
      if (eng.session.isConnected()) {
        setSessionState("connected");
        setActiveTransport(
          eng.session.getTransportType() === eng.m.TransportType.WebTransport
            ? "webtransport"
            : "webrtc",
        );
      } else {
        setSessionState("idle");
      }
      try {
        const devs = (await eng.m.getAudioDevices()) as AudioDevices;
        if (cancelled) return;
        setEngine(eng);
        setDevices(devs);
        setInputDeviceId(pickDevice(devs.inputDevices, ""));
        setOutputDeviceId(pickDevice(devs.outputDevices, ""));
        setPhase("ready");
      } catch (error) {
        console.error(error);
        if (!cancelled) setPhase("mic-error");
      }
    })().catch(console.error);
    return () => {
      cancelled = true;
      const session = engineRef.current?.session;
      if (session) beginDisconnect(session);
    };
  }, [loadAttempt]);

  // Re-list devices when hardware is plugged in or removed, falling back to
  // the first device if a selected one disappeared, and re-probe channels.
  useEffect(() => {
    if (!engine) return;
    const mediaDevices = navigator.mediaDevices;
    let cancelled = false;
    const handleDeviceChange = async () => {
      try {
        const devs = (await engine.m.getAudioDevices()) as AudioDevices;
        if (cancelled) return;
        setDevices(devs);
        setInputDeviceId((current) => pickDevice(devs.inputDevices, current));
        setOutputDeviceId((current) => pickDevice(devs.outputDevices, current));
        setProbeNonce((nonce) => nonce + 1);
      } catch (error) {
        console.error("Failed to refresh audio devices:", error);
      }
    };
    mediaDevices?.addEventListener("devicechange", handleDeviceChange);
    return () => {
      cancelled = true;
      mediaDevices?.removeEventListener("devicechange", handleDeviceChange);
    };
  }, [engine]);

  // Probe the selected input device's channel count. If the probe itself
  // fails, assume 2: capture clamps to what the device really grants, so this
  // never over-requests.
  useEffect(() => {
    if (!engine) return;
    let cancelled = false;
    setInputDeviceChannels(undefined);
    engine.m
      .getInputDeviceChannels(inputDeviceId || undefined)
      .catch((error: unknown) => {
        console.warn("Failed to probe input device channels:", error);
        return 2;
      })
      .then((channels) => {
        if (!cancelled) setInputDeviceChannels(channels);
      });
    return () => {
      cancelled = true;
    };
  }, [engine, inputDeviceId, probeNonce]);

  // Probe the selected output device's channel count. A failed probe assumes
  // 2 for the same reason as the input probe: playback clamps to the device.
  useEffect(() => {
    if (!engine) return;
    let cancelled = false;
    setOutputDeviceChannels(undefined);
    engine.m
      .getOutputDeviceChannels(outputDeviceId || undefined)
      .catch((error: unknown) => {
        console.warn("Failed to probe output device channels:", error);
        return 2;
      })
      .then((channels) => {
        if (!cancelled) setOutputDeviceChannels(channels);
      });
    return () => {
      cancelled = true;
    };
  }, [engine, outputDeviceId, probeNonce]);

  const webTransportAvailable = engine?.m.WebTripSession.isWebTransportAvailable() ?? false;

  // Keep the session's transport in sync with the selector ("auto" resolves
  // to WebTransport when available, else WebRTC). Applied only while idle:
  // setTransportType is ignored in any other state, and the label must keep
  // reflecting the transport the session is actually connecting with or using.
  useEffect(() => {
    if (!engine || sessionState !== "idle") return;
    const resolved =
      transportChoice === "auto"
        ? webTransportAvailable
          ? "webtransport"
          : "webrtc"
        : transportChoice;
    engine.session.setTransportType(
      resolved === "webtransport" ? engine.m.TransportType.WebTransport : engine.m.TransportType.WebRTC,
    );
    setActiveTransport(resolved);
  }, [engine, transportChoice, webTransportAvailable, sessionState]);

  // An error state from a transport callback means the connection dropped:
  // tear the session down so it returns to a reconnectable idle state.
  useEffect(() => {
    const session = engineRef.current?.session;
    if (sessionState === "error" && session) beginDisconnect(session);
  }, [sessionState]);

  const handleConnect = async () => {
    if (!engine || dialing || inputDeviceChannels === undefined || outputDeviceChannels === undefined) {
      return;
    }
    const host = serverHost.trim();
    if (!host) {
      alert("Please enter a server host.");
      return;
    }
    const port = parseInt(serverPort, 10) || 4464;
    const outputChannels = outputDeviceChannels >= 2 ? 2 : 1;
    const mode = CAPTURE_MODES[inputDeviceChannels === 1 ? "mono" : captureMode];

    dialing = true;
    setBusy(true);
    let timer: number | undefined;
    try {
      // The teardown wait runs inside the raced promise so the timeout also
      // covers a disconnect that never settles.
      const connectPromise = (async () => {
        // Drain teardowns until none are pending: a new disconnect can be
        // registered while the previous one is being awaited, and it must not
        // be dropped or the connect below would race its teardown.
        while (pendingDisconnect) {
          const prior = pendingDisconnect;
          await prior.catch(() => {});
          if (pendingDisconnect === prior) pendingDisconnect = null;
        }
        // Apply the channel configuration now, while the session is Idle (the
        // setters are ignored in any other state). Applying it at connect time
        // also means a remount never has to re-sync the singleton session.
        engine.session.setReceiveChannels(RECEIVE_CHANNELS);
        engine.session.setOutputChannels(outputChannels);
        engine.session.setInputChannels(mode.inputChannels);
        engine.session.setSendChannels(mode.sendChannels);
        await engine.session.connectToStudio(
          host,
          port,
          inputDeviceId || undefined,
          agc,
          echo,
          noise,
          clientName.trim() || undefined,
        );
      })();
      // If the timeout wins, the connect may still settle later; storing it
      // pre-caught both tracks it for the unmount cleanup and keeps the loser
      // from firing a late unhandled rejection.
      connectInFlight = connectPromise.catch(() => {});
      await Promise.race([
        connectPromise,
        new Promise<never>((_, reject) => {
          timer = window.setTimeout(
            () => reject(new Error("Connection timed out — check server address and network.")),
            CONNECT_TIMEOUT_MS,
          );
        }),
      ]);
      if (!mountedRef.current) return;

      try {
        await engine.session.setOutputDevice(outputDeviceId || undefined);
      } catch (error) {
        // Don't fail the connection if output device setting fails.
        console.warn("Failed to set output device:", error);
      }

      // On iOS Safari, AudioContext.resume() requires a user gesture and may
      // not have resolved yet. If the context is still suspended, show a
      // one-time tap banner so the user can unlock audio output without
      // having to disconnect and reconnect.
      if (isIOS() && engine.session.isAudioSuspended()) {
        setShowIosPrompt(true);
      }
    } catch (error) {
      console.error("Failed to connect:", error);
      // connectToStudio failed before storing the transport, so the session is
      // still in "Connecting" state and needs to be reset to Idle.
      beginDisconnect(engine.session);
      if (mountedRef.current) alert(`Connection failed: ${error}`);
    } finally {
      dialing = false;
      clearTimeout(timer);
      setBusy(false);
    }
  };

  const handleDisconnect = () => {
    if (engine) beginDisconnect(engine.session);
  };

  const handleIosResume = async () => {
    try {
      await engine?.session.resumeAudio();
    } catch {
      // Best-effort; audio may still work via implicit unlock.
    } finally {
      setShowIosPrompt(false);
    }
  };

  if (phase === "loading") {
    return (
      <div className="demo-page">
        <div className="card loading">
          <div className="loading-spinner" />
          <div className="loading-text">Initializing audio...</div>
        </div>
      </div>
    );
  }

  if (phase === "engine-error") {
    return (
      <div className="demo-page">
        <div className="card error">
          <h2>Failed to Load Audio Engine</h2>
          <p>Check your network connection and try again.</p>
          <button
            className="action-btn primary"
            onClick={() => {
              setPhase("loading");
              setLoadAttempt((attempt) => attempt + 1);
            }}
          >
            Retry
          </button>
        </div>
      </div>
    );
  }

  if (phase === "mic-error" || !engine || !devices) {
    return (
      <div className="demo-page">
        <div className="card error">
          <h2>Microphone Access Required</h2>
          <p>Please allow microphone access and refresh the page.</p>
        </div>
      </div>
    );
  }

  const connected = sessionState === "connected";
  const inProgress = sessionState === "connecting" || sessionState === "negotiating";
  // Connection and device settings are applied at connect time, so they are
  // locked from the moment a connect starts until the session is back to
  // idle. The transient "error" state is included: its teardown is scheduled
  // by an effect, and a connect that sneaks in first would attach a transport
  // while the Rust state machine drops the Error → Connecting transition,
  // desyncing UI and session.
  const configLocked = busy || inProgress || connected || sessionState === "error";
  const deviceChannelsKnown = inputDeviceChannels !== undefined && outputDeviceChannels !== undefined;
  const statusLabel = connected
    ? `Connected (${activeTransport === "webtransport" ? "WebTransport" : "WebRTC"})`
    : SESSION_STATE_LABELS[sessionState] ?? sessionState;

  return (
    <div className="demo-page">
      <div className="card">
        {showIosPrompt && (
          <div
            className="ios-audio-prompt"
            role="button"
            tabIndex={0}
            onClick={handleIosResume}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") handleIosResume();
            }}
          >
            Tap here to enable audio output
          </div>
        )}

        <h1 className="title">WebTrip Demo</h1>
        <p className="subtitle">Real-time lossless audio streaming using WebAssembly</p>

        <div className="section-header">Studio Connection</div>
        <div className={`connection-status ${sessionState}`}>
          <span className="status-dot" />
          <span className="status-text">{statusLabel}</span>
        </div>

        {connected && <StatsPanel engine={engine} />}

        <div className="host-port-row">
          <div className="control-group host-group">
            <label className="label" htmlFor="server-host">Server Host</label>
            <input
              id="server-host"
              type="text"
              className="text-input"
              placeholder="studio.jacktrip.org"
              value={serverHost}
              disabled={configLocked}
              onChange={(e) => setServerHost(e.target.value)}
            />
          </div>
          <div className="control-group port-group">
            <label className="label" htmlFor="server-port">Port</label>
            <input
              id="server-port"
              type="number"
              className="text-input"
              placeholder="4464"
              value={serverPort}
              disabled={configLocked}
              onChange={(e) => setServerPort(e.target.value)}
            />
          </div>
        </div>

        <div className="control-group">
          <label className="label" htmlFor="client-name">Client Name (Optional)</label>
          <input
            id="client-name"
            type="text"
            className="text-input"
            placeholder="Leave empty for anonymous"
            value={clientName}
            disabled={configLocked}
            onChange={(e) => setClientName(e.target.value)}
          />
        </div>

        <div className="control-group">
          <label className="label" htmlFor="transport">Transport</label>
          <select
            id="transport"
            className="select"
            value={transportChoice}
            disabled={configLocked}
            onChange={(e) => setTransportChoice(e.target.value as TransportChoice)}
          >
            <option value="auto">Auto</option>
            <option value="webrtc">WebRTC</option>
            <option value="webtransport" disabled={!webTransportAvailable}>
              WebTransport{webTransportAvailable ? "" : " (Not Available)"}
            </option>
          </select>
        </div>

        <div className="connection-buttons">
          <button
            className="action-btn primary"
            disabled={configLocked || !deviceChannelsKnown}
            onClick={handleConnect}
          >
            {connected ? "Connected" : busy || inProgress ? "Connecting..." : "Connect to Studio"}
          </button>
          <button
            className="action-btn secondary"
            disabled={!connected && !inProgress}
            onClick={handleDisconnect}
          >
            Disconnect
          </button>
        </div>

        <div className="section-header">Audio Devices</div>
        <div className="control-group">
          <label className="label" htmlFor="input-device">Input Device</label>
          <select
            id="input-device"
            className="select"
            value={inputDeviceId}
            disabled={configLocked}
            onChange={(e) => setInputDeviceId(e.target.value)}
          >
            {devices.inputDevices.map((d) => (
              <option key={d.deviceId} value={d.deviceId}>
                {d.label || `Device ${d.deviceId.substring(0, 8)}`}
              </option>
            ))}
          </select>
        </div>
        <div className="control-group">
          <label className="label" htmlFor="output-device">Output Device</label>
          {devices.outputDevices.length > 0 ? (
            <select
              id="output-device"
              className="select"
              value={outputDeviceId}
              disabled={configLocked}
              onChange={(e) => setOutputDeviceId(e.target.value)}
            >
              {devices.outputDevices.map((d) => (
                <option key={d.deviceId} value={d.deviceId}>
                  {d.label || `Device ${d.deviceId.substring(0, 8)}`}
                </option>
              ))}
            </select>
          ) : (
            <>
              {/* iOS Safari (and some other mobile browsers) do not enumerate
                  audiooutput devices; show a disabled placeholder instead. */}
              <select
                id="output-device"
                className="select"
                disabled
                title="Output device selection is not supported on this browser"
              >
                <option>System Default</option>
              </select>
              <p className="device-note">Output routing is not available on this browser.</p>
            </>
          )}
        </div>

        <div className="section-header">Audio Processing</div>
        <div className="toggles-grid">
          <ToggleButton
            active={agc}
            line1="AGC"
            line2="Auto Gain"
            onClick={() => setAgc(!agc)}
            disabled={configLocked}
          />
          <ToggleButton
            active={echo}
            line1="Echo"
            line2="Cancellation"
            onClick={() => setEcho(!echo)}
            disabled={configLocked}
          />
          <ToggleButton
            active={noise}
            line1="Noise"
            line2="Suppression"
            onClick={() => setNoise(!noise)}
            disabled={configLocked}
          />
          {inputDeviceChannels === 1 ? (
            <ToggleButton active={false} line1="Mono" line2="1 ch" onClick={() => {}} disabled />
          ) : (
            inputDeviceChannels !== undefined && (
              <ToggleButton
                active={captureMode === "stereo"}
                line1={CAPTURE_MODES[captureMode].line1}
                line2={CAPTURE_MODES[captureMode].line2}
                onClick={() => setCaptureMode(CAPTURE_MODES[captureMode].next)}
                disabled={configLocked}
              />
            )
          )}
        </div>

        <div className="section-header">Gain Controls</div>
        <div className="sliders-container">
          <Slider
            label="Input Gain"
            min={-20}
            max={20}
            value={inputGain}
            display={`${inputGain >= 0 ? "+" : ""}${inputGain.toFixed(1)} dB`}
            onChange={(v) => {
              setInputGain(v);
              engine.m.setInputGainFromPtr(engine.paramsPtr, v);
            }}
          />
          <Slider
            label="Output Volume"
            min={0}
            max={100}
            value={outputVolume}
            display={`${Math.round(outputVolume)}%`}
            onChange={(v) => {
              setOutputVolume(v);
              engine.m.setOutputVolumeFromPtr(engine.paramsPtr, v / 100);
            }}
          />
          <Slider
            label="Monitor"
            min={0}
            max={100}
            value={monitorVolume}
            display={monitorVolume === 0 ? "Off" : `${Math.round(monitorVolume)}%`}
            onChange={(v) => {
              setMonitorVolume(v);
              engine.m.setMonitorVolumeFromPtr(engine.paramsPtr, v / 100);
            }}
          />
        </div>

        <LevelMeter engine={engine} active={connected} />
      </div>
    </div>
  );
}
