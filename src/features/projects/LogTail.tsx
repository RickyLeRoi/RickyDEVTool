import { useEffect, useRef, useState } from "react";
import { post } from "../../lib/api";
import { ws } from "../../lib/ws";
import type { ApiError, TailInfo } from "../../lib/types";

interface TailEvent {
  event: "line" | "error";
  line?: string;
  message?: string;
}

/** Segue un file di log (tail -f) con streaming via WS, come i task. */
export function LogTail({ projectPath }: { projectPath: string }) {
  const [open, setOpen] = useState(false);
  const [file, setFile] = useState("");
  const [tail, setTail] = useState<TailInfo | null>(null);
  const [lines, setLines] = useState<string[]>([]);
  const [error, setError] = useState<ApiError | null>(null);
  const boxRef = useRef<HTMLPreElement>(null);

  useEffect(() => {
    if (!tail) return;
    setLines([]);
    return ws.subscribe(`tail:${tail.id}`, (event) => {
      const p = event.payload as TailEvent;
      if (p.event === "line" && p.line !== undefined) {
        setLines((prev) => [...prev.slice(-999), p.line as string]);
      }
    });
  }, [tail]);

  useEffect(() => {
    const box = boxRef.current;
    if (box) box.scrollTop = box.scrollHeight;
  }, [lines]);

  const start = async () => {
    setError(null);
    // Path assoluto o relativo al progetto.
    const path = file.startsWith("/") ? file : `${projectPath}/${file}`;
    const r = await post<TailInfo>("/api/logtail/start", { path });
    if (r.ok) setTail(r.data);
    else setError(r.error);
  };

  const stop = async () => {
    if (tail) await post(`/api/logtail/${tail.id}/stop`, {});
    setTail(null);
  };

  return (
    <div className="logtail">
      <button className="link-btn" onClick={() => setOpen(!open)}>
        {open ? "▾" : "▸"} Segui un log
      </button>
      {open && (
        <div className="logtail-body">
          {!tail ? (
            <form
              className="net-form"
              onSubmit={(e) => {
                e.preventDefault();
                start();
              }}
            >
              <input
                value={file}
                onChange={(e) => setFile(e.target.value)}
                placeholder="es. logs/app.log oppure /var/log/…"
              />
              <button disabled={!file.trim()}>Avvia</button>
            </form>
          ) : (
            <div className="task-log">
              <div className="task-log-header">
                <span className="dim">{tail.path}</span>
                <button className="danger small" onClick={stop}>
                  Stop
                </button>
              </div>
              <pre ref={boxRef} className="task-log-box">
                {lines.map((l, i) => (
                  <div key={i}>{l}</div>
                ))}
                {lines.length === 0 && <span className="dim">in attesa di nuove righe…</span>}
              </pre>
            </div>
          )}
          {error && (
            <div className="banner banner-error">
              {error.message}
              {error.osHint && <div className="hint">{error.osHint}</div>}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
