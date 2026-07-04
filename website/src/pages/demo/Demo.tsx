import { useEffect, useRef, useState } from "react";
import {
  getDemoEngine,
  isIOS,
  type AudioDevices,
  type DemoEngine,
  type SessionState,
} from "../../lib/webtrip";
import LevelMeter from "./LevelMeter";
import Slider from "./Slider";
import StatsPanel from "./StatsPanel";
import "./demo.css";

type Phase = "loading" | "ready" | "mic-error" | "engine-error";
type TransportChoice = "auto" | "webrtc" | "webtransport";

const SESSION_STATE_LABELS: Record<SessionState, string> = {
  idle: "Not Connected",
  connecting: "Connecting to Server...",
  negotiating: "Negotiating WebRTC...",
  connected: "Connected",
  error: "Connection Error",
};

// Wrap connectToStudio in a 45-second timeout so the UI never hangs forever.
// The most common cause on iOS is AudioContext.resume() waiting for a user
// gesture that never comes — the fire-and-forget fix in engine.rs prevents the
// hang, but the timeout is kept as a safety net for any other async path that
// might stall.
const CONNECT_TIMEOUT_MS = 45_000;

// The session is a module-level singleton (see getDemoEngine), so a
// disconnect started by an unmount can still be tearing down when the next
// mount connects. Reconnecting before teardown quiesces races the regulator's
// shared sequence-number state (see WebTripSession::disconnect), so the
// promise lives at module scope — matching the session's lifetime — and
// handleConnect awaits it before dialing.
let pendingDisconnect: Promise<void> | null = null;

function ToggleButton({
  active,
  line1,
  line2,
  onClick,
}: {
  active: boolean;
  line1: string;
  line2: string;
  onClick: () => void;
}) {
  return (
    <button className={`toggle-btn-compact${active ? " active" : ""}`} onClick={onClick}>
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

  const [agc, setAgc] = useState(false);
  const [echo, setEcho] = useState(false);
  const [noise, setNoise] = useState(false);
  const [stereo, setStereo] = useState(true);

  const [inputGain, setInputGain] = useState(0);
  const [outputVolume, setOutputVolume] = useState(100);
  const [monitorVolume, setMonitorVolume] = useState(0);

  const [showIosPrompt, setShowIosPrompt] = useState(false);

  const engineRef = useRef<DemoEngine | null>(null);

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
      // previous mount; sync the UI to its real state instead of assuming idle.
      setSessionState(eng.session.isConnected() ? "connected" : "idle");
      try {
        const devs = (await eng.m.getAudioDevices()) as AudioDevices;
        if (cancelled) return;
        engineRef.current = eng;
        setEngine(eng);
        setDevices(devs);
        setInputDeviceId(devs.inputDevices[0]?.deviceId ?? "");
        setOutputDeviceId(devs.outputDevices[0]?.deviceId ?? "");
        setPhase("ready");
      } catch (error) {
        console.error(error);
        if (!cancelled) setPhase("mic-error");
      }
    })().catch(console.error);
    return () => {
      cancelled = true;
      const session = engineRef.current?.session;
      if (session) pendingDisconnect = session.disconnect();
    };
  }, [loadAttempt]);

  const webTransportAvailable = engine?.m.WebTripSession.isWebTransportAvailable() ?? false;

  // Keep the session's transport in sync with the selector ("auto" resolves
  // to WebTransport when available, else WebRTC).
  useEffect(() => {
    if (!engine) return;
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
  }, [engine, transportChoice, webTransportAvailable]);

  // An error state from a transport callback means the connection dropped:
  // tear the session down so it returns to a reconnectable idle state.
  useEffect(() => {
    if (sessionState === "error") engineRef.current?.session.disconnect();
  }, [sessionState]);

  const handleConnect = async () => {
    if (!engine) return;
    const host = serverHost.trim();
    if (!host) {
      alert("Please enter a server host.");
      return;
    }
    const port = parseInt(serverPort, 10) || 4464;

    setBusy(true);
    let timer: number | undefined;
    try {
      if (pendingDisconnect) {
        await pendingDisconnect.catch(() => {});
        pendingDisconnect = null;
      }
      const connectPromise = engine.session.connectToStudio(
        host,
        port,
        inputDeviceId || undefined,
        agc,
        echo,
        noise,
        clientName.trim() || undefined,
      );
      // If the timeout wins, the connect may still settle later; swallow it so
      // the loser never fires a late unhandled rejection.
      connectPromise.catch(() => {});
      await Promise.race([
        connectPromise,
        new Promise<never>((_, reject) => {
          timer = window.setTimeout(
            () => reject(new Error("Connection timed out — check server address and network.")),
            CONNECT_TIMEOUT_MS,
          );
        }),
      ]);

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
      engine.session.disconnect();
      alert(`Connection failed: ${error}`);
    } finally {
      clearTimeout(timer);
      setBusy(false);
    }
  };

  const handleDisconnect = () => {
    engine?.session.disconnect();
  };

  const handleOutputDeviceChange = async (deviceId: string) => {
    const previousDeviceId = outputDeviceId;
    setOutputDeviceId(deviceId);
    if (!engine) return;
    try {
      await engine.session.setOutputDevice(deviceId || undefined);
    } catch (error) {
      console.error("Failed to set output device:", error);
      alert(`Failed to change output device: ${error}`);
      setOutputDeviceId(previousDeviceId);
    }
  };

  const handleStereoToggle = () => {
    const next = !stereo;
    setStereo(next);
    engine?.session.setChannels(next ? 2 : 1);
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
            onChange={(e) => setClientName(e.target.value)}
          />
        </div>

        <div className="control-group">
          <label className="label" htmlFor="transport">Transport</label>
          <select
            id="transport"
            className="select"
            value={transportChoice}
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
            disabled={busy || inProgress || connected}
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
              onChange={(e) => handleOutputDeviceChange(e.target.value)}
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
          <ToggleButton active={agc} line1="AGC" line2="Auto Gain" onClick={() => setAgc(!agc)} />
          <ToggleButton
            active={echo}
            line1="Echo"
            line2="Cancellation"
            onClick={() => setEcho(!echo)}
          />
          <ToggleButton
            active={noise}
            line1="Noise"
            line2="Suppression"
            onClick={() => setNoise(!noise)}
          />
          <ToggleButton
            active={stereo}
            line1={stereo ? "Stereo" : "Mono"}
            line2={stereo ? "2 Channels" : "1 Channel"}
            onClick={handleStereoToggle}
          />
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
