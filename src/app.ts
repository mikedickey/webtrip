// TypeScript frontend for WASM Audio Worklet
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
  AudioEngine,
  DeviceInfo,
  getAudioDevices,
} from "../pkg/wasm_audio_worklet.js";

interface AudioDevices {
  inputDevices: DeviceInfo[];
  outputDevices: DeviceInfo[];
}

class AudioCaptureApp {
  private paramsPtr: number = 0;
  private engine: AudioEngine | null = null;
  private isCapturing = false;
  private animationFrameId: number | null = null;

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
  private toggleButtons: Map<string, HTMLButtonElement> = new Map();

  async init(): Promise<void> {
    // Initialize WASM module
    await init();
    wasmInit();

    // Create shared audio params
    this.paramsPtr = createAudioParams();

    try {
      const devices = (await getAudioDevices()) as AudioDevices;
      this.createUI(devices.inputDevices, devices.outputDevices);
      this.startVolumeAnimation();
    } catch (error) {
      this.showError("Microphone Access Required", "Please allow microphone access and refresh the page.");
      throw error;
    }
  }

  private createUI(inputDevices: DeviceInfo[], outputDevices: DeviceInfo[]): void {
    const app = document.getElementById("app")!;
    app.innerHTML = "";

    const card = this.createElement("div", "card");

    // Title
    const title = this.createElement("h1", "title");
    title.textContent = "JackTrip";
    card.appendChild(title);

    const subtitle = this.createElement("p", "subtitle");
    subtitle.textContent = "Real-time audio monitoring with WebAssembly";
    card.appendChild(subtitle);

    // Input device selector
    const inputGroup = this.createElement("div", "control-group");
    const inputLabel = this.createElement("label", "label");
    inputLabel.textContent = "Input Device";
    this.inputSelect = document.createElement("select");
    this.inputSelect.className = "select";
    this.populateDeviceOptions(this.inputSelect, inputDevices);
    inputGroup.appendChild(inputLabel);
    inputGroup.appendChild(this.inputSelect);
    card.appendChild(inputGroup);

    // Output device selector
    const outputGroup = this.createElement("div", "control-group");
    const outputLabel = this.createElement("label", "label");
    outputLabel.textContent = "Output Device";
    this.outputSelect = document.createElement("select");
    this.outputSelect.className = "select";
    this.populateDeviceOptions(this.outputSelect, outputDevices);
    outputGroup.appendChild(outputLabel);
    outputGroup.appendChild(this.outputSelect);
    card.appendChild(outputGroup);

    // Audio processing section header
    const processingHeader = this.createElement("div", "section-header");
    processingHeader.textContent = "Audio Processing";
    card.appendChild(processingHeader);

    // Toggle buttons container
    const togglesContainer = this.createElement("div", "toggles-grid");

    // Create toggle buttons
    const toggleConfigs = [
      { id: "agc", line1: "AGC", line2: "Auto Gain" },
      { id: "echo", line1: "Echo", line2: "Cancellation" },
      { id: "noise", line1: "Noise", line2: "Suppression" },
    ];

    for (const config of toggleConfigs) {
      const button = this.createToggleButton(config.line1, config.line2);
      button.addEventListener("click", () => this.handleToggle(config.id, button));
      this.toggleButtons.set(config.id, button);
      togglesContainer.appendChild(button);
    }

    card.appendChild(togglesContainer);

    // Gain Controls section header
    const gainHeader = this.createElement("div", "section-header");
    gainHeader.textContent = "Gain Controls";
    card.appendChild(gainHeader);

    // Sliders container
    const slidersContainer = this.createElement("div", "sliders-container");

    // Input Gain Slider
    const inputGainGroup = this.createElement("div", "slider-group");
    const inputGainHeader = this.createElement("div", "slider-header");
    const inputGainLabel = this.createElement("label", "slider-label");
    inputGainLabel.textContent = "Input Gain";
    this.inputGainValue = this.createElement("span", "slider-value") as HTMLSpanElement;
    this.inputGainValue.textContent = "0 dB";
    inputGainHeader.appendChild(inputGainLabel);
    inputGainHeader.appendChild(this.inputGainValue);
    inputGainGroup.appendChild(inputGainHeader);
    
    const inputGainSliderWrapper = this.createElement("div", "slider-wrapper");
    const inputGainMin = this.createElement("span", "slider-bound");
    inputGainMin.textContent = "-20";
    const inputGainMax = this.createElement("span", "slider-bound");
    inputGainMax.textContent = "+20";
    this.inputGainSlider = document.createElement("input");
    this.inputGainSlider.type = "range";
    this.inputGainSlider.className = "gain-slider";
    this.inputGainSlider.min = "-20";
    this.inputGainSlider.max = "20";
    this.inputGainSlider.step = "0.5";
    this.inputGainSlider.value = "0";
    this.inputGainSlider.addEventListener("input", () => this.handleInputGainChange());
    inputGainSliderWrapper.appendChild(inputGainMin);
    inputGainSliderWrapper.appendChild(this.inputGainSlider);
    inputGainSliderWrapper.appendChild(inputGainMax);
    inputGainGroup.appendChild(inputGainSliderWrapper);
    slidersContainer.appendChild(inputGainGroup);

    // Output Volume Slider
    const outputVolumeGroup = this.createElement("div", "slider-group");
    const outputVolumeHeader = this.createElement("div", "slider-header");
    const outputVolumeLabel = this.createElement("label", "slider-label");
    outputVolumeLabel.textContent = "Output Volume";
    this.outputVolumeValue = this.createElement("span", "slider-value") as HTMLSpanElement;
    this.outputVolumeValue.textContent = "100%";
    outputVolumeHeader.appendChild(outputVolumeLabel);
    outputVolumeHeader.appendChild(this.outputVolumeValue);
    outputVolumeGroup.appendChild(outputVolumeHeader);
    
    const outputVolumeSliderWrapper = this.createElement("div", "slider-wrapper");
    const outputVolumeMin = this.createElement("span", "slider-bound");
    outputVolumeMin.textContent = "0";
    const outputVolumeMax = this.createElement("span", "slider-bound");
    outputVolumeMax.textContent = "100";
    this.outputVolumeSlider = document.createElement("input");
    this.outputVolumeSlider.type = "range";
    this.outputVolumeSlider.className = "volume-slider";
    this.outputVolumeSlider.min = "0";
    this.outputVolumeSlider.max = "100";
    this.outputVolumeSlider.step = "1";
    this.outputVolumeSlider.value = "100";
    this.outputVolumeSlider.addEventListener("input", () => this.handleOutputVolumeChange());
    outputVolumeSliderWrapper.appendChild(outputVolumeMin);
    outputVolumeSliderWrapper.appendChild(this.outputVolumeSlider);
    outputVolumeSliderWrapper.appendChild(outputVolumeMax);
    outputVolumeGroup.appendChild(outputVolumeSliderWrapper);
    slidersContainer.appendChild(outputVolumeGroup);

    // Monitor Volume Slider
    const monitorVolumeGroup = this.createElement("div", "slider-group");
    const monitorVolumeHeader = this.createElement("div", "slider-header");
    const monitorVolumeLabel = this.createElement("label", "slider-label");
    monitorVolumeLabel.textContent = "Monitor";
    this.monitorVolumeValue = this.createElement("span", "slider-value") as HTMLSpanElement;
    this.monitorVolumeValue.textContent = "Off";
    monitorVolumeHeader.appendChild(monitorVolumeLabel);
    monitorVolumeHeader.appendChild(this.monitorVolumeValue);
    monitorVolumeGroup.appendChild(monitorVolumeHeader);
    
    const monitorVolumeSliderWrapper = this.createElement("div", "slider-wrapper");
    const monitorVolumeMin = this.createElement("span", "slider-bound");
    monitorVolumeMin.textContent = "0";
    const monitorVolumeMax = this.createElement("span", "slider-bound");
    monitorVolumeMax.textContent = "100";
    this.monitorVolumeSlider = document.createElement("input");
    this.monitorVolumeSlider.type = "range";
    this.monitorVolumeSlider.className = "monitor-slider";
    this.monitorVolumeSlider.min = "0";
    this.monitorVolumeSlider.max = "100";
    this.monitorVolumeSlider.step = "1";
    this.monitorVolumeSlider.value = "0";
    this.monitorVolumeSlider.addEventListener("input", () => this.handleMonitorVolumeChange());
    monitorVolumeSliderWrapper.appendChild(monitorVolumeMin);
    monitorVolumeSliderWrapper.appendChild(this.monitorVolumeSlider);
    monitorVolumeSliderWrapper.appendChild(monitorVolumeMax);
    monitorVolumeGroup.appendChild(monitorVolumeSliderWrapper);
    slidersContainer.appendChild(monitorVolumeGroup);

    card.appendChild(slidersContainer);

    // Volume meter
    const meterGroup = this.createElement("div", "control-group");
    const meterHeader = this.createElement("div", "meter-header");
    const meterLabel = this.createElement("label", "label");
    meterLabel.textContent = "Level";
    meterHeader.appendChild(meterLabel);
    
    // Peak dB display
    this.peakDbDisplay = this.createElement("div", "peak-db-display") as HTMLDivElement;
    this.peakDbDisplay.innerHTML = '<span class="peak-label">PEAK</span><span class="peak-value">-∞</span>';
    meterHeader.appendChild(this.peakDbDisplay);
    meterGroup.appendChild(meterHeader);
    
    // Meter visualization
    const meterWrapper = this.createElement("div", "meter-wrapper");
    
    // dB scale markers
    const scaleMarkers = this.createElement("div", "scale-markers");
    const dbMarks = [-60, -48, -36, -24, -12, -6, -3, 0];
    for (const db of dbMarks) {
      const marker = this.createElement("div", "scale-marker");
      const pos = ((db + 60) / 60) * 100;
      marker.style.left = `${pos}%`;
      const label = this.createElement("span", "marker-label");
      label.textContent = db === 0 ? "0" : String(db);
      marker.appendChild(label);
      scaleMarkers.appendChild(marker);
    }
    meterWrapper.appendChild(scaleMarkers);
    
    // Main meter container
    const meterContainer = this.createElement("div", "meter-container");
    
    // Segmented background
    const segments = this.createElement("div", "meter-segments");
    for (let i = 0; i < 60; i++) {
      const segment = this.createElement("div", "meter-segment");
      segments.appendChild(segment);
    }
    meterContainer.appendChild(segments);
    
    // Meter fill
    this.meterFill = this.createElement("div", "meter-fill") as HTMLDivElement;
    meterContainer.appendChild(this.meterFill);
    
    // Peak indicator
    this.peakIndicator = this.createElement("div", "peak-indicator") as HTMLDivElement;
    meterContainer.appendChild(this.peakIndicator);
    
    // Clip indicator
    const clipIndicator = this.createElement("div", "clip-indicator");
    meterContainer.appendChild(clipIndicator);
    
    meterWrapper.appendChild(meterContainer);
    meterGroup.appendChild(meterWrapper);
    card.appendChild(meterGroup);

    // Start/Stop button
    const buttonGroup = this.createElement("div", "button-group");
    this.startButton = document.createElement("button");
    this.startButton.className = "start-btn";
    this.startButton.textContent = "Start Capture";
    this.startButton.addEventListener("click", () => this.handleStartStop());
    buttonGroup.appendChild(this.startButton);
    card.appendChild(buttonGroup);

    app.appendChild(card);
  }

