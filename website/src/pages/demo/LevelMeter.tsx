import { useEffect, useRef } from "react";
import type { DemoEngine } from "../../lib/webtrip";

const SCALE_MARKS = [-60, -48, -36, -24, -12, -6, -3, 0];
const SEGMENT_COUNT = 60;

/**
 * Input level meter. Levels change every audio callback, so the meter is
 * driven imperatively at requestAnimationFrame rate through refs rather than
 * React state — re-rendering at 60fps would be wasted work.
 */
export default function LevelMeter({ engine, active }: { engine: DemoEngine; active: boolean }) {
  const fillRef = useRef<HTMLDivElement>(null);
  const peakRef = useRef<HTMLDivElement>(null);
  const clipRef = useRef<HTMLDivElement>(null);
  const peakDisplayRef = useRef<HTMLDivElement>(null);
  const peakValueRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    const fill = fillRef.current!;
    const peak = peakRef.current!;
    const clip = clipRef.current!;
    const peakDisplay = peakDisplayRef.current!;
    const peakValue = peakValueRef.current!;

    if (!active) {
      fill.style.width = "0%";
      peak.classList.remove("active");
      peak.style.left = "0%";
      clip.classList.remove("clipping");
      peakValue.textContent = "-∞";
      peakDisplay.classList.remove("hot", "warm");
      return;
    }

    const { m, paramsPtr } = engine;
    let frameId: number;
    const animate = () => {
      const volume = m.getVolumeLevelFromPtr(paramsPtr);
      const peakVolume = m.getPeakLevelFromPtr(paramsPtr);
      const db = m.getDbLevelFromPtr(paramsPtr);
      const peakDb = m.getPeakDbLevelFromPtr(paramsPtr);

      fill.style.width = `${Math.min(volume, 100)}%`;
      peak.style.left = `${Math.min(peakVolume, 100)}%`;
      peak.classList.add("active");

      // Hysteresis: latch on near clipping, release only well below it.
      if (db >= -0.5) {
        clip.classList.add("clipping");
      } else if (db < -3) {
        clip.classList.remove("clipping");
      }

      peakValue.textContent = peakDb <= -59 ? "-∞" : peakDb.toFixed(1);
      if (peakDb >= -3) {
        peakDisplay.classList.add("hot");
        peakDisplay.classList.remove("warm");
      } else if (peakDb >= -12) {
        peakDisplay.classList.add("warm");
        peakDisplay.classList.remove("hot");
      } else {
        peakDisplay.classList.remove("hot", "warm");
      }

      frameId = requestAnimationFrame(animate);
    };
    frameId = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(frameId);
  }, [engine, active]);

  return (
    <div className="control-group">
      <div className="meter-header">
        <label className="label">Level</label>
        <div className="peak-db-display" ref={peakDisplayRef}>
          <span className="peak-label">PEAK</span>
          <span className="peak-value" ref={peakValueRef}>
            -∞
          </span>
        </div>
      </div>
      <div className="meter-wrapper">
        <div className="scale-markers">
          {SCALE_MARKS.map((db) => (
            <div key={db} className="scale-marker" style={{ left: `${((db + 60) / 60) * 100}%` }}>
              <span className="marker-label">{db}</span>
            </div>
          ))}
        </div>
        <div className="meter-container">
          <div className="meter-segments">
            {Array.from({ length: SEGMENT_COUNT }, (_, i) => (
              <div key={i} className="meter-segment" />
            ))}
          </div>
          <div className="meter-fill" ref={fillRef} />
          <div className="peak-indicator" ref={peakRef} />
          <div className="clip-indicator" ref={clipRef} />
        </div>
      </div>
    </div>
  );
}
