// WebTrip Audio - UI Controller
//
// This file handles ONLY UI interactions. All audio and network
// logic is handled in Rust via WebTripSession.

import init, {
  init as wasmInit,
  createAudioParams,
  getVolumeLevelFromPtr,
  getDbLevelFromPtr,
  getPeakLevelFromPtr,
  getPeakDbLevelFromPtr,
  setInputGainFromPtr,
  setOutputVolumeFromPtr,
  setMonitorVolumeFromPtr,
  getCallbackCountFromPtr,
  DeviceInfo,
  getAudioDevices,
  WebTripSession,
  TransportType,
} from "../pkg/webtrip.js";

interface AudioDevices {
  inputDevices: DeviceInfo[];
  outputDevices: DeviceInfo[];
}

type SessionState =
  | "idle"
  | "connecting"
  | "negotiating"
  | "connected"
  | "error";

class WebTripApp {
  private paramsPtr: number = 0;
  private session: WebTripSession | null = null;
  private animationFrameId: number | null = null;
  private statsIntervalId: number | null = null;
  private sessionState: SessionState = "idle";
  
  // Callback rate tracking
  private lastCallbackCount: bigint = 0n;
  private lastCallbackTime: number = 0;
  private callbackRate: number = 0;
  
  // Ring buffer rate tracking
  private lastRingWrites: bigint = 0n;
  private lastRingSamples: bigint = 0n;
  private ringWriteRate: number = 0;
  private avgSamplesPerWrite: number = 0;

  // UI Elements
  private inputSelect!: HTMLSelectElement;
  private outputSelect!: HTMLSelectElement;
  private meterFill!: HTMLDivElement;
  private peakIndicator!: HTMLDivElement;
  private peakDbDisplay!: HTMLDivElement;
  private inputGainSlider!: HTMLInputElement;
  private inputGainValue!: HTMLSpanElement;
  private outputVolumeSlider!: HTMLInputElement;
  private outputVolumeValue!: HTMLSpanElement;
  private monitorVolumeSlider!: HTMLInputElement;
  private monitorVolumeValue!: HTMLSpanElement;
  private connectionStatus!: HTMLDivElement;
  private serverHostInput!: HTMLInputElement;
  private serverPortInput!: HTMLInputElement;
  private clientNameInput!: HTMLInputElement;
  private connectButton!: HTMLButtonElement;
  private disconnectButton!: HTMLButtonElement;
  private statsDisplay!: HTMLDivElement;
  private toggleButtons: Map<string, HTMLButtonElement> = new Map();
  private transportSelect!: HTMLSelectElement;
  private activeTransportId: "webrtc" | "webtransport" | "mock" = "webrtc";

  async init(): Promise<void> {
    // Initialize WASM module
    await init();
    wasmInit();

    // Create shared audio params
    this.paramsPtr = createAudioParams();

    // Create session (handles all audio and network logic)
    this.session = new WebTripSession(this.paramsPtr);
    this.setupSessionCallbacks();

    try {
      const devices = (await getAudioDevices()) as AudioDevices;
      this.createUI(devices.inputDevices, devices.outputDevices);
      this.startVolumeAnimation();
    } catch (error) {
      this.showError(
        "Microphone Access Required",
        "Please allow microphone access and refresh the page."
      );
      throw error;
    }
  }

  private setupSessionCallbacks(): void {
    if (!this.session) return;

    // State change callback
    this.session.set_on_state_change((state: string) => {
      this.updateConnectionStatus(state as SessionState);
    });
  }

  // ==================== UI Creation ====================

  private createUI(
    inputDevices: DeviceInfo[],
    outputDevices: DeviceInfo[]
  ): void {
    const app = document.getElementById("app")!;
    app.innerHTML = "";

    const card = this.createElement("div", "card");

    // Title
    const title = this.createElement("h1", "title");
    title.textContent = "WebTrip Demo";
    card.appendChild(title);

    const subtitle = this.createElement("p", "subtitle");
    subtitle.textContent = "Real-time lossless audio streaming using WebAssembly";
    card.appendChild(subtitle);

    // Connection Section
    this.createConnectionSection(card);

    // Device Selection
    this.createDeviceSection(card, inputDevices, outputDevices);

    // Audio Processing Toggles
    this.createProcessingSection(card);

    // Gain Controls
    this.createGainSection(card);

    // Volume Meter
    this.createMeterSection(card);

    // Start/Stop Button
    this.createButtonSection(card);

    app.appendChild(card);
  }

