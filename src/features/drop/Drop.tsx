import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, API_BASE, post } from "../../lib/api";
import { fmtBytes } from "../../lib/format";
import { getDeviceName, setDeviceName } from "../../lib/device";
import { TOAST_MS } from "../../lib/constants";
import { useDropStore } from "../../stores/dropStore";
import { useTrayIntentStore } from "../../stores/trayIntentStore";
import type { DropPeer, ReceivedFile } from "../../lib/types";

function PeerCard({ peer, focusSeq }: { peer: DropPeer; focusSeq: number }) {
  const { t } = useTranslation();
  const cardRef = useRef<HTMLDivElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [showText, setShowText] = useState(false);

  useEffect(() => {
    if (focusSeq === 0) return;
    setShowText(true);
    cardRef.current?.scrollIntoView({ behavior: "smooth", block: "center" });
  }, [focusSeq]);

  const sendFile = async (file: File) => {
    setStatus(t("drop.sendingFile", { name: file.name }));
    const form = new FormData();
    form.append("to", peer.deviceId);
    form.append("fromName", getDeviceName());
    form.append("file", file);
    try {
      const res = await fetch(`${API_BASE}/api/drop/send`, { method: "POST", body: form });
      const json = await res.json();
      setStatus(
        json.ok
          ? t("drop.sentFile", { name: file.name })
          : t("drop.sendError", { message: json.error?.message ?? t("drop.sendFailed") }),
      );
    } catch {
      setStatus(t("drop.networkError"));
    }
    setTimeout(() => setStatus(null), TOAST_MS);
  };

  const sendText = async () => {
    if (!text.trim()) return;
    const r = await post("/api/drop/text", {
      to: peer.deviceId,
      fromName: getDeviceName(),
      text,
    });
    setStatus(r.ok ? t("drop.textSent") : t("drop.textError"));
    if (r.ok) {
      setText("");
      setShowText(false);
    }
    setTimeout(() => setStatus(null), TOAST_MS);
  };

  return (
    <div className="peer-card" ref={cardRef}>
      <div className="peer-head">
        <span className="peer-icon">{peer.remote ? "🌐" : peer.isDesktop ? "🖥" : "📱"}</span>
        <span className="peer-name">{peer.name}</span>
        {peer.remote ? (
          <span className="badge" title={t("drop.otherNetworkTitle")}>
            {t("drop.otherNetwork")}
          </span>
        ) : (
          peer.isDesktop && <span className="badge">{t("drop.thisPc")}</span>
        )}
      </div>
      <div className="peer-actions">
        <button className="small" onClick={() => fileInput.current?.click()}>
          {t("drop.sendFile")}
        </button>
        <button className="small" onClick={() => setShowText(!showText)}>
          {t("drop.sendText")}
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
            placeholder={t("drop.textPlaceholder")}
            rows={2}
          />
          <button className="small" onClick={sendText}>
            {t("drop.send")}
          </button>
        </div>
      )}
      {status && <div className="dim peer-status">{status}</div>}
    </div>
  );
}

function ReceivedPanel() {
  const { t } = useTranslation();
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

  if (remote) return null;

  return (
    <section className="received">
      <div className="section-header">
        <h3>{t("drop.receivedTitle")}</h3>
        <div>
          <button className="small" onClick={() => post("/api/drop/open-folder", {})}>
            {t("drop.openFolder")}
          </button>{" "}
          <button className="small" onClick={load}>
            {t("common.refresh")}
          </button>
        </div>
      </div>
      {folder && <div className="dim received-folder">{folder}</div>}
      {files && files.length === 0 && <div className="empty">{t("drop.noReceived")}</div>}
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
                    {t("common.open")}
                  </button>
                  <button
                    className="small"
                    onClick={() => post(`/api/drop/reveal/${encodeURIComponent(f.name)}`, {})}
                  >
                    {t("drop.reveal")}
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
                    {t("common.delete")}
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
  const { t } = useTranslation();
  const peers = useDropStore((s) => s.peers);
  const [name, setName] = useState(getDeviceName());
  const traySection = useTrayIntentStore((s) => s.section);
  const trayDeviceId = useTrayIntentStore((s) => s.extra);
  const traySeq = useTrayIntentStore((s) => s.seq);

  const saveName = () => setDeviceName(name);

  return (
    <div className="drop">
      <div className="section-header">
        <h2>{t("nav.drop")}</h2>
        <label className="device-name">
          {t("drop.myName")}{" "}
          <input value={name} onChange={(e) => setName(e.target.value)} onBlur={saveName} />
        </label>
      </div>

      <p className="hint">{t("drop.intro")}</p>

      {peers.length === 0 ? (
        <div className="empty">{t("drop.noPeers")}</div>
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
