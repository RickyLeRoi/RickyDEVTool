import { useEffect, useState } from "react";
import { useStatsStore } from "../stores/statsStore";
import { Sparkline } from "../components/Sparkline";
import { ws } from "../lib/ws";
import { api, post } from "../lib/api";
import type { Alert } from "../lib/types";

function fmtGb(bytes: number) {
  return (bytes / 1024 ** 3).toFixed(1);
}

/** Pannello destro sempre visibile: CPU/RAM in miniatura + alert. */
export function VitalsPanel() {
  const { latest, cpuHistory, memHistory } = useStatsStore();
  const [alerts, setAlerts] = useState<Alert[]>([]);

  useEffect(() => {
    api<{ alerts: Alert[] }>("/api/alerts").then((r) => {
      if (r.ok) setAlerts(r.data.alerts);
    });
    return ws.subscribe("alerts", (event) => {
      setAlerts((event.payload as { alerts: Alert[] }).alerts);
    });
  }, []);

  return (
    <aside className="vitals">
      <div className="vitals-block">
        <div className="vitals-row">
          <span className="vitals-label">CPU</span>
          <span className="vitals-value">
            {latest ? `${latest.cpuTotalPct.toFixed(0)}%` : "—"}
          </span>
        </div>
        <Sparkline values={cpuHistory} width={180} height={24} />
        {latest && (
          <div className="vitals-sub">{latest.cores.length} core</div>
        )}
      </div>
      <div className="vitals-block">
        <div className="vitals-row">
          <span className="vitals-label">RAM</span>
          <span className="vitals-value">
            {latest ? `${latest.mem.usedPct.toFixed(0)}%` : "—"}
          </span>
        </div>
        <Sparkline values={memHistory} width={180} height={24} stroke="var(--accent2)" />
        {latest && (
          <div className="vitals-sub">
            {fmtGb(latest.mem.usedBytes)} / {fmtGb(latest.mem.totalBytes)} GB
          </div>
        )}
      </div>
      <div className="vitals-spacer" />
      <div className="vitals-alerts">
        <div className="vitals-row">
          <span className="vitals-label">Alert</span>
          {alerts.length > 0 && (
            <button className="small" onClick={() => post("/api/alerts/ack", {})}>
              pulisci
            </button>
          )}
        </div>
        {alerts.length === 0 && <div className="vitals-sub">Nessun alert</div>}
        {alerts.slice(0, 5).map((a) => (
          <button
            key={a.id}
            className={`alert-item ${a.severity}`}
            title={`${a.detail} (clic per confermare)`}
            onClick={() => post("/api/alerts/ack", { id: a.id })}
          >
            <span className="alert-title">{a.title}</span>
            <span className="alert-detail">{a.detail}</span>
          </button>
        ))}
      </div>
    </aside>
  );
}