  private createConnectionSection(card: HTMLElement): void {
    const header = this.createElement("div", "section-header");
    header.textContent = "Studio Connection";
    card.appendChild(header);

    // Connection status
    this.connectionStatus = this.createElement(
      "div",
      "connection-status"
    ) as HTMLDivElement;
    this.connectionStatus.innerHTML =
      '<span class="status-dot"></span><span class="status-text">Not Connected</span>';
    card.appendChild(this.connectionStatus);

    // Stats display
    this.statsDisplay = this.createElement(
      "div",
      "stats-display"
    ) as HTMLDivElement;
    this.statsDisplay.style.display = "none";
    card.appendChild(this.statsDisplay);

    // Server host and port - inline
    const hostPortRow = this.createElement("div", "host-port-row");
    hostPortRow.style.display = "flex";
    hostPortRow.style.gap = "1rem";
    hostPortRow.style.alignItems = "flex-end";

    // Server host input
    const hostGroup = this.createElement("div", "control-group");
    hostGroup.style.flex = "1";
    const hostLabel = this.createElement("label", "label");
    hostLabel.textContent = "Server Host";
    this.serverHostInput = document.createElement("input");
    this.serverHostInput.type = "text";
    this.serverHostInput.className = "text-input";
    this.serverHostInput.placeholder = "studio.jacktrip.org";
    this.serverHostInput.value = "localhost.miked.io";
    hostGroup.appendChild(hostLabel);
    hostGroup.appendChild(this.serverHostInput);
    hostPortRow.appendChild(hostGroup);

    // Server port input
    const portGroup = this.createElement("div", "control-group");
    portGroup.style.width = "80px";
    portGroup.style.flexShrink = "0";
    const portLabel = this.createElement("label", "label");
    portLabel.textContent = "Port";
    this.serverPortInput = document.createElement("input");
    this.serverPortInput.type = "number";
    this.serverPortInput.className = "text-input";
    this.serverPortInput.placeholder = "4464";
    this.serverPortInput.value = "4464";
    this.serverPortInput.style.width = "100%";
    portGroup.appendChild(portLabel);
    portGroup.appendChild(this.serverPortInput);
    hostPortRow.appendChild(portGroup);

    card.appendChild(hostPortRow);

    // Client name input
    const clientNameGroup = this.createElement("div", "control-group");
    const clientNameLabel = this.createElement("label", "label");
    clientNameLabel.textContent = "Client Name (Optional)";
    this.clientNameInput = document.createElement("input");
    this.clientNameInput.type = "text";
    this.clientNameInput.className = "text-input";
    this.clientNameInput.placeholder = "Leave empty for anonymous";
    this.clientNameInput.value = "";
    clientNameGroup.appendChild(clientNameLabel);
    clientNameGroup.appendChild(this.clientNameInput);
    card.appendChild(clientNameGroup);

    // Transport selector
    const transportGroup = this.createElement("div", "control-group");
    const transportLabel = this.createElement("label", "label");
    transportLabel.textContent = "Transport";
    this.transportSelect = document.createElement("select");
    this.transportSelect.className = "select";
    
    // Add transport options with feature detection
    const transports = this.detectAvailableTransports();
    for (const transport of transports) {
      const option = document.createElement("option");
      option.value = transport.id;
      option.textContent = transport.name + (transport.available ? "" : " (Not Available)");
      option.disabled = !transport.available;
      if (transport.id === "auto") {
        option.selected = true; // Default to Auto
      }
      this.transportSelect.appendChild(option);
    }

    // Handle transport selection changes
    this.transportSelect.addEventListener("change", () => {
      this.handleTransportChange();
    });

    transportGroup.appendChild(transportLabel);
    transportGroup.appendChild(this.transportSelect);
    card.appendChild(transportGroup);
    
    // Initialize transport with the default selection
    this.handleTransportChange();

    // Connection buttons
    const buttons = this.createElement("div", "connection-buttons");

    this.connectButton = this.createButton(
      "Connect to Studio",
      "action-btn primary",
      () => this.handleConnect()
    );
    this.disconnectButton = this.createButton(
      "Disconnect",
      "action-btn secondary",
      () => this.handleDisconnect()
    );
    this.disconnectButton.disabled = true;

    buttons.appendChild(this.connectButton);
    buttons.appendChild(this.disconnectButton);
    card.appendChild(buttons);
  }


