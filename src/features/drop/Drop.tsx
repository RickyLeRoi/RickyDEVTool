import { useEffect, useRef, useState } from "react";
import { api, API_BASE, post } from "../../lib/api";
import { fmtBytes } from "../../lib/format";
import { getDeviceName, setDeviceName } from "../../lib/device";
import { useDropStore } from "../../stores/dropStore";
import { useTrayIntentStore } from "../../stores/trayIntentStore";
import type { DropPeer, ReceivedFile } from "../../lib/types";

function PeerCard({ peer, focusSeq }: { peer: DropPeer; focusSeq: number }) {
  const cardRef = useRef<HTMLDivElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [showText, setShowText] = useState(false);

  // "Invia testo..." scelto dal menu del tray per QUESTO dispositivo
  // (focusSeq cambia a ogni click, anche ripetuto sulla stessa voce).
  useEffect(() => {
    if (focusSeq === 0) return;
    setShowText(true);
    cardRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
  }, [focusSeq]);

  const sendFile = async (file: File) => {
    setStatus(`Invio ${file.name}…`);
    const form = new FormData();
    form.append("to", peer.deviceId);
    form.append("fromName", getDeviceName());
    form.append("file", file);
    try {
      const res = await fetch(`${API_BASE}/api/drop/send`, { method: "POST", body: form });
      const json = await res.json();
      setStatus(json.ok ? `Inviato: ${file.name}` : `Errore: ${json.error?.message ?? "invio fallito"}`);
    } catch {
      setStatus("Errore di rete durante l'invio");
    }
    setTimeout(() => setStatus(null), 4000);
  };

  const sendText = async () => {
    if (!text.trim()) return;
    const r = await post("/api/drop/text", {
      to: peer.deviceId,
      fromName: getDeviceName(),
      text,
    });
    setStatus(r.ok ? "Testo inviato" : "Errore invio testo");
    if (r.ok) {
      setText("");
      setShowText(false);
    }
    setTimeout(() => setStatus(null), 4000);
  };

  return (
    <div className="peer-card" ref={cardRef}>
      <div className="peer-head">
        <span className="peer-icon">{peer.remote ? "🌐" : peer.isDesktop ? "🖥" : "📱"}</span>
        <span className="peer-name">{peer.name}</span>
        {peer.remote ? (
          <span className="badge" title="Altro computer scoperto in rete locale">
            altra rete
          </span>
        ) : (
          peer.isDesktop && <span className="badge">questo PC</span>
        )}
      </div>
      <div className="peer-actions">
        <button className="small" onClick={() => fileInput.current?.click()}>
          Invia file
        </button>
        <button className="small" onClick={() => setShowText(!showText)}>
          Invia testo
        </button>
        <input
          ref={fileInput}
          type="file"
          hidden
          onChange={(e) => {
            const f = e.target.files?.[0];
            if (f) sendFile(f);
            e.target.value = "";
          }}
        />
      </div>
      {showText && (
        <div className="peer-text">
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="Testo o link da inviare…"
            rows={2}
          />
          <button className="small" onClick={sendText}>
            Invia
          </button>
        </div>
      )}
      {status && <div className="dim peer-status">{status}</div>}
    </div>
  );
}

function ReceivedPanel() {
  const [files, setFiles] = useState<ReceivedFile[] | null>(null);
  const [folder, setFolder] = useState<string>("");
  const [remote, setRemote] = useState(false);

  const load = async () => {
    const r = await api<{ files: ReceivedFile[]; folder: string }>("/api/drop/received");
    if (r.ok) {
      setFiles(r.data.files);
      setFolder(r.data.folder);
      setRemote(false);
    } else if (r.error.code === "REMOTE_FORBIDDEN") {
      setRemote(true);
    }
  };

  useEffect(() => {
    load();
  }, []);

  // Da telefono la cartella dei ricevuti vive sul desktop: non mostrarla.
  if (remote) return null;

  return (
    <section className="received">
      <div className="section-header">
        <h3>File ricevuti su questo PC</h3>
        <div>
          <button className="small" onClick={() => post("/api/drop/open-folder", {})}>
            Apri cartella
          </button>{" "}
          <button className="small" onClick={load}>
            Aggiorna
          </button>
        </div>
      </div>
      {folder && <div className="dim received-folder">{folder}</div>}
      {files && files.length === 0 && <div className="empty">Nessun file ricevuto.</div>}
      {files && files.length > 0 && (
        <table className="proc-table">
          <tbody>
            {files.map((f) => (
              <tr key={f.name}>
                <td>{f.name}</td>
                <td className="num dim">{fmtBytes(f.sizeBytes)}</td>
                <td className="num received-actions">
                  <button
                    className="small"
                    onClick={() => post(`/api/drop/open/${encodeURIComponent(f.name)}`, {})}
                  >
                    Apri
                  </button>
                  <button
                    className="small"
                    onClick={() => post(`/api/drop/reveal/${encodeURIComponent(f.name)}`, {})}
                  >
                    Mostra
                  </button>
                  <button
                    className="small danger"
                    onClick={async () => {
                      await api(`/api/drop/received/${encodeURIComponent(f.name)}`, {
                        method: "DELETE",
                      });
                      load();
                    }}
                  >
                    Elimina
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

export function Drop() {
  const peers = useDropStore((s) => s.peers);
  const [name, setName] = useState(getDeviceName());
  const traySection = useTrayIntentStore((s) => s.section);
  const trayDeviceId = useTrayIntentStore((s) => s.extra);
  const traySeq = useTrayIntentStore((s) => s.seq);

  const saveName = () => setDeviceName(name);

  return (
    <div className="drop">
      <div className="section-header">
        <h2>Drop</h2>
        <label className="device-name">
          Il mio nome:{" "}
          <input value={name} onChange={(e) => setName(e.target.value)} onBlur={saveName} />
        </label>
      </div>

      <p className="hint">
        Condividi file e testo con gli altri dispositivi collegati (stessa rete, con la UI
        aperta), stile AirDrop. I file inviati a questo PC finiscono in Download/RickyDEVTool.
      </p>

      {peers.length === 0 ? (
        <div className="empty">
          Nessun altro dispositivo online. Apri RickyDEVTool su un altro device (o il telefono
          via QR) per vederlo qui.
        </div>
      ) : (
        <div className="peer-grid">
          {peers.map((p) => (
            <PeerCard
              key={p.deviceId}
              peer={p}
              focusSeq={traySection === "drop" && trayDeviceId === p.deviceId ? traySeq : 0}
            />
          ))}
        </div>
      )}

      <ReceivedPanel />
    </div>
  );
}
