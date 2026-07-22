import { useCallback, useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import type { ClipboardHistory, ClipEntry } from "../../lib/types";

const REFRESH_MS = 2000;
const PREVIEW_CHARS = 280;

function fmtTime(ms: number) {
  return new Date(ms).toLocaleTimeString("it-IT", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function fmtBytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

function Entry({
  entry,
  onChanged,
}: {
  entry: ClipEntry;
  onChanged: (h: ClipboardHistory) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);
  const long = entry.text.length > PREVIEW_CHARS;
  const shown = expanded || !long ? entry.text : entry.text.slice(0, PREVIEW_CHARS) + "…";

  const copy = async () => {
    const r = await post<{ copied: boolean }>("/api/clipboard/copy", { id: entry.id });
    if (r.ok) {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    }
  };
  const pin = async () => {
    const r = await post<ClipboardHistory>("/api/clipboard/pin", {
      id: entry.id,
      pinned: !entry.pinned,
    });
    if (r.ok) onChanged(r.data);
  };
  const del = async () => {
    const r = await post<ClipboardHistory>("/api/clipboard/delete", { id: entry.id });
    if (r.ok) onChanged(r.data);
  };

  return (
    <li className={`clip-entry ${entry.pinned ? "pinned" : ""}`}>
      <div className="clip-meta">
        <span className="dim">{fmtTime(entry.copiedAt)}</span>
        <span className="dim">{fmtBytes(entry.bytes)}</span>
        <span className="clip-actions">
          <button className="small" onClick={copy} title="Copia negli appunti">
            {copied ? "Copiato ✓" : "Copia"}
          </button>
          <button
            className={`small ${entry.pinned ? "" : "ghost"}`}
            onClick={pin}
            title={entry.pinned ? "Rimuovi dai fissati" : "Fissa"}
          >
            {entry.pinned ? "📌" : "📍"}
          </button>
          <button className="small ghost" onClick={del} title="Elimina">
            ✕
          </button>
        </span>
      </div>
      <pre className="clip-text" onClick={() => long && setExpanded(!expanded)}>
        {shown}
      </pre>
      {long && (
        <button className="small ghost clip-expand" onClick={() => setExpanded(!expanded)}>
          {expanded ? "comprimi" : `mostra tutto (${entry.text.length} caratteri)`}
        </button>
      )}
    </li>
  );
}

export function Clipboard() {
  const [hist, setHist] = useState<ClipboardHistory | null>(null);

  const load = useCallback(async () => {
    const r = await api<ClipboardHistory>("/api/clipboard/history");
    if (r.ok) setHist(r.data);
  }, []);

  useEffect(() => {
    load();
    const id = setInterval(load, REFRESH_MS);
    return () => clearInterval(id);
  }, [load]);

  const toggleEnabled = async () => {
    if (!hist) return;
    const r = await post<{ enabled: boolean }>("/api/clipboard/enabled", {
      enabled: !hist.enabled,
    });
    if (r.ok) setHist({ ...hist, enabled: r.data.enabled });
  };

  const clear = async (keepPinned: boolean) => {
    const r = await post<ClipboardHistory>("/api/clipboard/clear", { keepPinned });
    if (r.ok) setHist(r.data);
  };

  return (
    <div>
      <div className="section-header">
        <h2>Storico appunti</h2>
        {hist && (
          <div className="clip-toolbar">
            <button
              className={hist.enabled ? "small" : "small danger"}
              onClick={toggleEnabled}
              title={hist.enabled ? "Metti in pausa la cattura" : "Riprendi la cattura"}
            >
              {hist.enabled ? "⏸ In pausa" : "▶ Riprendi"}
            </button>
            <button className="small" onClick={() => clear(true)} title="Svuota, tieni i fissati">
              Svuota
            </button>
            <button className="small danger" onClick={() => clear(false)} title="Svuota tutto">
              Svuota tutto
            </button>
          </div>
        )}
      </div>

      <p className="hint">
        La cronologia vive solo in memoria: non è salvata su disco e sparisce a ogni riavvio.
        {hist && !hist.enabled && " — cattura in pausa."}
      </p>

      {hist && !hist.supported && (
        <div className="banner banner-error">
          Gli appunti di sistema non sono accessibili su questo sistema operativo.
        </div>
      )}

      {hist && hist.supported && hist.entries.length === 0 && (
        <div className="empty">
          Nessuna copia registrata: copia qualcosa (da qualsiasi app) e comparirà qui.
        </div>
      )}

      {hist && hist.entries.length > 0 && (
        <ul className="clip-list">
          {hist.entries.map((e) => (
            <Entry key={e.id} entry={e} onChanged={setHist} />
          ))}
        </ul>
      )}
    </div>
  );
}