  private detectAvailableTransports(): Array<{id: string, name: string, available: boolean}> {
    const transports = [
      {
        id: "auto",
        name: "Auto",
        available: true,
      },
      {
        id: "webrtc",
        name: "WebRTC",
        available: true,
      },
      {
        id: "webtransport",
        name: "WebTransport",
        available: typeof (window as any).WebTransport !== "undefined",
      },
      {
        id: "mock",
        name: "Mock",
        available: true,
      },
    ];
    return transports;
  }

  private handleTransportChange(): void {
    if (!this.session) return;

    const transportId = this.transportSelect.value;
    
    // Map string ID to TransportType enum
    let transportType: TransportType;
    let actualTransport: string = transportId;
    
    switch (transportId) {
      case "auto":
        // Auto-detect: use WebTransport if available, otherwise WebRTC
        const isWebTransportAvailable = typeof (window as any).WebTransport !== "undefined";
        if (isWebTransportAvailable) {
          transportType = TransportType.WebTransport;
          actualTransport = "webtransport";
        } else {
          transportType = TransportType.WebRTC;
          actualTransport = "webrtc";
        }
        console.debug(`🚀 Auto transport selected: using ${actualTransport}`);
        break;
      case "webrtc":
        transportType = TransportType.WebRTC;
        break;
      case "webtransport":
        transportType = TransportType.WebTransport;
        break;
      case "mock":
        transportType = TransportType.Mock;
        break;
      default:
        console.error("Unknown transport type:", transportId);
        return;
    }

    this.session.setTransportType(transportType);
    if (actualTransport === "webrtc" || actualTransport === "webtransport" || actualTransport === "mock") {
      this.activeTransportId = actualTransport;
    }
    if (transportId !== "auto") {
      console.debug(`🚀 Transport changed to: ${transportId}`);
    }
  }

  private createDeviceSection(
    card: HTMLElement,
    inputDevices: DeviceInfo[],
    outputDevices: DeviceInfo[]
  ): void {
    const header = this.createElement("div", "section-header");
    header.textContent = "Audio Devices";
    card.appendChild(header);

    // Input device
    const inputGroup = this.createElement("div", "control-group");
    const inputLabel = this.createElement("label", "label");
    inputLabel.textContent = "Input Device";
    this.inputSelect = document.createElement("select");
    this.inputSelect.className = "select";
    this.populateDeviceOptions(this.inputSelect, inputDevices);
    inputGroup.appendChild(inputLabel);
    inputGroup.appendChild(this.inputSelect);
    card.appendChild(inputGroup);

    // Output device
    const outputGroup = this.createElement("div", "control-group");
    const outputLabel = this.createElement("label", "label");
    outputLabel.textContent = "Output Device";
    this.outputSelect = document.createElement("select");
    this.outputSelect.className = "select";

    if (outputDevices.length > 0) {
      this.populateDeviceOptions(this.outputSelect, outputDevices);
      this.outputSelect.addEventListener("change", () => {
        this.handleOutputDeviceChange();
      });
    } else {
      // iOS Safari (and some other mobile browsers) do not enumerate audiooutput devices.
      // Show a disabled placeholder so the UI isn't confusing.
      const defaultOption = document.createElement("option");
      defaultOption.value = "";
      defaultOption.textContent = "System Default";
      this.outputSelect.appendChild(defaultOption);
      this.outputSelect.disabled = true;
      this.outputSelect.title = "Output device selection is not supported on this browser";

      const note = this.createElement("p", "device-note");
      note.textContent = "Output routing is not available on this browser.";
      outputGroup.appendChild(note);
    }

    outputGroup.appendChild(outputLabel);
    outputGroup.appendChild(this.outputSelect);
    card.appendChild(outputGroup);
  }

