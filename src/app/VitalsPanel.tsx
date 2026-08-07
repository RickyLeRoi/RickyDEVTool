import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useStatsStore } from "../stores/statsStore";
import { Sparkline } from "../components/Sparkline";
import { ws } from "../lib/ws";
import { api, post } from "../lib/api";
import { FLASH_MS, POLL_MS } from "../lib/constants";
import { SERIES_COLORS, VITALS_SPARKLINE } from "../lib/styles";
import type { Alert, DockerState, LaunchBundle } from "../lib/types";

function fmtGb(bytes: number) {
  return (bytes / 1024 ** 3).toFixed(1);
}

function alertTarget(kind: string): string | null {
  if (kind === "service-down" || kind.startsWith("cert")) return "services";
  if (kind === "task-failed") return "tasks";
  if (kind === "cpu-sustained" || kind === "mem-high" || kind === "temp-high" || kind === "battery-low")
    return "dashboard";
  return null;
}

export function VitalsPanel({ onNavigate }: { onNavigate?: (section: string) => void }) {
  const { t } = useTranslation();
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

  useEffect(() => {
    api<{ bundles: LaunchBundle[] }>("/api/launch/bundles").then((r) => {
      if (r.ok) setBundles(r.data.bundles ?? []);
    });
  }, []);

  useEffect(() => {
    let alive = true;
    const load = () =>
      api<DockerState>("/api/docker").then((r) => {
        if (alive && r.ok) setDocker(r.data);
      });
    load();
    const id = setInterval(load, POLL_MS.dockerVitals);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  const runBundle = async (b: LaunchBundle) => {
    const r = await post("/api/launch/run", { id: b.id });
    if (r.ok) {
      setRanBundle(b.id);
      setTimeout(() => setRanBundle(null), FLASH_MS);
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
        <Sparkline values={cpuHistory} {...VITALS_SPARKLINE} />
        {latest && (
          <div className="vitals-sub">{t("vitals.cores", { count: latest.cores.length })}</div>
        )}
      </div>
      <div className="vitals-block">
        <div className="vitals-row">
          <span className="vitals-label">RAM</span>
          <span className="vitals-value">
            {latest ? `${latest.mem.usedPct.toFixed(0)}%` : "—"}
          </span>
        </div>
        <Sparkline values={memHistory} {...VITALS_SPARKLINE} stroke={SERIES_COLORS.mem} />
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
            title={t("vitals.goToLaunches")}
          >
            {t("vitals.quickLaunches")}
          </button>
          <div className="vitals-chips">
            {bundles.slice(0, 6).map((b) => (
              <button
                key={b.id}
                className="vitals-chip"
                title={t("vitals.runBundleTitle", { name: b.name })}
                onClick={() => runBundle(b)}
              >
                {ranBundle === b.id ? t("vitals.started") : t("vitals.bundleChip", { name: b.name })}
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
            title={t("vitals.openDocker")}
          >
            <span className="vitals-label">🐳 Docker ›</span>
            <span className="vitals-value">
              {dockerRunning}/{dockerTotal}
            </span>
          </button>
          <div className="vitals-sub">
            {t("vitals.dockerActiveOf", { running: dockerRunning, total: dockerTotal })}
            {docker?.host ? t("vitals.dockerRemote") : ""}
          </div>
        </div>
      )}

      <div className="vitals-spacer" />
      <div className="vitals-alerts">
        <div className="vitals-row">
          <span className="vitals-label">{t("vitals.alerts")}</span>
          {alerts.length > 0 && (
            <button className="small" onClick={() => post("/api/alerts/ack", {})}>
              {t("vitals.clear")}
            </button>
          )}
        </div>
        {alerts.length === 0 && <div className="vitals-sub">{t("vitals.noAlerts")}</div>}
        {alerts.slice(0, 5).map((a) => {
          const target = alertTarget(a.kind);
          return (
            <button
              key={a.id}
              className={`alert-item ${a.severity}`}
              title={
                target
                  ? t("vitals.alertGoConfirm", { detail: a.detail })
                  : t("vitals.alertConfirm", { detail: a.detail })
              }
              onClick={() => {
                if (target) onNavigate?.(target);
                post("/api/alerts/ack", { id: a.id });
              }}
            >
              <span className="alert-title">{a.title}</span>
              <span className="alert-detail">{a.detail}</span>
            </button>
          );
        })}
      </div>
    </aside>
  );
}
