import { useCallback, useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { TaskLog } from "../../components/TaskLog";
import type { DockerContainer, DockerImage, DockerState, TaskInfo } from "../../lib/types";

const REFRESH_MS = 5000;

function ImagesPanel() {
  const [open, setOpen] = useState(false);
  const [images, setImages] = useState<DockerImage[] | null>(null);

  const toggle = async () => {
    if (!open && !images) {
      const r = await api<{ images: DockerImage[] }>("/api/docker/images");
      if (r.ok) setImages(r.data.images);
    }
    setOpen(!open);
  };

  return (
    <div className="docker-images-wrap">
      <button className="small" onClick={toggle}>
        {open ? "▾" : "▸"} Immagini{images ? ` (${images.length})` : ""}
      </button>
      {open && images && (
        <table className="proc-table docker-images">
          <tbody>
            {images.length === 0 && (
              <tr>
                <td className="dim">Nessuna immagine.</td>
              </tr>
            )}
            {images.map((img) => (
              <tr key={img.id}>
                <td>
                  {img.repository}
                  <span className="dim">:{img.tag}</span>
                </td>
                <td className="num dim">{img.size}</td>
                <td className="num dim">{img.created}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

function stateClass(state: string): string {
  if (state === "running") return "docker-state running";
  if (state === "paused" || state === "restarting") return "docker-state warn";
  return "docker-state stopped";
}

// Configurazione dell'host Docker: vuoto = daemon locale; altrimenti si punta a
// un Docker remoto (es. una VM sul server di casa) via ssh:// o tcp://.
function HostBar({ host, onSaved }: { host: string | null; onSaved: () => void }) {
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
    if (r.ok) onSaved();
    else setError(r.error.message);
  };

  const dirty = value.trim() !== (host ?? "");

  return (
    <div className="docker-host">
      <label className="form-row">
        <span title="Vuoto = daemon locale">Host Docker</span>
        <input
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder="ssh://user@host  ·  tcp://ip:2375  (vuoto = locale)"
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
          }}
        />
        <button className="small" onClick={save} disabled={saving || !dirty}>
          {saving ? "Salvo…" : "Salva"}
        </button>
      </label>
      {error && <div className="banner banner-error">{error}</div>}
      <div className="hint">
        {host
          ? `Puntando a un host remoto: ${host}`
          : "Daemon locale. Per un Docker su un'altra macchina (es. VM Proxmox) inserisci ssh:// o tcp:// (serve comunque la CLI docker su questo computer)."}
      </div>
    </div>
  );
}

function ContainerRow({
  container,
  onChanged,
  onLogs,
}: {
  container: DockerContainer;
  onChanged: () => void;
  onLogs: (task: TaskInfo, name: string) => void;
}) {
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
    <tr>
      <td>
        <span className={stateClass(container.state)} title={container.state} />
        <span className="docker-name">{container.name}</span>
      </td>
      <td className="dim docker-image" title={container.image}>
        {container.image}
      </td>
      <td className="dim">{container.status}</td>
      <td className="dim docker-ports">
        {container.ports.length > 0 ? container.ports.join(", ") : "—"}
      </td>
      <td className="num docker-actions">
        {running ? (
          <>
            <button className="small" disabled={busy} onClick={() => act("restart")}>
              Restart
            </button>
            <button className="small danger" disabled={busy} onClick={() => act("stop")}>
              Stop
            </button>
          </>
        ) : (
          <button className="small" disabled={busy} onClick={() => act("start")}>
            Start
          </button>
        )}
        <button className="small ghost" onClick={showLogs}>
          Logs
        </button>
        {error && <div className="banner banner-error docker-row-error">{error}</div>}
      </td>
    </tr>
  );
}

export function Docker() {
  const [state, setState] = useState<DockerState | null>(null);
  const [logsFor, setLogsFor] = useState<{ task: TaskInfo; name: string } | null>(null);

  const load = useCallback(async () => {
    const r = await api<DockerState>("/api/docker");
    if (r.ok) setState(r.data);
  }, []);

  useEffect(() => {
    load();
    const id = setInterval(load, REFRESH_MS);
    return () => clearInterval(id);
  }, [load]);

  // `docker logs -f` non termina da solo: va fermato non solo alla chiusura
  // esplicita ma anche quando si passa a un altro container o si lascia la
  // sezione (unmount), altrimenti il processo di follow resta appeso.
  const openLogTaskId = logsFor?.task.id;
  useEffect(() => {
    if (!openLogTaskId) return;
    return () => {
      post(`/api/tasks/${openLogTaskId}/stop`, {});
    };
  }, [openLogTaskId]);

  return (
    <div>
      <div className="section-header">
        <h2>Docker</h2>
        <button className="small" onClick={load}>
          Aggiorna
        </button>
      </div>

      <HostBar host={state?.host ?? null} onSaved={load} />

      {!state && <div className="empty">Controllo Docker…</div>}

      {state && !state.available && (
        <div className="empty">
          Docker non è installato (o la CLI <code>docker</code> non è nel PATH).
        </div>
      )}

      {state && state.available && state.daemonDown && (
        <div className="banner banner-error">
          {state.host
            ? `Non riesco a contattare il Docker remoto (${state.host}). Verifica che l'host sia raggiungibile e che il daemon sia attivo.`
            : "Docker è installato ma il demone non risponde. Avvia Docker Desktop (o il tuo runtime) e riprova."}
        </div>
      )}

      {state && state.available && !state.daemonDown && state.containers.length === 0 && (
        <div className="empty">Nessun container (né attivo né fermo).</div>
      )}

      {state && state.containers.length > 0 && (
        <table className="proc-table docker-table">
          <thead>
            <tr>
              <th>Nome</th>
              <th>Immagine</th>
              <th>Stato</th>
              <th>Porte</th>
              <th className="num">Azioni</th>
            </tr>
          </thead>
          <tbody>
            {state.containers.map((c) => (
              <ContainerRow
                key={c.id}
                container={c}
                onChanged={load}
                onLogs={(task, name) => setLogsFor({ task, name })}
              />
            ))}
          </tbody>
        </table>
      )}

      {state && state.available && !state.daemonDown && <ImagesPanel />}

      {logsFor && (
        <div className="docker-logs">
          <div className="section-header">
            <h3>Log · {logsFor.name}</h3>
            <button className="small" onClick={() => setLogsFor(null)}>
              {/* Lo stop del follow è gestito dall'effect di cleanup su logsFor. */}
              Chiudi
            </button>
          </div>
          <TaskLog key={logsFor.task.id} task={logsFor.task} />
        </div>
      )}
    </div>
  );
}