  private createProcessingSection(card: HTMLElement): void {
    const header = this.createElement("div", "section-header");
    header.textContent = "Audio Processing";
    card.appendChild(header);

    const container = this.createElement("div", "toggles-grid");
    const toggles = [
      { id: "agc", line1: "AGC", line2: "Auto Gain" },
      { id: "echo", line1: "Echo", line2: "Cancellation" },
      { id: "noise", line1: "Noise", line2: "Suppression" },
      { id: "stereo", line1: "Stereo", line2: "2 Channels" },
    ];

    for (const config of toggles) {
      const button = this.createToggleButton(config.line1, config.line2);
      
      if (config.id === "stereo") {
        // Stereo is active by default (session defaults to 2 channels)
        button.classList.add("active");
        button.addEventListener("click", () => {
          button.classList.toggle("active");
          const isStereo = button.classList.contains("active");
          this.handleChannelToggle(button, isStereo);
        });
      } else {
        button.addEventListener("click", () => button.classList.toggle("active"));
      }
      
      this.toggleButtons.set(config.id, button);
      container.appendChild(button);
    }

    card.appendChild(container);
  }

  private createGainSection(card: HTMLElement): void {
    const header = this.createElement("div", "section-header");
    header.textContent = "Gain Controls";
    card.appendChild(header);

    const container = this.createElement("div", "sliders-container");

    // Input Gain
    const inputGain = this.createSlider(
      "Input Gain",
      "-20",
      "20",
      "0",
      "dB",
      (value) => {
        setInputGainFromPtr(this.paramsPtr, value);
        const sign = value >= 0 ? "+" : "";
        this.inputGainValue.textContent = `${sign}${value.toFixed(1)} dB`;
      }
    );
    this.inputGainSlider = inputGain.slider;
    this.inputGainValue = inputGain.valueDisplay;
    this.inputGainValue.textContent = "0 dB";
    container.appendChild(inputGain.group);

    // Output Volume
    const outputVol = this.createSlider(
      "Output Volume",
      "0",
      "100",
      "100",
      "%",
      (value) => {
        setOutputVolumeFromPtr(this.paramsPtr, value / 100);
        this.outputVolumeValue.textContent = `${Math.round(value)}%`;
      }
    );
    this.outputVolumeSlider = outputVol.slider;
    this.outputVolumeValue = outputVol.valueDisplay;
    this.outputVolumeValue.textContent = "100%";
    container.appendChild(outputVol.group);

    // Monitor Volume
    const monitorVol = this.createSlider(
      "Monitor",
      "0",
      "100",
      "0",
      "%",
      (value) => {
        setMonitorVolumeFromPtr(this.paramsPtr, value / 100);
        this.monitorVolumeValue.textContent =
          value === 0 ? "Off" : `${Math.round(value)}%`;
      }
    );
    this.monitorVolumeSlider = monitorVol.slider;
    this.monitorVolumeValue = monitorVol.valueDisplay;
    this.monitorVolumeValue.textContent = "Off";
    container.appendChild(monitorVol.group);

    card.appendChild(container);
  }

  private createMeterSection(card: HTMLElement): void {
    const group = this.createElement("div", "control-group");

    const header = this.createElement("div", "meter-header");
    const label = this.createElement("label", "label");
    label.textContent = "Level";
    header.appendChild(label);

    this.peakDbDisplay = this.createElement(
      "div",
      "peak-db-display"
    ) as HTMLDivElement;
    this.peakDbDisplay.innerHTML =
      '<span class="peak-label">PEAK</span><span class="peak-value">-∞</span>';
    header.appendChild(this.peakDbDisplay);
    group.appendChild(header);

    const wrapper = this.createElement("div", "meter-wrapper");

    // Scale markers
    const markers = this.createElement("div", "scale-markers");
    for (const db of [-60, -48, -36, -24, -12, -6, -3, 0]) {
      const marker = this.createElement("div", "scale-marker");
      marker.style.left = `${((db + 60) / 60) * 100}%`;
      const markerLabel = this.createElement("span", "marker-label");
      markerLabel.textContent = db === 0 ? "0" : String(db);
      marker.appendChild(markerLabel);
      markers.appendChild(marker);
    }
    wrapper.appendChild(markers);

    // Meter container
    const container = this.createElement("div", "meter-container");

    const segments = this.createElement("div", "meter-segments");
    for (let i = 0; i < 60; i++) {
      segments.appendChild(this.createElement("div", "meter-segment"));
    }
    container.appendChild(segments);

    this.meterFill = this.createElement("div", "meter-fill") as HTMLDivElement;
    container.appendChild(this.meterFill);

    this.peakIndicator = this.createElement(
      "div",
      "peak-indicator"
    ) as HTMLDivElement;
    container.appendChild(this.peakIndicator);

    container.appendChild(this.createElement("div", "clip-indicator"));

    wrapper.appendChild(container);
    group.appendChild(wrapper);
    card.appendChild(group);
  }

