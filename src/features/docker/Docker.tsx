import { useCallback, useEffect, useRef, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { ws } from "../../lib/ws";
import { TaskLog } from "../../components/TaskLog";
import { LocalNetworkBanner } from "../../components/LocalNetworkBanner";
import type {
  ContainerStat,
  DockerContainer,
  DockerImage,
  DockerState,
  TaskInfo,
} from "../../lib/types";

const REFRESH_MS = 5000;
const STAT_INTERVALS = [2000, 3000, 5000, 10000];

function ImagesPanel() {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [images, setImages] = useState<DockerImage[] | null>(null);
  const [pruning, setPruning] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const fetchImages = useCallback(async () => {
    const r = await api<{ images: DockerImage[] }>("/api/docker/images");
    if (r.ok) setImages(r.data.images);
  }, []);

  useEffect(() => {
    if (!open) return;
    fetchImages();
    const id = setInterval(fetchImages, REFRESH_MS);
    return () => clearInterval(id);
  }, [open, fetchImages]);

  const toggle = () => setOpen((v) => !v);

  const prune = async () => {
    if (!confirm(t("docker.pruneConfirm"))) return;
    setPruning(true);
    setMsg(null);
    const r = await post<{ summary: string }>("/api/docker/images/prune", {});
    setPruning(false);
    if (r.ok) {
      setMsg(r.data.summary?.trim() || t("docker.pruneDone"));
      fetchImages();
    } else setMsg(t("docker.pruneError", { message: r.error.message }));
  };

  const unusedCount = images?.filter((i) => i.unused).length ?? 0;

  return (
    <div className="docker-images-wrap">
      <div className="docker-images-head">
        <button className="small" onClick={toggle}>
          {open ? "▾" : "▸"} {t("docker.images")}
          {images ? ` (${images.length})` : ""}
        </button>
        {open && images && images.length > 0 && (
          <button className="small danger" onClick={prune} disabled={pruning}>
            {pruning
              ? t("docker.pruneBusy")
              : unusedCount
                ? t("docker.pruneUnused", { count: unusedCount })
                : t("docker.prune")}
          </button>
        )}
      </div>
      {msg && <div className="dim docker-prune-msg">{msg}</div>}
      {open && images && (
        <div className="table-scroll">
        <table className="proc-table docker-images">
          <tbody>
            {images.length === 0 && (
              <tr>
                <td className="dim">{t("docker.noImages")}</td>
              </tr>
            )}
            {images.map((img) => (
              <tr key={img.id} className={img.unused ? "img-unused" : ""}>
                <td>
                  {img.repository}
                  <span className="dim">:{img.tag}</span>
                  {img.unused && (
                    <span className="badge badge-warn" title={t("docker.imgUnusedTitle")}>
                      {t("docker.imgUnused")}
                    </span>
                  )}
                </td>
                <td className="num dim">{img.size}</td>
                <td className="num dim">{img.created}</td>
              </tr>
            ))}
          </tbody>
        </table>
        </div>
      )}
    </div>
  );
}

function stateClass(state: string): string {
  if (state === "running") return "docker-state running";
  if (state === "paused" || state === "restarting") return "docker-state warn";
  return "docker-state stopped";
}

type HostHealth = "loading" | "ok" | "down" | "missing";

function healthOf(state: DockerState | null, loadError: string | null): HostHealth {
  if (loadError) return "down";
  if (!state) return "loading";
  if (!state.available) return "missing";
  if (state.daemonDown || state.error) return "down";
  return "ok";
}

function HostBar({
  host,
  health,
  onSaved,
}: {
  host: string | null;
  health: HostHealth;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [value, setValue] = useState(host ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setValue(host ?? ""), [host]);

  const save = async () => {
    setSaving(true);
    setError(null);
    const r = await post<{ host: string | null }>("/api/config/docker-host", {
      host: value.trim() || null,
    });
    setSaving(false);
    if (r.ok) {
      onSaved();
    } else {
      setError(r.error.message);
      setValue(host ?? "");
    }
  };

  const dirty = value.trim() !== (host ?? "");

  return (
    <div className="docker-host">
      <label className="form-row">
        <span title={t("docker.hostDockerTitle")}>{t("docker.hostDocker")}</span>
        <input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={t("docker.hostPlaceholder")}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
          }}
        />
        <button className="small" onClick={save} disabled={saving || !dirty}>
          {saving ? t("docker.testing") : t("common.save")}
        </button>
      </label>
      {error && <div className="banner banner-error">{error}</div>}
      {host && <LocalNetworkBanner what={t("docker.whatEngine")} />}
      <div className={`hint ${health === "down" ? "hint-error" : ""}`}>
        {health === "missing"
          ? t("docker.missingCli")
          : host
            ? health === "loading"
              ? t("docker.contactingRemote", { host })
              : health === "down"
                ? t("docker.remoteUnreachable", { host })
                : t("docker.remoteConnected", { host })
            : health === "down"
              ? t("docker.localDown")
              : t("docker.localHint")}
      </div>
    </div>
  );
}

