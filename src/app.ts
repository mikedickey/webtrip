// JackTrip WebRTC Audio - UI Controller
//
// This file handles ONLY UI interactions. All audio and network
// logic is handled in Rust via JackTripSession.

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
  JackTripSession,
  hasAtomicsWaitAsync,
} from "../pkg/jacktrip_web.js";

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

class JackTripApp {
  private paramsPtr: number = 0;
  private session: JackTripSession | null = null;
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
  
  // Packet rate tracking
  private lastPacketsSent: bigint = 0n;
  private lastPacketsReceived: bigint = 0n;
  private packetSendRate: number = 0;
  private packetReceiveRate: number = 0;

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
  private connectButton!: HTMLButtonElement;
  private disconnectButton!: HTMLButtonElement;
  private statsDisplay!: HTMLDivElement;
  private toggleButtons: Map<string, HTMLButtonElement> = new Map();

  async init(): Promise<void> {
    // Initialize WASM module
    await init();
    wasmInit();

    // Create shared audio params
    this.paramsPtr = createAudioParams();

    // Create session (handles all audio and network logic)
    this.session = new JackTripSession(this.paramsPtr);
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
      console.log("Session state:", state);
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
    title.textContent = "JackTrip";
    card.appendChild(title);

    const subtitle = this.createElement("p", "subtitle");
    subtitle.textContent = "Real-time audio streaming over WebRTC";
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

    // Server host input
    const hostGroup = this.createElement("div", "control-group");
    const hostLabel = this.createElement("label", "label");
    hostLabel.textContent = "Server Host";
    this.serverHostInput = document.createElement("input");
    this.serverHostInput.type = "text";
    this.serverHostInput.className = "text-input";
    this.serverHostInput.placeholder = "studio.jacktrip.org";
    this.serverHostInput.value = "localhost";
    hostGroup.appendChild(hostLabel);
    hostGroup.appendChild(this.serverHostInput);
    card.appendChild(hostGroup);

    // Server port input
    const portGroup = this.createElement("div", "control-group inline");
    const portLabel = this.createElement("label", "label");
    portLabel.textContent = "Port";
    this.serverPortInput = document.createElement("input");
    this.serverPortInput.type = "number";
    this.serverPortInput.className = "text-input port-input";
    this.serverPortInput.placeholder = "4464";
    this.serverPortInput.value = "4464";
    portGroup.appendChild(portLabel);
    portGroup.appendChild(this.serverPortInput);
    card.appendChild(portGroup);

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
    this.populateDeviceOptions(this.outputSelect, outputDevices);
    
    // Handle output device changes
    this.outputSelect.addEventListener("change", () => {
      this.handleOutputDeviceChange();
    });
    
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
    console.log(`Audio channels set to: ${channels} (${isStereo ? "Stereo" : "Mono"})`);
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

  private async handleConnect(): Promise<void> {
    if (!this.session) return;

    const serverHost = this.serverHostInput.value.trim();
    if (!serverHost) {
      alert("Please enter a server host.");
      return;
    }

    const port = parseInt(this.serverPortInput.value, 10) || 4464;
    const useTls = serverHost.includes("jacktrip.org"); // Use TLS for production servers

    try {
      this.connectButton.disabled = true;
      this.connectButton.textContent = "Connecting...";

      console.log("Connecting to studio...");
      await this.session.connectToStudio(
        serverHost,
        port,
        useTls,
        this.inputSelect.value || undefined,
        this.isToggleActive("agc"),
        this.isToggleActive("echo"),
        this.isToggleActive("noise")
      );
      console.log("Connected to studio");

      // Set the output device to the selected device
      const outputDeviceId = this.outputSelect.value || undefined;
      try {
        await this.session.setOutputDevice(outputDeviceId);
      } catch (error) {
        console.warn("Failed to set output device:", error);
        // Don't fail the connection if output device setting fails
      }

      this.disconnectButton.disabled = false;
    } catch (error) {
      console.error("Failed to connect:", error);
      this.connectButton.disabled = false;
      this.connectButton.textContent = "Connect to Studio";
      alert(`Connection failed: ${error}`);
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

  private updateConnectionStatus(state: SessionState): void {
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

    statusText.textContent = labels[state] || state;

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
      
      // Packet send/receive rate
      const currentPacketsSent = stats.packets_sent;
      const currentPacketsReceived = stats.packets_received;
      const deltaPacketsSent = Number(currentPacketsSent - this.lastPacketsSent);
      const deltaPacketsReceived = Number(currentPacketsReceived - this.lastPacketsReceived);
      
      if (deltaTime > 0) {
        this.callbackRate = deltaCallbacks / deltaTime;
        this.ringWriteRate = deltaRingWrites / deltaTime;
        this.packetSendRate = deltaPacketsSent / deltaTime;
        this.packetReceiveRate = deltaPacketsReceived / deltaTime;
        if (deltaRingWrites > 0) {
          this.avgSamplesPerWrite = deltaRingSamples / deltaRingWrites;
        }
      }
      
      this.lastCallbackCount = currentCount;
      this.lastCallbackTime = currentTime;
      this.lastRingWrites = currentRingWrites;
      this.lastRingSamples = currentRingSamples;
      this.lastPacketsSent = currentPacketsSent;
      this.lastPacketsReceived = currentPacketsReceived;

      this.statsDisplay.innerHTML = `
        <div class="stat-row">
          <span class="stat-label">Callbacks/s:</span>
          <span class="stat-value">${this.callbackRate.toFixed(0)}</span>
        </div>
        <div class="stat-row">
          <span class="stat-label">Ring writes/s:</span>
          <span class="stat-value">${this.ringWriteRate.toFixed(0)}</span>
        </div>
        <div class="stat-row">
          <span class="stat-label">Samples/write:</span>
          <span class="stat-value">${this.avgSamplesPerWrite.toFixed(0)}</span>
        </div>
        <div class="stat-row">
          <span class="stat-label">Sent/s:</span>
          <span class="stat-value">${this.packetSendRate.toFixed(0)}</span>
        </div>
        <div class="stat-row">
          <span class="stat-label">Recv/s:</span>
          <span class="stat-value">${this.packetReceiveRate.toFixed(0)}</span>
        </div>
        <div class="stat-row">
          <span class="stat-label">Jitter depth:</span>
          <span class="stat-value">${stats.jitter_depth}/${stats.jitter_target_depth} ${stats.jitter_buffering ? '(buffering)' : ''}</span>
        </div>
        <div class="stat-row">
          <span class="stat-label">Jitter played:</span>
          <span class="stat-value">${stats.jitter_played}</span>
        </div>
        <div class="stat-row">
          <span class="stat-label">Jitter underruns:</span>
          <span class="stat-value">${stats.jitter_underruns}</span>
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
const app = new JackTripApp();
app.init().catch(console.error);