  private createButtonSection(card: HTMLElement): void {
    // Button section removed - audio capture is now integrated with connection
  }

  // ==================== UI Helpers ====================

  private createElement(tag: string, className: string): HTMLElement {
    const el = document.createElement(tag);
    el.className = className;
    return el;
  }

  private createButton(
    text: string,
    className: string,
    onClick: () => void
  ): HTMLButtonElement {
    const btn = document.createElement("button");
    btn.className = className;
    btn.textContent = text;
    btn.addEventListener("click", onClick);
    return btn;
  }

  private createToggleButton(line1: string, line2: string): HTMLButtonElement {
    const button = document.createElement("button");
    button.className = "toggle-btn-compact";

    const span1 = document.createElement("span");
    span1.className = "toggle-line1";
    span1.textContent = line1;

    const span2 = document.createElement("span");
    span2.className = "toggle-line2";
    span2.textContent = line2;

    button.appendChild(span1);
    button.appendChild(span2);
    return button;
  }

  private createSlider(
    label: string,
    min: string,
    max: string,
    defaultVal: string,
    _unit: string,
    onChange: (value: number) => void
  ): {
    group: HTMLElement;
    slider: HTMLInputElement;
    valueDisplay: HTMLSpanElement;
  } {
    const group = this.createElement("div", "slider-group");

    const header = this.createElement("div", "slider-header");
    const labelEl = this.createElement("label", "slider-label");
    labelEl.textContent = label;
    const valueDisplay = this.createElement(
      "span",
      "slider-value"
    ) as HTMLSpanElement;
    header.appendChild(labelEl);
    header.appendChild(valueDisplay);
    group.appendChild(header);

    const wrapper = this.createElement("div", "slider-wrapper");
    const minLabel = this.createElement("span", "slider-bound");
    minLabel.textContent = min;
    const maxLabel = this.createElement("span", "slider-bound");
    maxLabel.textContent = max;

    const slider = document.createElement("input");
    slider.type = "range";
    slider.className = "gain-slider";
    slider.min = min;
    slider.max = max;
    slider.step = "0.5";
    slider.value = defaultVal;
    
    // Initialize the slider fill based on default value
    const range = parseFloat(max) - parseFloat(min);
    const initialPercent = ((parseFloat(defaultVal) - parseFloat(min)) / range) * 100;
    slider.style.setProperty("--slider-fill", `${initialPercent}%`);
    
    slider.addEventListener("input", () => {
      const value = parseFloat(slider.value);
      onChange(value);
      const percent = ((value - parseFloat(min)) / range) * 100;
      slider.style.setProperty("--slider-fill", `${percent}%`);
    });

    wrapper.appendChild(minLabel);
    wrapper.appendChild(slider);
    wrapper.appendChild(maxLabel);
    group.appendChild(wrapper);

    return { group, slider, valueDisplay };
  }

  private populateDeviceOptions(
    select: HTMLSelectElement,
    devices: DeviceInfo[]
  ): void {
    for (const device of devices) {
      const option = document.createElement("option");
      option.value = device.deviceId;
      option.textContent =
        device.label || `Device ${device.deviceId.substring(0, 8)}`;
      select.appendChild(option);
    }
  }

  private isToggleActive(id: string): boolean {
    return this.toggleButtons.get(id)?.classList.contains("active") ?? false;
  }

  // ==================== Event Handlers ====================

  private handleChannelToggle(button: HTMLButtonElement, isStereo: boolean): void {
    if (!this.session) return;

    const channels = isStereo ? 2 : 1;
    this.session.setChannels(channels);

    // Update button text
    const line1 = button.querySelector(".toggle-line1") as HTMLElement;
    const line2 = button.querySelector(".toggle-line2") as HTMLElement;
    
    if (isStereo) {
      line1.textContent = "Stereo";
      line2.textContent = "2 Channels";
    } else {
      line1.textContent = "Mono";
      line2.textContent = "1 Channel";
    }

    // Log the change
    console.debug(`Audio channels set to: ${channels} (${isStereo ? "Stereo" : "Mono"})`);
  }