function ContainerRow({
  container,
  stat,
  onChanged,
  onLogs,
}: {
  container: DockerContainer;
  stat?: ContainerStat;
  onChanged: () => void;
  onLogs: (task: TaskInfo, name: string) => void;
}) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const running = container.state === "running";

  const act = async (action: "start" | "stop" | "restart") => {
    setBusy(true);
    setError(null);
    const r = await post(`/api/docker/${encodeURIComponent(container.id)}/action`, { action });
    setBusy(false);
    if (r.ok) onChanged();
    else setError(r.error.message);
  };

  const showLogs = async () => {
    const r = await post<TaskInfo>(`/api/docker/${encodeURIComponent(container.id)}/logs`, {});
    if (r.ok) onLogs(r.data, container.name);
    else setError(r.error.message);
  };

  return (
    <>
      {
}
      <tr className="docker-row" onClick={() => setExpanded(!expanded)}>
        <td className="docker-name-cell">
          <span className={stateClass(container.state)} title={container.state} />
          <span className="docker-name">{container.name}</span>
          {running && stat && (
            <span className="docker-stat" title={`CPU ${stat.cpuPct}% · MEM ${stat.memUsage}`}>
              <span className="docker-stat-metric">
                <span className="docker-stat-label">CPU</span>
                {stat.cpuPct.toFixed(0)}%
              </span>
              <span className="docker-stat-metric">
                <span className="docker-stat-label">MEM</span>
                {stat.memPct.toFixed(0)}%
              </span>
            </span>
          )}
        </td>
        <td className="docker-actions-cell">
          {}
          <div className="docker-actions" onClick={(e) => e.stopPropagation()}>
            {running ? (
              <>
                <button
                  className="docker-btn restart"
                  disabled={busy}
                  title={t("docker.restart")}
                  aria-label={t("docker.restart")}
                  onClick={() => act("restart")}
                >
                  ↻
                </button>
                <button
                  className="docker-btn stop"
                  disabled={busy}
                  title={t("docker.stop")}
                  aria-label={t("docker.stop")}
                  onClick={() => act("stop")}
                >
                  ◼
                </button>
              </>
            ) : (
              <button
                className="docker-btn start"
                disabled={busy}
                title={t("docker.start")}
                aria-label={t("docker.start")}
                onClick={() => act("start")}
              >
                ▶
              </button>
            )}
            <button
              className="docker-btn logs"
              title={t("docker.logsBtn")}
              aria-label={t("docker.logsBtn")}
              onClick={showLogs}
            >
              ≣
            </button>
          </div>
          <span className="docker-expand dim" aria-hidden>
            {expanded ? "▾" : "▸"}
          </span>
        </td>
      </tr>
      {expanded && (
        <tr className="docker-detail">
          <td colSpan={2}>
            <dl className="docker-detail-grid">
              <div>
                <dt>{t("docker.image")}</dt>
                <dd className="docker-image" title={container.image}>
                  {container.image}
                </dd>
              </div>
              <div>
                <dt>{t("docker.statusLabel")}</dt>
                <dd className="dim">{container.status}</dd>
              </div>
              <div>
                <dt>{t("docker.portsLabel")}</dt>
                <dd className="dim">
                  {container.ports.length > 0 ? container.ports.join(", ") : t("common.none")}
                </dd>
              </div>
              {running && stat && (
                <div>
                  <dt>{t("docker.resources")}</dt>
                  <dd className="dim">
                    CPU {stat.cpuPct.toFixed(1)}% · MEM {stat.memPct.toFixed(1)}% ({stat.memUsage})
                  </dd>
                </div>
              )}
            </dl>
          </td>
        </tr>
      )}
      {error && (
        <tr className="docker-detail">
          <td colSpan={2}>
            <div className="banner banner-error docker-row-error">{error}</div>
          </td>
        </tr>
      )}
    </>
  );
}

