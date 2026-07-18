import { API_BASE, post } from "../../lib/api";
import { fmtBytes } from "../../lib/format";
import { useDropStore } from "../../stores/dropStore";

/** Notifiche fluttuanti per file/testo in arrivo, visibili da qualsiasi sezione. */
export function DropToasts() {
  const { incoming, dismiss } = useDropStore();

  if (incoming.length === 0) return null;

  return (
    <div className="drop-toasts">
      {incoming.slice(0, 5).map((item) => {
        const d = item.data;
        return (
          <div key={item.id} className="drop-toast">
            <button className="drop-toast-close" onClick={() => dismiss(item.id)}>
              ✕
            </button>
            {d.kind === "file" ? (
              <>
                <div className="drop-toast-title">📎 {d.fromName} ti ha inviato un file</div>
                <div className="drop-toast-body">
                  <strong>{d.name}</strong> <span className="dim">{fmtBytes(d.sizeBytes)}</span>
                </div>
                {d.savedPath ? (
                  <>
                    <div className="drop-path" title={d.savedPath}>
                      📁 {d.savedPath}
                    </div>
                    <div className="drop-toast-actions">
                      <button
                        className="drop-toast-action"
                        onClick={() => post(`/api/drop/open/${encodeURIComponent(d.name)}`, {})}
                      >
                        Apri
                      </button>
                      <button
                        className="drop-toast-action ghost"
                        onClick={() => post(`/api/drop/reveal/${encodeURIComponent(d.name)}`, {})}
                      >
                        Mostra nella cartella
                      </button>
                    </div>
                  </>
                ) : (
                  <a
                    className="drop-toast-action"
                    href={`${API_BASE}/api/drop/download/${d.transferId}`}
                    download={d.name}
                    onClick={() => dismiss(item.id)}
                  >
                    Scarica
                  </a>
                )}
              </>
            ) : (
              <>
                <div className="drop-toast-title">💬 Testo da {d.fromName}</div>
                <div className="drop-toast-body drop-text">{d.text}</div>
                <button
                  className="drop-toast-action"
                  onClick={() => {
                    navigator.clipboard.writeText(d.text);
                    dismiss(item.id);
                  }}
                >
                  Copia
                </button>
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
