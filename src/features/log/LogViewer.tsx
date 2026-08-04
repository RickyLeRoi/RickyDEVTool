import { useCallback, useEffect, useRef, useState } from "react";
import { api, post } from "../../lib/api";
import { ws } from "../../lib/ws";
import type { ApiError, TailInfo } from "../../lib/types";

interface TailEvent {
  event: "line" | "error" | "rotated";
  line?: string;
  message?: string;
}

function baseName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

export function LogViewer() {
  const [tails, setTails] = useState<TailInfo[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [path, setPath] = useState("");
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<ApiError | null>(null);
  const [busy, setBusy] = useState(false);
  const boxRef = useRef<HTMLPreElement>(null);

  const loadList = useCallback(async () => {
    const r = await api<{ tails: TailInfo[] }>("/api/logtail");
    if (r.ok) {
      const list = r.data.tails ?? [];
      setTails(list);
      setActiveId((cur) => {
        if (cur && list.some((t) => t.id === cur)) return cur;
        return list[0]?.id ?? null;
      });
    }
  }, []);

  useEffect(() => {
    loadList();
  }, [loadList]);

  useEffect(() => {
    if (!activeId) {
      setLines([]);
      return;
    }
    setLines([]);
    return ws.subscribe(`tail:${activeId}`, (event) => {
      const p = event.payload as TailEvent;
      if (p.event === "line" && p.line !== undefined) {
        setLines((prev) => [...prev.slice(-1999), p.line as string]);
      } else if (p.event === "rotated") {
        setLines((prev) => [...prev.slice(-1999), "— file ruotato, riparto dall'inizio —"]);
      } else if (p.event === "error" && p.message) {
        setError({ code: "TAIL", message: p.message, retryable: false });
      }
    });
  }, [activeId]);

  useEffect(() => {
    const box = boxRef.current;
    if (box) box.scrollTop = box.scrollHeight;
  }, [lines]);

  const start = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!path.trim()) return;
    setBusy(true);
    setError(null);
    const r = await post<TailInfo>("/api/logtail/start", { path: path.trim() });
    setBusy(false);
    if (r.ok) {
      setPath("");
      await loadList();
      setActiveId(r.data.id);
    } else setError(r.error);
  };

  const stop = async (id: string) => {
    await post(`/api/logtail/${id}/stop`, {});
    await loadList();
  };

  const active = tails.find((t) => t.id === activeId) ?? null;

  return (
    <div className="logviewer">
      <div className="section-header">
        <h2>Log</h2>
        <span className="dim">{tails.length}/5 in ascolto</span>
      </div>

      <form className="net-form" onSubmit={start}>
        <input
          value={path}
          onChange={(e) => setPath(e.target.value)}
          placeholder="percorso assoluto del file — es. /var/log/system.log"
        />
        <button disabled={busy || !path.trim()}>{busy ? "Avvio…" : "Segui"}</button>
      </form>

      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}

      {tails.length === 0 && (
        <div className="empty">
          Nessun log seguito. Inserisci il percorso di un file per iniziare il tail -f.
        </div>
      )}

      {tails.length > 0 && (
        <div className="log-tabs">
          {tails.map((t) => (
            <div
              key={t.id}
              className={`log-tab ${t.id === activeId ? "active" : ""}`}
              title={t.path}
            >
              <button className="log-tab-name" onClick={() => setActiveId(t.id)}>
                📄 {baseName(t.path)}
              </button>
              <button className="log-tab-close" title="Ferma" onClick={() => stop(t.id)}>
                ✕
              </button>
            </div>
          ))}
        </div>
      )}

      {active && (
        <div className="task-log">
          <div className="task-log-header">
            <span className="dim">{active.path}</span>
          </div>
          <pre ref={boxRef} className="task-log-box">
            {lines.map((l, i) => (
              <div key={i}>{l}</div>
            ))}
            {lines.length === 0 && <span className="dim">in attesa di nuove righe…</span>}
          </pre>
        </div>
      )}
    </div>
  );
}
