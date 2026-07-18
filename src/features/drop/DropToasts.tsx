import { API_BASE } from "../../lib/api";
import { fmtBytes } from "../../lib/format";
import { useDropStore } from "../../stores/dropStore";

/** Notifiche fluttuanti per file/testo in arrivo, visibili da qualsiasi sezione. */
export function DropToasts() {
  const { incoming, dismiss } = useDropStore();

  if (incoming.length === 0) return null;

  return (
    <div className="drop-toasts">
      {incoming.slice(0, 5).map((item) => (
        <div key={item.id} className="drop-toast">
          <button className="drop-toast-close" onClick={() => dismiss(item.id)}>
            ✕
          </button>
          {item.data.kind === "file" ? (
            <>
              <div className="drop-toast-title">
                📎 {item.data.fromName} ti ha inviato un file
              </div>
              <div className="drop-toast-body">
                <strong>{item.data.name}</strong>{" "}
                <span className="dim">{fmtBytes(item.data.sizeBytes)}</span>
              </div>
              {item.data.savedPath ? (
                <div className="dim">Salvato in {item.data.savedPath}</div>
              ) : (
                <a
                  className="drop-toast-action"
                  href={`${API_BASE}/api/drop/download/${item.data.transferId}`}
                  download={item.data.name}
                  onClick={() => dismiss(item.id)}
                >
                  Scarica
                </a>
              )}
            </>
          ) : (
            <>
              <div className="drop-toast-title">💬 Testo da {item.data.fromName}</div>
              <div className="drop-toast-body drop-text">{item.data.text}</div>
              <button
                className="drop-toast-action"
                onClick={() => {
                  navigator.clipboard.writeText(item.data.kind === "text" ? item.data.text : "");
                  dismiss(item.id);
                }}
              >
                Copia
              </button>
            </>
          )}
        </div>
      ))}
    </div>
  );
}