  private async handleOutputDeviceChange(): Promise<void> {
    if (!this.session) return;

    const deviceId = this.outputSelect.value || undefined;
    
    try {
      await this.session.setOutputDevice(deviceId);
    } catch (error) {
      console.error("Failed to set output device:", error);
      alert(`Failed to change output device: ${error}`);
    }
  }

  /** Detect iOS / iPadOS (including iPad on iOS 13+ which reports as "Macintosh"). */
  private isIOS(): boolean {
    return (
      /iPad|iPhone|iPod/.test(navigator.userAgent) ||
      (navigator.platform === "MacIntel" && navigator.maxTouchPoints > 1)
    );
  }

  private async handleConnect(): Promise<void> {
    if (!this.session) return;

    const serverHost = this.serverHostInput.value.trim();
    if (!serverHost) {
      alert("Please enter a server host.");
      return;
    }

    const port = parseInt(this.serverPortInput.value, 10) || 4464;
    const clientName = this.clientNameInput.value.trim() || undefined;

    try {
      this.connectButton.disabled = true;
      this.connectButton.textContent = "Connecting...";

      // Wrap connectToStudio in a 45-second timeout so the UI never hangs forever.
      // The most common cause on iOS is AudioContext.resume() waiting for a user gesture
      // that never comes — the fire-and-forget fix in engine.rs prevents the hang, but
      // the timeout is kept as a safety net for any other async path that might stall.
      const CONNECT_TIMEOUT_MS = 45_000;
      const connectPromise = this.session.connectToStudio(
        serverHost,
        port,
        this.inputSelect.value || undefined,
        this.isToggleActive("agc"),
        this.isToggleActive("echo"),
        this.isToggleActive("noise"),
        clientName
      );
      const timeoutPromise = new Promise<never>((_, reject) =>
        setTimeout(
          () => reject(new Error("Connection timed out — check server address and network.")),
          CONNECT_TIMEOUT_MS
        )
      );
      await Promise.race([connectPromise, timeoutPromise]);

      // Set the output device to the selected device
      const outputDeviceId = this.outputSelect.value || undefined;
      try {
        await this.session.setOutputDevice(outputDeviceId);
      } catch (error) {
        console.warn("Failed to set output device:", error);
        // Don't fail the connection if output device setting fails
      }

      this.disconnectButton.disabled = false;

      // On iOS Safari, AudioContext.resume() requires a user gesture and may not have
      // resolved yet.  If the context is still suspended, show a one-time tap banner so
      // the user can unlock audio output without having to disconnect and reconnect.
      if (this.isIOS() && this.session.isAudioSuspended()) {
        this.showAudioResumePrompt();
      }
    } catch (error) {
      console.error("Failed to connect:", error);
      this.connectButton.disabled = false;
      this.connectButton.textContent = "Connect to Studio";
      // Reset session state — connect_to_studio failed before storing the transport,
      // so the session is still in "Connecting" state and needs to be reset to Idle.
      if (this.session) {
        this.session.disconnect();
      }
      alert(`Connection failed: ${error}`);
    }
  }

