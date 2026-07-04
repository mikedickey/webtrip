import { useEffect, useState } from "react";
import type { DemoEngine } from "../../lib/webtrip";

interface DemoStats {
  toleranceMs: number;
  initialized: boolean;
  headroomMs: number;
  maxLatencyMs: number;
  depth: number;
  lastSeq: number;
  played: number;
  plc: number;
  plcRate: number;
  skipped: number;
  lossRate: number;
}

/** Connection statistics, polled from the session twice a second while mounted. */
export default function StatsPanel({ engine }: { engine: DemoEngine }) {
  const [stats, setStats] = useState<DemoStats | null>(null);

  useEffect(() => {
    const id = window.setInterval(() => {
      const s = engine.session.get_stats();
      const played = Number(s.regulator_packets_played);
      const plc = Number(s.regulator_plc_count);
      const skipped = Number(s.regulator_skipped);
      const received = Number(s.packets_received);
      setStats({
        toleranceMs: s.regulator_tolerance_ms,
        initialized: s.regulator_initialized,
        headroomMs: s.regulator_headroom_ms,
        maxLatencyMs: s.regulator_max_latency_ms,
        depth: s.regulator_depth,
        lastSeq: s.regulator_last_seq,
        played,
        plc,
        plcRate: played > 0 ? (plc / played) * 100 : 0,
        skipped,
        lossRate: received > 0 ? (skipped / received) * 100 : 0,
      });
    }, 500);
    return () => clearInterval(id);
  }, [engine]);

  if (!stats) {
    return null;
  }

  const rows = (entries: [string, string][]) =>
    entries.map(([label, value]) => (
      <div className="stat-row" key={label}>
        <span className="stat-label">{label}</span>
        <span className="stat-value">{value}</span>
      </div>
    ));

  return (
    <div className="stats-display">
      <div className="stat-section">
        <div className="stat-section-title">Regulator (Burg PLC)</div>
        {rows([
          ["Tolerance:", `${stats.toleranceMs.toFixed(1)} ms${stats.initialized ? "" : " (init)"}`],
          ["Headroom:", `${stats.headroomMs.toFixed(1)} ms`],
          ["Latency:", `${stats.maxLatencyMs.toFixed(1)} ms`],
          ["Queue Depth:", `${stats.depth} pkts`],
          ["Last seq #:", `${stats.lastSeq}${stats.lastSeq > 65000 ? " (wrap soon)" : ""}`],
        ])}
      </div>
      <div className="stat-section">
        <div className="stat-section-title">Quality</div>
        {rows([
          ["Packets played:", String(stats.played)],
          ["PLC activations:", `${stats.plc} (${stats.plcRate.toFixed(2)}%)`],
          ["Packets skipped:", `${stats.skipped} (${stats.lossRate.toFixed(2)}%)`],
        ])}
      </div>
    </div>
  );
}
