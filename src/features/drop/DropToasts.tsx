import { useState } from "react";
import { API_BASE, post } from "../../lib/api";
import { getDeviceSecret } from "../../lib/device";
import { fmtBytes } from "../../lib/format";
import { useDropStore } from "../../stores/dropStore";

// 20260806 ++ RG #Drop il segreto del device viaggia in un header, non nella URL: un <a href>
// finirebbe nella cronologia e nei log. Quindi scarichiamo via fetch e salviamo il blob.
async function scarica(transferId: string, name: string): Promise<string | null> {
  try {
    const res = await fetch(`${API_BASE}/api/drop/download/${transferId}`, {
      headers: { "X-RickyDev-Device-Secret": getDeviceSecret() },
    });
    if (!res.ok) {
      const body = await res.json().catch(() => null);
      return body?.error?.message ?? `download fallito (HTTP ${res.status})`;
    }
    const url = URL.createObjectURL(await res.blob());
    const a = document.createElement("a");
    a.href = url;
    a.download = name;
    a.click();
    URL.revokeObjectURL(url);
    return null;
  } catch (e) {
    return e instanceof Error ? e.message : "download fallito";
  }
}

export function DropToasts() {
  const { incoming, dismiss } = useDropStore();
  const [errors, setErrors] = useState<Record<string, string>>({});

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
                  <>
                    {errors[item.id] && (
                      <div className="banner banner-error">{errors[item.id]}</div>
                    )}
                    <button
                      className="drop-toast-action"
                      onClick={async () => {
                        const error = await scarica(d.transferId, d.name);
                        if (error) setErrors((e) => ({ ...e, [item.id]: error }));
                        else dismiss(item.id);
                      }}
                    >
                      Scarica
                    </button>
                  </>
                )}
              </>
            ) : (
              <>
                <div className="drop-toast-title">
                  {d.kind === "clipboard"
                    ? `📋 Appunti da ${d.fromName}`
                    : `💬 Testo da ${d.fromName}`}
                </div>
                <div className="drop-toast-body drop-text">{d.text}</div>
                {d.kind === "clipboard" && (
                  <div className="hint">Aggiunto allo storico appunti.</div>
                )}
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