  /** Show a dismissible banner asking the user to tap to activate audio output on iOS. */
  private showAudioResumePrompt(): void {
    const existing = document.getElementById("ios-audio-prompt");
    if (existing) return; // Already shown

    const banner = document.createElement("div");
    banner.id = "ios-audio-prompt";
    banner.className = "ios-audio-prompt";
    banner.setAttribute("role", "button");
    banner.setAttribute("tabindex", "0");
    banner.textContent = "Tap here to enable audio output";

    const activate = async () => {
      try {
        if (this.session) {
          await this.session.resumeAudio();
        }
      } catch {
        // Best-effort; audio may still work via implicit unlock
      } finally {
        banner.remove();
      }
    };

    banner.addEventListener("click", activate);
    banner.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") activate();
    });

    // Insert after the status bar so it's immediately visible
    const card = document.querySelector(".card");
    if (card) {
      card.insertBefore(banner, card.firstChild);
    } else {
      document.body.insertBefore(banner, document.body.firstChild);
    }
  }

  private handleDisconnect(): void {
    if (!this.session) return;

    // Disconnect (Rust handles cleanup including event loop)
    this.session.disconnect();
    this.connectButton.disabled = false;
    this.connectButton.textContent = "Connect to Studio";
    this.disconnectButton.disabled = true;
  }

  // ==================== Status Updates ====================

  private getTransportDisplayName(): string {
    switch (this.activeTransportId) {
      case "webrtc":
        return "WebRTC";
      case "webtransport":
        return "WebTransport";
      case "mock":
        return "Mock";
      default:
        return "";
    }
  }

  private updateConnectionStatus(state: SessionState): void {
    // Ignore stale regressions that can arrive from late transport callbacks
    // after we've already reached connected.
    if (
      this.sessionState === "connected" &&
      (state === "connecting" || state === "negotiating")
    ) {
      return;
    }
    this.sessionState = state;
    const statusText = this.connectionStatus.querySelector(
      ".status-text"
    ) as HTMLElement;

    this.connectionStatus.classList.remove(
      "idle",
      "connecting",
      "negotiating",
      "connected",
      "error"
    );
    this.connectionStatus.classList.add(state);

    const labels: Record<SessionState, string> = {
      idle: "Not Connected",
      connecting: "Connecting to Server...",
      negotiating: "Negotiating WebRTC...",
      connected: "Connected",
      error: "Connection Error",
    };

    let label = labels[state] || state;
    if (state === "connected") {
      const transportName = this.getTransportDisplayName();
      if (transportName) {
        label = `Connected (${transportName})`;
      }
    }

    statusText.textContent = label;

    // Update button states
    if (state === "connected") {
      this.connectButton.disabled = true;
      this.connectButton.textContent = "Connected";
      this.disconnectButton.disabled = false;
    } else if (state === "connecting" || state === "negotiating") {
      this.connectButton.disabled = true;
      this.connectButton.textContent = "Connecting...";
      this.disconnectButton.disabled = false;
    } else {
      this.connectButton.disabled = false;
      this.connectButton.textContent = "Connect to Studio";
      this.disconnectButton.disabled = true;
      
      // If we received an error state, disconnect and clean up
      if (state === "error" && this.session) {
        this.session.disconnect();
        // Stop stats updates
        if (this.statsIntervalId !== null) {
          clearInterval(this.statsIntervalId);
          this.statsIntervalId = null;
        }
      }
    }

    // Show/hide stats
    if (state === "connected") {
      this.statsDisplay.style.display = "block";
      this.startStatsUpdate();
    } else {
      this.statsDisplay.style.display = "none";
      this.stopStatsUpdate();
    }
  }

  private startStatsUpdate(): void {
    if (this.statsIntervalId !== null) return;
    
    // Initialize callback tracking
    this.lastCallbackCount = getCallbackCountFromPtr(this.paramsPtr);
    this.lastCallbackTime = performance.now();

    this.statsIntervalId = window.setInterval(() => {
      if (!this.session) return;

      const stats = this.session.get_stats();
      
      // Calculate rates
      const currentCount = getCallbackCountFromPtr(this.paramsPtr);
      const currentTime = performance.now();
      const deltaTime = (currentTime - this.lastCallbackTime) / 1000; // seconds
      const deltaCallbacks = Number(currentCount - this.lastCallbackCount);
      
      // Ring buffer write rate
      const currentRingWrites = stats.ring_buffer_writes;
      const currentRingSamples = stats.ring_buffer_samples_written;
      const deltaRingWrites = Number(currentRingWrites - this.lastRingWrites);
      const deltaRingSamples = Number(currentRingSamples - this.lastRingSamples);
      
      if (deltaTime > 0) {
        this.callbackRate = deltaCallbacks / deltaTime;
        this.ringWriteRate = deltaRingWrites / deltaTime;
        if (deltaRingWrites > 0) {
          this.avgSamplesPerWrite = deltaRingSamples / deltaRingWrites;
        }
      }
      
      this.lastCallbackCount = currentCount;
      this.lastCallbackTime = currentTime;
      this.lastRingWrites = currentRingWrites;
      this.lastRingSamples = currentRingSamples;

      // Calculate PLC rate and packet loss percentage
      const plcRate = stats.regulator_packets_played > 0 
        ? (Number(stats.regulator_plc_count) / Number(stats.regulator_packets_played) * 100) 
        : 0;
      const lossRate = stats.packets_received > 0
        ? (Number(stats.regulator_skipped) / Number(stats.packets_received) * 100)
        : 0;
      
      this.statsDisplay.innerHTML = `
        <div class="stat-section">
          <div class="stat-section-title">Regulator (Burg PLC)</div>
          <div class="stat-row">
            <span class="stat-label">Tolerance:</span>
            <span class="stat-value">${stats.regulator_tolerance_ms.toFixed(1)} ms ${stats.regulator_initialized ? '' : '(init)'}</span>
          </div>
          <div class="stat-row">
            <span class="stat-label">Headroom:</span>
            <span class="stat-value">${stats.regulator_headroom_ms.toFixed(1)} ms</span>
          </div>
          <div class="stat-row">
            <span class="stat-label">Latency:</span>
            <span class="stat-value">${stats.regulator_max_latency_ms.toFixed(1)} ms</span>
          </div>
          <div class="stat-row">
            <span class="stat-label">Queue Depth:</span>
            <span class="stat-value">${stats.regulator_depth} pkts</span>
          </div>
          <div class="stat-row">
            <span class="stat-label">Last seq #:</span>
            <span class="stat-value">${stats.regulator_last_seq} ${stats.regulator_last_seq > 65000 ? '(wrap soon)' : ''}</span>
          </div>
        </div>
        
        <div class="stat-section">
          <div class="stat-section-title">Quality</div>
          <div class="stat-row">
            <span class="stat-label">Packets played:</span>
            <span class="stat-value">${stats.regulator_packets_played}</span>
          </div>
          <div class="stat-row">
            <span class="stat-label">PLC activations:</span>
            <span class="stat-value">${stats.regulator_plc_count} (${plcRate.toFixed(2)}%)</span>
          </div>
          <div class="stat-row">
            <span class="stat-label">Packets skipped:</span>
            <span class="stat-value">${stats.regulator_skipped} (${lossRate.toFixed(2)}%)</span>
          </div>
        </div>
      `;
    }, 500);
  }

  private stopStatsUpdate(): void {
    if (this.statsIntervalId !== null) {
      clearInterval(this.statsIntervalId);
      this.statsIntervalId = null;
    }
  }

  // ==================== Volume Meter Animation ====================

  private startVolumeAnimation(): void {
    const animate = () => {
      if (this.sessionState === "connected") {
        const volume = getVolumeLevelFromPtr(this.paramsPtr);
        const peakVolume = getPeakLevelFromPtr(this.paramsPtr);
        const db = getDbLevelFromPtr(this.paramsPtr);
        const peakDb = getPeakDbLevelFromPtr(this.paramsPtr);

        this.meterFill.style.width = `${Math.min(volume, 100)}%`;
        this.peakIndicator.style.left = `${Math.min(peakVolume, 100)}%`;
        this.peakIndicator.classList.add("active");

        const clipIndicator = document.querySelector(
          ".clip-indicator"
        ) as HTMLElement;
        if (clipIndicator) {
          if (db >= -0.5) {
            clipIndicator.classList.add("clipping");
          } else if (db < -3) {
            clipIndicator.classList.remove("clipping");
          }
        }

        const peakValue = this.peakDbDisplay.querySelector(
          ".peak-value"
        ) as HTMLElement;
        if (peakValue) {
          peakValue.textContent = peakDb <= -59 ? "-∞" : peakDb.toFixed(1);
          if (peakDb >= -3) {
            this.peakDbDisplay.classList.add("hot");
            this.peakDbDisplay.classList.remove("warm");
          } else if (peakDb >= -12) {
            this.peakDbDisplay.classList.add("warm");
            this.peakDbDisplay.classList.remove("hot");
          } else {
            this.peakDbDisplay.classList.remove("hot", "warm");
          }
        }
      } else {
        this.meterFill.style.width = "0%";
        this.peakIndicator.classList.remove("active");
        this.peakIndicator.style.left = "0%";

        const clipIndicator = document.querySelector(
          ".clip-indicator"
        ) as HTMLElement;
        if (clipIndicator) clipIndicator.classList.remove("clipping");

        const peakValue = this.peakDbDisplay.querySelector(
          ".peak-value"
        ) as HTMLElement;
        if (peakValue) peakValue.textContent = "-∞";
        this.peakDbDisplay.classList.remove("hot", "warm");
      }

      this.animationFrameId = requestAnimationFrame(animate);
    };

    animate();
  }

  private showError(title: string, message: string): void {
    const app = document.getElementById("app")!;
    app.innerHTML = `
      <div class="card error">
        <h2>${title}</h2>
        <p>${message}</p>
      </div>
    `;
  }
}

// Initialize the app
const app = new WebTripApp();
app.init().catch(console.error);
