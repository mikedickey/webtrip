import { useEffect, useRef, useState } from "react";
import {
  getDemoEngine,
  isIOS,
  type AudioDevices,
  type DemoEngine,
  type DeviceInfo,
  type SessionState,
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
  connected: "Connected",
  error: "Connection Error",
  disconnecting: "Disconnecting...",
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

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

// Keep the current selection if the device is still present, otherwise fall
// back to the first listed device.
function pickDevice(devices: DeviceInfo[], currentId: string): string {
  return devices.some((d) => d.deviceId === currentId)
    ? currentId
    : (devices[0]?.deviceId ?? "");
}

// The channel count of a selected device, probed before connecting and again
// whenever the id or probe nonce changes. undefined means "not yet known" (a
// probe is in flight). A failed probe assumes 2: capture and playback clamp to
// what the device really grants, so this never over-requests.
//
// Probing is skipped while `locked`: counts are only applied at the next
// connect, and the input probe's getUserMedia on a device that is already
// capturing may disturb the live track. A probe skipped or cancelled while
// locked runs on unlock; one already completed for this id and nonce does not
// repeat.
function useDeviceChannels(
  probe: ((deviceId?: string) => Promise<number>) | undefined,
  deviceId: string,
  probeNonce: number,
  locked: boolean,
  kind: "input" | "output",
): number | undefined {
  const [channels, setChannels] = useState<number | undefined>(undefined);
  const probedKey = useRef<string | null>(null);
  useEffect(() => {
    const key = `${deviceId}\n${probeNonce}`;
    if (!probe || locked || probedKey.current === key) return;
    let cancelled = false;
    setChannels(undefined);
    probe(deviceId || undefined)
      .catch((error: unknown) => {
        console.warn(`Failed to probe ${kind} device channels:`, error);
        return 2;
      })
      .then((count) => {
        if (cancelled) return;
        probedKey.current = key;
        setChannels(count);
      });
    return () => {
      cancelled = true;
    };
  }, [probe, deviceId, probeNonce, locked, kind]);
  return channels;
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
  const [connectError, setConnectError] = useState<string | null>(null);

  const [serverHost, setServerHost] = useState(window.location.hostname || "localhost");
  const [serverPort, setServerPort] = useState("4464");
  const [clientName, setClientName] = useState("");
  const [transportChoice, setTransportChoice] = useState<TransportChoice>("auto");
  const [activeTransport, setActiveTransport] = useState<"webrtc" | "webtransport">("webrtc");
  const [inputDeviceId, setInputDeviceId] = useState("");
  const [outputDeviceId, setOutputDeviceId] = useState("");

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
      // Expose the engine to the unmount cleanup before any further awaits.
      engineRef.current = eng;
      eng.session.set_on_state_change((state: string) => setSessionState(state as SessionState));
      // The singleton session outlives this component and may still be
      // connected (or tearing down) from a previous mount; sync the UI to it,
      // including which transport it is using.
      setSessionState(eng.session.getState() as SessionState);
      setActiveTransport(
        eng.session.getTransportType() === eng.m.TransportType.WebTransport ? "webtransport" : "webrtc",
      );
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
      void engineRef.current?.session.disconnect();
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

  const connected = sessionState === "connected";
  const connecting = sessionState === "connecting";
  // Connection and device settings are applied at connect time, while the
  // session is idle, so they are locked from the moment a connect starts
  // until the session is back to idle.
  const configLocked = busy || sessionState !== "idle";

  const inputDeviceChannels = useDeviceChannels(
    engine?.m.getInputDeviceChannels,
    inputDeviceId,
    probeNonce,
    configLocked,
    "input",
  );
  const outputDeviceChannels = useDeviceChannels(
    engine?.m.getOutputDeviceChannels,
    outputDeviceId,
    probeNonce,
    configLocked,
    "output",
  );

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

  // An error state means an established connection dropped: report it and
  // tear the session down so it returns to a reconnectable idle state.
  useEffect(() => {
    const session = engineRef.current?.session;
    if (sessionState !== "error" || !session) return;
    setConnectError("Connection lost.");
    void session.disconnect();
  }, [sessionState]);

  const handleConnect = async () => {
    if (!engine || inputDeviceChannels === undefined || outputDeviceChannels === undefined) {
      return;
    }
    const host = serverHost.trim();
    if (!host) {
      setConnectError("Please enter a server host.");
      return;
    }
    const port = parseInt(serverPort, 10) || 4464;
    const outputChannels = outputDeviceChannels >= 2 ? 2 : 1;
    const mode = CAPTURE_MODES[inputDeviceChannels === 1 ? "mono" : captureMode];

    setConnectError(null);
    setBusy(true);
    try {
      // Apply the channel configuration now: Connect is only enabled while
      // the session is idle, the one state the setters take effect in.
      // Applying it at connect time also means a remount never has to re-sync
      // the singleton session.
      engine.session.setReceiveChannels(RECEIVE_CHANNELS);
      engine.session.setOutputChannels(outputChannels);
      engine.session.setInputChannels(mode.inputChannels);
      engine.session.setSendChannels(mode.sendChannels);
      // Store the sink before connecting so start_capture routes playback to
      // it during engine bring-up, instead of starting on the default output.
      await engine.session.setOutputDevice(outputDeviceId || undefined);
      await engine.session.connectToStudio(
        host,
        port,
        inputDeviceId || undefined,
        agc,
        echo,
        noise,
        clientName.trim() || undefined,
      );

      // On iOS Safari, AudioContext.resume() requires a user gesture and may
      // not have resolved yet. If the context is still suspended, show a
      // one-time tap banner so the user can unlock audio output without
      // having to disconnect and reconnect.
      if (isIOS() && engine.session.isAudioSuspended()) {
        setShowIosPrompt(true);
      }
    } catch (error) {
      // An AbortError is the connect being cancelled by our own disconnect.
      if (error instanceof Error && error.name === "AbortError") return;
      console.error("Failed to connect:", error);
      setConnectError(`Connection failed: ${errorMessage(error)}`);
    } finally {
      setBusy(false);
    }
  };

  const handleDisconnect = () => {
    void engine?.session.disconnect();
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
            {connected ? "Connected" : busy || connecting ? "Connecting..." : "Connect to Studio"}
          </button>
          <button
            className="action-btn secondary"
            disabled={!connected && !connecting}
            onClick={handleDisconnect}
          >
            Disconnect
          </button>
        </div>
        {connectError && (
          <p className="connect-error" role="alert">
            {connectError}
          </p>
        )}

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