  private createElement(tag: string, className: string): HTMLElement {
    const el = document.createElement(tag);
    el.className = className;
    return el;
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

  private populateDeviceOptions(select: HTMLSelectElement, devices: DeviceInfo[]): void {
    for (const device of devices) {
      const option = document.createElement("option");
      option.value = device.deviceId;
      option.textContent = device.label || `Device ${device.deviceId.substring(0, 8)}`;
      select.appendChild(option);
    }
  }

  private handleToggle(id: string, button: HTMLButtonElement): void {
    button.classList.toggle("active");
  }

  private handleInputGainChange(): void {
    const gainDb = parseFloat(this.inputGainSlider.value);
    setInputGainFromPtr(this.paramsPtr, gainDb);
    
    // Update display
    const sign = gainDb >= 0 ? "+" : "";
    this.inputGainValue.textContent = `${sign}${gainDb.toFixed(1)} dB`;
    
    // Update slider fill
    const percent = ((gainDb + 20) / 40) * 100;
    this.inputGainSlider.style.setProperty("--slider-fill", `${percent}%`);
  }

  private handleOutputVolumeChange(): void {
    const volumePercent = parseFloat(this.outputVolumeSlider.value);
    setOutputVolumeFromPtr(this.paramsPtr, volumePercent / 100);
    
    // Update display
    this.outputVolumeValue.textContent = `${Math.round(volumePercent)}%`;
    
    // Update slider fill
    this.outputVolumeSlider.style.setProperty("--slider-fill", `${volumePercent}%`);
  }

  private handleMonitorVolumeChange(): void {
    const volumePercent = parseFloat(this.monitorVolumeSlider.value);
    setMonitorVolumeFromPtr(this.paramsPtr, volumePercent / 100);
    
    // Update display
    if (volumePercent === 0) {
      this.monitorVolumeValue.textContent = "Off";
    } else {
      this.monitorVolumeValue.textContent = `${Math.round(volumePercent)}%`;
    }
    
    // Update slider fill
    this.monitorVolumeSlider.style.setProperty("--slider-fill", `${volumePercent}%`);
  }

  private isToggleActive(id: string): boolean {
    const button = this.toggleButtons.get(id);
    return button?.classList.contains("active") ?? false;
  }

  private async handleStartStop(): Promise<void> {
    if (this.isCapturing) {
      await this.stopCapture();
    } else {
      await this.startCapture();
    }
  }

  private async startCapture(): Promise<void> {
    try {
      const deviceId = this.inputSelect.value;
      const autoGainControl = this.isToggleActive("agc");
      const echoCancellation = this.isToggleActive("echo");
      const noiseSuppression = this.isToggleActive("noise");

      this.engine = await AudioEngine.create(this.paramsPtr);
      await this.engine.startCapture(
        deviceId || undefined,
        autoGainControl,
        echoCancellation,
        noiseSuppression
      );

      this.isCapturing = true;
      this.startButton.textContent = "Stop Capture";
      this.startButton.classList.add("active");
    } catch (error) {
      console.error("Failed to start capture:", error);
    }
  }

  private async stopCapture(): Promise<void> {
    if (this.engine) {
      this.engine.stopCapture();
      this.engine = null;
    }

    this.isCapturing = false;
    this.startButton.textContent = "Start Capture";
    this.startButton.classList.remove("active");
  }

  private startVolumeAnimation(): void {
    const animate = () => {
      if (this.isCapturing) {
        // Read levels from WASM shared params
        const volume = getVolumeLevelFromPtr(this.paramsPtr);
        const peakVolume = getPeakLevelFromPtr(this.paramsPtr);
        const db = getDbLevelFromPtr(this.paramsPtr);
        const peakDb = getPeakDbLevelFromPtr(this.paramsPtr);

        // Update meter fill width
        this.meterFill.style.width = `${Math.min(volume, 100)}%`;
        
        // Update peak indicator position
        this.peakIndicator.style.left = `${Math.min(peakVolume, 100)}%`;
        this.peakIndicator.classList.add("active");
        
        // Update clip indicator if clipping
        const clipIndicator = document.querySelector(".clip-indicator") as HTMLElement;
        if (clipIndicator) {
          if (db >= -0.5) {
            clipIndicator.classList.add("clipping");
          } else if (db < -3) {
            clipIndicator.classList.remove("clipping");
          }
        }
        
        // Update peak dB display
        const peakValue = this.peakDbDisplay.querySelector(".peak-value") as HTMLElement;
        if (peakValue) {
          if (peakDb <= -59) {
            peakValue.textContent = "-∞";
          } else {
            peakValue.textContent = peakDb.toFixed(1);
          }
          // Color the peak based on level
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
        // Reset meter when not capturing
        this.meterFill.style.width = "0%";
        this.peakIndicator.classList.remove("active");
        this.peakIndicator.style.left = "0%";
        
        const clipIndicator = document.querySelector(".clip-indicator") as HTMLElement;
        if (clipIndicator) {
          clipIndicator.classList.remove("clipping");
        }
        
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
const app = new AudioCaptureApp();
app.init().catch(console.error);
