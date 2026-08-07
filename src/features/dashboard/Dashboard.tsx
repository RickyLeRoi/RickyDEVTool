import { useTranslation } from "react-i18next";
import { useStatsStore } from "../../stores/statsStore";
import { Sparkline } from "../../components/Sparkline";
import { post } from "../../lib/api";
import { HeavyProcesses } from "./HeavyProcesses";
import { Disks } from "./Disks";
import { MetricsHistory } from "./MetricsHistory";
import { SensorsPanel } from "./SensorsPanel";
import { STATS_INTERVAL_OPTIONS_MS } from "../../lib/constants";
import { DEFAULT_STATS_INTERVAL_MS } from "../../lib/defaults";

function fmtGb(bytes: number) {
  return (bytes / 1024 ** 3).toFixed(1);
}

export function Dashboard() {
  const { t } = useTranslation();
  const { latest, cpuHistory, memHistory, error } = useStatsStore();
  const activeInterval = latest?.intervalMs ?? DEFAULT_STATS_INTERVAL_MS;

  const setInterval = (ms: number) =>
    post("/api/pollers/stats/interval", { intervalMs: ms });

  return (
    <div className="dashboard">
      <div className="dashboard-header">
        <h2>{t("nav.dashboard")}</h2>
        <div className="segmented">
          {STATS_INTERVAL_OPTIONS_MS.map((ms) => (
            <button
              key={ms}
              className={activeInterval === ms ? "active" : ""}
              onClick={() => setInterval(ms)}
            >
              {ms < 1000 ? `${ms}ms` : `${ms / 1000}s`}
            </button>
          ))}
        </div>
      </div>

      {error && (
        <div className="banner banner-error">{t("dashboard.statsError", { message: error })}</div>
      )}

      {!latest && !error && <div className="empty">{t("dashboard.waitingFirstSample")}</div>}

      {
}
      <div className="gauges">
        {latest && (
          <div className="gauge-card">
            <div className="gauge-title">CPU</div>
            <div className="gauge-value">{latest.cpuTotalPct.toFixed(0)}%</div>
            <Sparkline values={cpuHistory} />
            <div className="cores">
              {latest.cores.map((c) => (
                <div
                  key={c.core}
                  className="core-bar"
                  title={t("dashboard.coreTitle", { core: c.core, pct: c.pct.toFixed(0) })}
                >
                  <div className="core-fill" style={{ height: `${Math.min(c.pct, 100)}%` }} />
                </div>
              ))}
            </div>
          </div>
        )}

        {latest && (
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
                {t("dashboard.swap", {
                  used: fmtGb(latest.swap.usedBytes),
                  total: fmtGb(latest.swap.totalBytes),
                })}
              </div>
            )}
          </div>
        )}

        <div className="gauge-card metrics-card">
          <MetricsHistory />
        </div>
      </div>

      <SensorsPanel />
      <Disks />
      <HeavyProcesses />
    </div>
  );
}
