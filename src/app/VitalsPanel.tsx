import { useEffect, useState } from "react";
import { useStatsStore } from "../stores/statsStore";
import { Sparkline } from "../components/Sparkline";
import { ws } from "../lib/ws";
import { api, post } from "../lib/api";
import type { Alert, DockerState, LaunchBundle } from "../lib/types";

function fmtGb(bytes: number) {
  return (bytes / 1024 ** 3).toFixed(1);
}

// Poll lento: lo shortcut Docker è un indicatore, non un monitor. Su host remoto
// ssh:// non vogliamo aprire una connessione ogni pochi secondi.
const DOCKER_POLL_MS = 60_000;

/** Pannello destro sempre visibile: CPU/RAM, avvii rapidi, Docker e alert. */
export function VitalsPanel({ onNavigate }: { onNavigate?: (section: string) => void }) {
  const { latest, cpuHistory, memHistory } = useStatsStore();
  const [alerts, setAlerts] = useState<Alert[]>([]);
  const [bundles, setBundles] = useState<LaunchBundle[]>([]);
  const [ranBundle, setRanBundle] = useState<string | null>(null);
  const [docker, setDocker] = useState<DockerState | null>(null);

  useEffect(() => {
    api<{ alerts: Alert[] }>("/api/alerts").then((r) => {
      if (r.ok) setAlerts(r.data.alerts);
    });
    return ws.subscribe("alerts", (event) => {
      setAlerts((event.payload as { alerts: Alert[] }).alerts);
    });
  }, []);

  // Profili di avvio: se ne hai configurati, li mostro qui come quick-launch.
  useEffect(() => {
    api<{ bundles: LaunchBundle[] }>("/api/launch/bundles").then((r) => {
      if (r.ok) setBundles(r.data.bundles ?? []);
    });
  }, []);

  // Stato Docker: lo shortcut appare solo se Docker è attivo.
  useEffect(() => {
    let alive = true;
    const load = () =>
      api<DockerState>("/api/docker").then((r) => {
        if (alive && r.ok) setDocker(r.data);
      });
    load();
    const id = setInterval(load, DOCKER_POLL_MS);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  const runBundle = async (b: LaunchBundle) => {
    const r = await post("/api/launch/run", { id: b.id });
    if (r.ok) {
      setRanBundle(b.id);
      setTimeout(() => setRanBundle(null), 1500);
    }
  };

  const dockerActive = !!docker?.available && !docker.daemonDown && !docker.error;
  const dockerRunning = docker?.containers?.filter((c) => c.state === "running").length ?? 0;
  const dockerTotal = docker?.containers?.length ?? 0;

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
        {latest && <div className="vitals-sub">{latest.cores.length} core</div>}
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

      {bundles.length > 0 && (
        <div className="vitals-block vitals-section">
          <button
            className="vitals-heading"
            onClick={() => onNavigate?.("launch")}
            title="Vai agli Avvii"
          >
            Avvii rapidi ›
          </button>
          <div className="vitals-chips">
            {bundles.slice(0, 6).map((b) => (
              <button
                key={b.id}
                className="vitals-chip"
                title={`Avvia "${b.name}"`}
                onClick={() => runBundle(b)}
              >
                {ranBundle === b.id ? "avviato ✓" : `▶ ${b.name}`}
              </button>
            ))}
          </div>
        </div>
      )}

      {dockerActive && (
        <div className="vitals-block vitals-section">
          <button
            className="vitals-shortcut"
            onClick={() => onNavigate?.("docker")}
            title="Apri Docker"
          >
            <span className="vitals-label">🐳 Docker ›</span>
            <span className="vitals-value">
              {dockerRunning}/{dockerTotal}
            </span>
          </button>
          <div className="vitals-sub">
            {dockerRunning} attivi su {dockerTotal}
            {docker?.host ? " · remoto" : ""}
          </div>
        </div>
      )}

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
