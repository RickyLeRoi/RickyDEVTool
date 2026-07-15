import { useStatsStore } from "../../stores/statsStore";
import { Sparkline } from "../../components/Sparkline";
import { post } from "../../lib/api";
import { HeavyProcesses } from "./HeavyProcesses";

const INTERVALS = [500, 1000, 2000, 5000, 10000];

function fmtGb(bytes: number) {
  return (bytes / 1024 ** 3).toFixed(1);
}

export function Dashboard() {
  const { latest, cpuHistory, memHistory, error } = useStatsStore();

  if (error) {
    return <div className="banner banner-error">Errore lettura statistiche: {error}</div>;
  }
  if (!latest) {
    return <div className="empty">In attesa del primo campione…</div>;
  }

  const setInterval = (ms: number) =>
    post("/api/pollers/stats/interval", { intervalMs: ms });

  return (
    <div className="dashboard">
      <div className="dashboard-header">
        <h2>Dashboard</h2>
        <div className="segmented">
          {INTERVALS.map((ms) => (
            <button
              key={ms}
              className={latest.intervalMs === ms ? "active" : ""}
              onClick={() => setInterval(ms)}
            >
              {ms < 1000 ? `${ms}ms` : `${ms / 1000}s`}
            </button>
          ))}
        </div>
      </div>

      <div className="gauges">
        <div className="gauge-card">
          <div className="gauge-title">CPU</div>
          <div className="gauge-value">{latest.cpuTotalPct.toFixed(0)}%</div>
          <Sparkline values={cpuHistory} />
          <div className="cores">
            {latest.cores.map((c) => (
              <div
                key={c.core}
                className="core-bar"
                title={`core ${c.core}: ${c.pct.toFixed(0)}%`}
              >
                <div
                  className="core-fill"
                  style={{ height: `${Math.min(c.pct, 100)}%` }}
                />
              </div>
            ))}
          </div>
        </div>

        <div className="gauge-card">
          <div className="gauge-title">RAM</div>
          <div className="gauge-value">
            {latest.mem.usedPct.toFixed(0)}%
            <span className="gauge-sub">
              {fmtGb(latest.mem.usedBytes)} / {fmtGb(latest.mem.totalBytes)} GB
            </span>
          </div>
          <Sparkline values={memHistory} stroke="var(--accent2)" />
          {latest.swap && (
            <div className="gauge-sub">
              swap {fmtGb(latest.swap.usedBytes)} / {fmtGb(latest.swap.totalBytes)} GB
            </div>
          )}
        </div>
      </div>

      <HeavyProcesses />
    </div>
  );
}
