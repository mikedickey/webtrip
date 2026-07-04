import type { CSSProperties } from "react";

interface SliderProps {
  label: string;
  min: number;
  max: number;
  value: number;
  /** Formatted value shown next to the label (e.g. "+3.0 dB", "Off"). */
  display: string;
  onChange: (value: number) => void;
}

export default function Slider({ label, min, max, value, display, onChange }: SliderProps) {
  const fill = ((value - min) / (max - min)) * 100;
  return (
    <div className="slider-group">
      <div className="slider-header">
        <label className="slider-label">{label}</label>
        <span className="slider-value">{display}</span>
      </div>
      <div className="slider-wrapper">
        <span className="slider-bound">{min}</span>
        <input
          type="range"
          className="gain-slider"
          min={min}
          max={max}
          step={0.5}
          value={value}
          style={{ "--slider-fill": `${fill}%` } as CSSProperties}
          onChange={(e) => onChange(parseFloat(e.target.value))}
        />
        <span className="slider-bound">{max}</span>
      </div>
    </div>
  );
}
