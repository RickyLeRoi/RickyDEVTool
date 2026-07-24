import { useCallback, useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { TaskLog } from "../../components/TaskLog";
import type { DockerContainer, DockerImage, DockerState, TaskInfo } from "../../lib/types";

const REFRESH_MS = 5000;

function ImagesPanel() {
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
    if (
      !confirm(
        "Rimuovere tutte le immagini non usate da nessun container (docker image prune -a)?\nLe immagini in uso non vengono toccate.",
      )
    )
      return;
    setPruning(true);
    setMsg(null);
    const r = await post<{ summary: string }>("/api/docker/images/prune", {});
    setPruning(false);
    if (r.ok) {
      setMsg(r.data.summary?.trim() || "Prune completato.");
      fetchImages();
    } else setMsg(`Errore: ${r.error.message}`);
  };

  const unusedCount = images?.filter((i) => i.unused).length ?? 0;

  return (
    <div className="docker-images-wrap">
      <div className="docker-images-head">
        <button className="small" onClick={toggle}>
          {open ? "▾" : "▸"} Immagini{images ? ` (${images.length})` : ""}
        </button>
        {open && images && images.length > 0 && (
          <button className="small danger" onClick={prune} disabled={pruning}>
            {pruning ? "Prune…" : unusedCount ? `Prune (${unusedCount} non usate)` : "Prune"}
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
                <td className="dim">Nessuna immagine.</td>
              </tr>
            )}
            {images.map((img) => (
              <tr key={img.id} className={img.unused ? "img-unused" : ""}>
                <td>
                  {img.repository}
                  <span className="dim">:{img.tag}</span>
                  {img.unused && (
                    <span className="badge badge-warn" title="Non usata da nessun container">
                      non usata
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
      {/* Riga compatta: nome + azioni sempre visibili (anche da remoto su schermi
          stretti); il resto — immagine, stato, porte — nel dettaglio al click. */}
      <tr className="docker-row" onClick={() => setExpanded(!expanded)}>
        <td className="docker-name-cell">
          <span className={stateClass(container.state)} title={container.state} />
          <span className="docker-name">{container.name}</span>
        </td>
        <td className="docker-actions-cell">
          {/* I click sulle azioni non devono aprire/chiudere il dettaglio. */}
          <div className="docker-actions" onClick={(e) => e.stopPropagation()}>
            {running ? (
              <>
                <button
                  className="docker-btn restart"
                  disabled={busy}
                  title="Restart"
                  aria-label="Restart"
                  onClick={() => act("restart")}
                >
                  ↻
                </button>
                <button
                  className="docker-btn stop"
                  disabled={busy}
                  title="Stop"
                  aria-label="Stop"
                  onClick={() => act("stop")}
                >
                  ◼
                </button>
              </>
            ) : (
              <button
                className="docker-btn start"
                disabled={busy}
                title="Start"
                aria-label="Start"
                onClick={() => act("start")}
              >
                ▶
              </button>
            )}
            <button className="docker-btn logs" title="Logs" aria-label="Logs" onClick={showLogs}>
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
                <dt>Immagine</dt>
                <dd className="docker-image" title={container.image}>
                  {container.image}
                </dd>
              </div>
              <div>
                <dt>Stato</dt>
                <dd className="dim">{container.status}</dd>
              </div>
              <div>
                <dt>Porte</dt>
                <dd className="dim">
                  {container.ports.length > 0 ? container.ports.join(", ") : "—"}
                </dd>
              </div>
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

      {/* Il comando docker è fallito: se c'è un errore lo mostriamo per intero
          (soprattutto per gli host remoti ssh://, dove il motivo — host key,
          chiave, risoluzione nome — è nell'stderr). Senza questo sembrava
          "0 container". */}
      {state && state.available && (state.daemonDown || state.error) && (
        <div className="banner banner-error">
          <div>
            {state.daemonDown
              ? state.host
                ? `Non riesco a contattare il Docker remoto (${state.host}). Verifica che l'host sia raggiungibile e che il daemon sia attivo.`
                : "Docker è installato ma il demone non risponde. Avvia Docker Desktop (o il tuo runtime) e riprova."
              : state.host
                ? `Non riesco a leggere il Docker remoto (${state.host}).`
                : "Errore nel contattare Docker."}
          </div>
          {state.error && (
            <pre className="docker-error-detail">
              docker -H {state.host} … → {state.error}
            </pre>
          )}
        </div>
      )}

      {state && state.available && !state.daemonDown && !state.error && state.containers.length === 0 && (
        <div className="empty">Nessun container (né attivo né fermo).</div>
      )}

      {state && state.containers.length > 0 && (
        <table className="proc-table docker-table">
          <thead>
            <tr>
              <th>Container</th>
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

      {state && state.available && !state.daemonDown && !state.error && <ImagesPanel />}

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