export function Docker() {
  const { t } = useTranslation();
  const [state, setState] = useState<DockerState | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [logsFor, setLogsFor] = useState<{ task: TaskInfo; name: string } | null>(null);
  const [stats, setStats] = useState<Record<string, ContainerStat>>({});
  const [statInterval, setStatInterval] = useState(3000);
  const inFlight = useRef(false);

  const load = useCallback(async () => {
    // 20260704 RG con un host remoto lento la risposta supera l'intervallo di refresh:
    // senza guardia le richieste si accavallano, ognuna con il suo `docker ps`.
    if (inFlight.current) return;
    inFlight.current = true;
    const r = await api<DockerState>("/api/docker");
    inFlight.current = false;
    if (r.ok) {
      setState(r.data);
      setLoadError(null);
    } else {
      setLoadError(r.error.message);
    }
  }, []);

  useEffect(() => {
    load();
    const id = setInterval(load, REFRESH_MS);
    return () => clearInterval(id);
  }, [load]);

  useEffect(() => {
    return ws.subscribe("docker:stats", (event) => {
      const payload = event.payload as { stats?: ContainerStat[] };
      const map: Record<string, ContainerStat> = {};
      for (const s of payload.stats ?? []) {
        if (s.id) map[s.id] = s;
        if (s.name) map[s.name] = s;
      }
      setStats(map);
    });
  }, []);

  const changeStatInterval = (ms: number) => {
    setStatInterval(ms);
    post("/api/pollers/docker:stats/interval", { intervalMs: ms });
  };

  const openLogTaskId = logsFor?.task.id;
  useEffect(() => {
    if (!openLogTaskId) return;
    return () => {
      post(`/api/tasks/${openLogTaskId}/stop`, {});
    };
  }, [openLogTaskId]);

  const health = healthOf(state, loadError);

  return (
    <div className="docker-tool">
      <div className="docker-toolbar">
        <div className="segmented" title={t("docker.statsInterval")}>
          {STAT_INTERVALS.map((ms) => (
            <button
              key={ms}
              className={statInterval === ms ? "active" : ""}
              onClick={() => changeStatInterval(ms)}
            >
              {ms / 1000}s
            </button>
          ))}
        </div>
        <button className="small" onClick={load}>
          {t("common.refresh")}
        </button>
      </div>

      <HostBar host={state?.host ?? null} health={health} onSaved={load} />

      {loadError && (
        <div className="banner banner-error">{t("docker.readError", { message: loadError })}</div>
      )}

      {!state && !loadError && <div className="empty">{t("docker.checking")}</div>}

      {state && !state.available && (
        <div className="empty">
          <Trans i18nKey="docker.notInstalled" components={{ code: <code /> }} />
        </div>
      )}

      {
}
      {state && state.available && (state.daemonDown || state.error) && (
        <div className="banner banner-error">
          <div>
            {state.daemonDown
              ? state.host
                ? t("docker.remoteDaemonDown", { host: state.host })
                : t("docker.localDaemonDown")
              : state.host
                ? t("docker.remoteReadError", { host: state.host })
                : t("docker.contactError")}
          </div>
          {state.error && (
            <pre className="docker-error-detail">
              {state.host ? `docker -H ${state.host} …` : "docker …"} → {state.error}
            </pre>
          )}
        </div>
      )}

      {state && state.available && !state.daemonDown && !state.error && state.containers.length === 0 && (
        <div className="empty">{t("docker.noContainers")}</div>
      )}

      {state && state.containers.length > 0 && (
        <table className="proc-table docker-table">
          <thead>
            <tr>
              <th>{t("docker.container")}</th>
              <th className="num">{t("docker.actions")}</th>
            </tr>
          </thead>
          <tbody>
            {state.containers.map((c) => (
              <ContainerRow
                key={c.id}
                container={c}
                stat={stats[c.id] ?? stats[c.name]}
                onChanged={load}
                onLogs={(task, name) => setLogsFor({ task, name })}
              />
            ))}
          </tbody>
        </table>
      )}

      {state && state.available && !state.daemonDown && !state.error && <ImagesPanel />}

      {logsFor && (
        <div className="docker-logs">
          <div className="section-header">
            <h3>{t("docker.logs", { name: logsFor.name })}</h3>
            <button className="small" onClick={() => setLogsFor(null)}>
              {t("common.close")}
            </button>
          </div>
          <TaskLog key={logsFor.task.id} task={logsFor.task} />
        </div>
      )}
    </div>
  );
}
