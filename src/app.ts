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
  DeviceInfo,
  getAudioDevices,
  JackTripSession,
} from "../pkg/jacktrip_web.js";

interface AudioDevices {
  inputDevices: DeviceInfo[];
  outputDevices: DeviceInfo[];
}

type SessionState = "idle" | "local" | "connecting" | "buffering" | "streaming" | "error";

class JackTripApp {
  private paramsPtr: number = 0;
  private session: JackTripSession | null = null;
  private isCapturing = false;
  private animationFrameId: number | null = null;
  private statsIntervalId: number | null = null;
  private sessionState: SessionState = "idle";

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
  private startButton!: HTMLButtonElement;
  private connectionStatus!: HTMLDivElement;
  private sdpTextarea!: HTMLTextAreaElement;
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
      this.showError("Microphone Access Required", "Please allow microphone access and refresh the page.");
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

    // Signaling callback (SDP/ICE)
    this.session.set_on_signaling((type: string, payload: string) => {
      console.log(`Signaling [${type}]:`, payload.substring(0, 100) + "...");
      if (type === "offer" || type === "answer") {
        this.sdpTextarea.value = payload;
        this.sdpTextarea.select();
      }
    });
  }

  // ==================== UI Creation ====================

  private createUI(inputDevices: DeviceInfo[], outputDevices: DeviceInfo[]): void {
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
    header.textContent = "Peer Connection";
    card.appendChild(header);

    // Connection status
    this.connectionStatus = this.createElement("div", "connection-status") as HTMLDivElement;
    this.connectionStatus.innerHTML = '<span class="status-dot"></span><span class="status-text">Not Connected</span>';
    card.appendChild(this.connectionStatus);

    // Stats display
    this.statsDisplay = this.createElement("div", "stats-display") as HTMLDivElement;
    this.statsDisplay.style.display = "none";
    card.appendChild(this.statsDisplay);

    // SDP Exchange area
    const sdpGroup = this.createElement("div", "control-group");
    const sdpLabel = this.createElement("label", "label");
    sdpLabel.textContent = "Session Description (SDP)";
    this.sdpTextarea = document.createElement("textarea");
    this.sdpTextarea.className = "sdp-textarea";
    this.sdpTextarea.placeholder = "Paste remote SDP here, or click 'Create Offer' to generate one...";
    this.sdpTextarea.rows = 3;
    sdpGroup.appendChild(sdpLabel);
    sdpGroup.appendChild(this.sdpTextarea);
    card.appendChild(sdpGroup);

    // Connection buttons
    const buttons = this.createElement("div", "connection-buttons");

    const createOfferBtn = this.createButton("Create Offer", "action-btn", () => this.handleCreateOffer());
    const acceptOfferBtn = this.createButton("Accept Offer", "action-btn secondary", () => this.handleAcceptOffer());
    const acceptAnswerBtn = this.createButton("Accept Answer", "action-btn secondary", () => this.handleAcceptAnswer());

    buttons.appendChild(createOfferBtn);
    buttons.appendChild(acceptOfferBtn);
    buttons.appendChild(acceptAnswerBtn);
    card.appendChild(buttons);
  }

  private createDeviceSection(card: HTMLElement, inputDevices: DeviceInfo[], outputDevices: DeviceInfo[]): void {
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
    ];

    for (const config of toggles) {
      const button = this.createToggleButton(config.line1, config.line2);
      button.addEventListener("click", () => button.classList.toggle("active"));
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
    const inputGain = this.createSlider("Input Gain", "-20", "+20", "0", "dB", (value) => {
      setInputGainFromPtr(this.paramsPtr, value);
      const sign = value >= 0 ? "+" : "";
      this.inputGainValue.textContent = `${sign}${value.toFixed(1)} dB`;
    });
    this.inputGainSlider = inputGain.slider;
    this.inputGainValue = inputGain.valueDisplay;
    this.inputGainValue.textContent = "0 dB";
    container.appendChild(inputGain.group);

    // Output Volume
    const outputVol = this.createSlider("Output Volume", "0", "100", "100", "%", (value) => {
      setOutputVolumeFromPtr(this.paramsPtr, value / 100);
      this.outputVolumeValue.textContent = `${Math.round(value)}%`;
    });
    this.outputVolumeSlider = outputVol.slider;
    this.outputVolumeValue = outputVol.valueDisplay;
    this.outputVolumeValue.textContent = "100%";
    container.appendChild(outputVol.group);

    // Monitor Volume
    const monitorVol = this.createSlider("Monitor", "0", "100", "0", "%", (value) => {
      setMonitorVolumeFromPtr(this.paramsPtr, value / 100);
      this.monitorVolumeValue.textContent = value === 0 ? "Off" : `${Math.round(value)}%`;
    });
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

    this.peakDbDisplay = this.createElement("div", "peak-db-display") as HTMLDivElement;
    this.peakDbDisplay.innerHTML = '<span class="peak-label">PEAK</span><span class="peak-value">-∞</span>';
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

    this.peakIndicator = this.createElement("div", "peak-indicator") as HTMLDivElement;
    container.appendChild(this.peakIndicator);

    container.appendChild(this.createElement("div", "clip-indicator"));

    wrapper.appendChild(container);
    group.appendChild(wrapper);
    card.appendChild(group);
  }

  private createButtonSection(card: HTMLElement): void {
    const group = this.createElement("div", "button-group");
    this.startButton = document.createElement("button");
    this.startButton.className = "start-btn";
    this.startButton.textContent = "Start Capture";
    this.startButton.addEventListener("click", () => this.handleStartStop());
    group.appendChild(this.startButton);
    card.appendChild(group);
  }

  // ==================== UI Helpers ====================

  private createElement(tag: string, className: string): HTMLElement {
    const el = document.createElement(tag);
    el.className = className;
    return el;
  }

  private createButton(text: string, className: string, onClick: () => void): HTMLButtonElement {
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
  ): { group: HTMLElement; slider: HTMLInputElement; valueDisplay: HTMLSpanElement } {
    const group = this.createElement("div", "slider-group");

    const header = this.createElement("div", "slider-header");
    const labelEl = this.createElement("label", "slider-label");
    labelEl.textContent = label;
    const valueDisplay = this.createElement("span", "slider-value") as HTMLSpanElement;
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
    slider.addEventListener("input", () => {
      const value = parseFloat(slider.value);
      onChange(value);
      const range = parseFloat(max) - parseFloat(min);
      const percent = ((value - parseFloat(min)) / range) * 100;
      slider.style.setProperty("--slider-fill", `${percent}%`);
    });

    wrapper.appendChild(minLabel);
    wrapper.appendChild(slider);
    wrapper.appendChild(maxLabel);
    group.appendChild(wrapper);

    return { group, slider, valueDisplay };
  }

  private populateDeviceOptions(select: HTMLSelectElement, devices: DeviceInfo[]): void {
    for (const device of devices) {
      const option = document.createElement("option");
      option.value = device.deviceId;
      option.textContent = device.label || `Device ${device.deviceId.substring(0, 8)}`;
      select.appendChild(option);
    }
  }

  private isToggleActive(id: string): boolean {
    return this.toggleButtons.get(id)?.classList.contains("active") ?? false;
  }

  // ==================== Event Handlers ====================

  private async handleStartStop(): Promise<void> {
    if (this.isCapturing) {
      await this.stopCapture();
    } else {
      await this.startCapture();
    }
  }

  private async startCapture(): Promise<void> {
    if (!this.session) return;

    try {
      await this.session.startCapture(
        this.inputSelect.value || undefined,
        this.isToggleActive("agc"),
        this.isToggleActive("echo"),
        this.isToggleActive("noise")
      );

      this.isCapturing = true;
      this.startButton.textContent = "Stop Capture";
      this.startButton.classList.add("active");
    } catch (error) {
      console.error("Failed to start capture:", error);
    }
  }

  private async stopCapture(): Promise<void> {
    if (!this.session) return;

    this.session.stopCapture();
    this.isCapturing = false;
    this.startButton.textContent = "Start Capture";
    this.startButton.classList.remove("active");
  }

  private async handleCreateOffer(): Promise<void> {
    if (!this.session) return;

    try {
      const offer = await this.session.createOffer();
      this.sdpTextarea.value = offer;
      this.sdpTextarea.select();
      alert("Offer created! Copy the SDP and send to your peer. Paste their answer and click 'Accept Answer'.");
    } catch (error) {
      console.error("Failed to create offer:", error);
    }
  }

  private async handleAcceptOffer(): Promise<void> {
    if (!this.session) return;

    const offerSdp = this.sdpTextarea.value.trim();
    if (!offerSdp) {
      alert("Please paste the remote offer SDP first.");
      return;
    }

    try {
      const answer = await this.session.handleOffer(offerSdp);
      this.sdpTextarea.value = answer;
      this.sdpTextarea.select();
      alert("Answer created! Copy and send back to the peer who created the offer.");
    } catch (error) {
      console.error("Failed to handle offer:", error);
    }
  }

  private async handleAcceptAnswer(): Promise<void> {
    if (!this.session) return;

    const answerSdp = this.sdpTextarea.value.trim();
    if (!answerSdp) {
      alert("Please paste the remote answer SDP first.");
      return;
    }

    try {
      await this.session.handleAnswer(answerSdp);
    } catch (error) {
      console.error("Failed to handle answer:", error);
    }
  }

  // ==================== Status Updates ====================

  private updateConnectionStatus(state: SessionState): void {
    this.sessionState = state;
    const statusText = this.connectionStatus.querySelector(".status-text") as HTMLElement;

    this.connectionStatus.classList.remove("idle", "local", "connecting", "buffering", "streaming", "error");
    this.connectionStatus.classList.add(state);

    const labels: Record<SessionState, string> = {
      idle: "Not Connected",
      local: "Local Audio Only",
      connecting: "Connecting...",
      buffering: "Buffering...",
      streaming: "Connected - Streaming",
      error: "Connection Error",
    };

    statusText.textContent = labels[state] || state;

    // Show/hide stats
    if (state === "streaming") {
      this.statsDisplay.style.display = "block";
      this.startStatsUpdate();
    } else {
      this.statsDisplay.style.display = "none";
      this.stopStatsUpdate();
    }
  }

  private startStatsUpdate(): void {
    if (this.statsIntervalId !== null) return;

    this.statsIntervalId = window.setInterval(() => {
      if (!this.session) return;

      const stats = this.session.get_stats();
      this.statsDisplay.innerHTML = `
        <div class="stat-row">
          <span class="stat-label">Sent:</span>
          <span class="stat-value">${stats.packets_sent}</span>
        </div>
        <div class="stat-row">
          <span class="stat-label">Received:</span>
          <span class="stat-value">${stats.packets_received}</span>
        </div>
        <div class="stat-row">
          <span class="stat-label">Jitter Buf:</span>
          <span class="stat-value">${stats.jitter_depth} pkts</span>
        </div>
        <div class="stat-row">
          <span class="stat-label">Latency:</span>
          <span class="stat-value">${stats.jitter_latency_ms.toFixed(1)} ms</span>
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
      if (this.isCapturing) {
        const volume = getVolumeLevelFromPtr(this.paramsPtr);
        const peakVolume = getPeakLevelFromPtr(this.paramsPtr);
        const db = getDbLevelFromPtr(this.paramsPtr);
        const peakDb = getPeakDbLevelFromPtr(this.paramsPtr);

        this.meterFill.style.width = `${Math.min(volume, 100)}%`;
        this.peakIndicator.style.left = `${Math.min(peakVolume, 100)}%`;
        this.peakIndicator.classList.add("active");

        const clipIndicator = document.querySelector(".clip-indicator") as HTMLElement;
        if (clipIndicator) {
          if (db >= -0.5) {
            clipIndicator.classList.add("clipping");
          } else if (db < -3) {
            clipIndicator.classList.remove("clipping");
          }
        }

        const peakValue = this.peakDbDisplay.querySelector(".peak-value") as HTMLElement;
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

        const clipIndicator = document.querySelector(".clip-indicator") as HTMLElement;
        if (clipIndicator) clipIndicator.classList.remove("clipping");

        const peakValue = this.peakDbDisplay.querySelector(".peak-value") as HTMLElement;
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
