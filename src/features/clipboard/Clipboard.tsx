import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, post, API_BASE } from "../../lib/api";
import { getDeviceName } from "../../lib/device";
import { getLang } from "../../lib/i18n";
import { useDropStore } from "../../stores/dropStore";
import type { ClipboardHistory, ClipEntry } from "../../lib/types";

const REFRESH_MS = 2000;
const PREVIEW_CHARS = 280;

function fmtTime(ms: number) {
  return new Date(ms).toLocaleTimeString(getLang(), {
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

function blobUrl(id: number, index?: number) {
  const q = index != null ? `&i=${index}` : "";
  return `${API_BASE}/api/clipboard/blob?id=${id}${q}`;
}

function TextBody({ entry }: { entry: ClipEntry }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const long = entry.text.length > PREVIEW_CHARS;
  const shown = expanded || !long ? entry.text : entry.text.slice(0, PREVIEW_CHARS) + "…";
  return (
    <>
      <pre className="clip-text" onClick={() => long && setExpanded(!expanded)}>
        {shown}
      </pre>
      {long && (
        <button className="small ghost clip-expand" onClick={() => setExpanded(!expanded)}>
          {expanded ? t("tool.clipboard.collapse") : t("tool.clipboard.showAll", { count: entry.text.length })}
        </button>
      )}
    </>
  );
}

function ImageBody({ entry }: { entry: ClipEntry }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  return (
    <div className="clip-media">
      <img
        className={`clip-image ${expanded ? "expanded" : ""}`}
        src={blobUrl(entry.id)}
        alt={entry.text}
        title={expanded ? t("tool.clipboard.reduce") : t("tool.clipboard.enlarge")}
        onClick={() => setExpanded(!expanded)}
      />
    </div>
  );
}

function FilesBody({ entry }: { entry: ClipEntry }) {
  const { t } = useTranslation();
  const files = entry.files ?? [];
  return (
    <ul className="clip-files">
      {files.map((f, i) => (
        <li key={i} className="clip-file">
          <span className="clip-file-ico" aria-hidden>
            📄
          </span>
          <span className="clip-file-name" title={f.name}>
            {f.name}
          </span>
          <span className="dim clip-file-size">{fmtBytes(f.size)}</span>
          {f.hasBlob ? (
            <a className="small ghost" href={blobUrl(entry.id, i)} download={f.name}>
              {t("common.save")}
            </a>
          ) : (
            <span className="badge" title={t("tool.clipboard.onlyNameTitle")}>
              {t("tool.clipboard.onlyName")}
            </span>
          )}
        </li>
      ))}
    </ul>
  );
}

function Entry({
  entry,
  onChanged,
}: {
  entry: ClipEntry;
  onChanged: (h: ClipboardHistory) => void;
}) {
  const { t } = useTranslation();
  const [copied, setCopied] = useState(false);
  const [sentTo, setSentTo] = useState<string | null>(null);
  const peers = useDropStore((s) => s.peers);

  const copy = async () => {
    const r = await post<{ copied: boolean }>("/api/clipboard/copy", { id: entry.id });
    if (r.ok) {
      setCopied(true);
      setTimeout(() => setCopied(false), 1200);
    }
  };

  const send = async (to: string, name: string) => {
    const r = await post<{ sent: boolean }>("/api/clipboard/send", {
      to,
      fromName: getDeviceName(),
      id: entry.id,
    });
    if (r.ok) {
      setSentTo(name);
      setTimeout(() => setSentTo(null), 1500);
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
        {entry.kind !== "text" && (
          <span className="badge clip-kind">
            {entry.kind === "image" ? t("tool.clipboard.image") : t("tool.clipboard.file")}
          </span>
        )}
        <span className="dim">{fmtBytes(entry.bytes)}</span>
        <span className="clip-actions">
          <button className="small" onClick={copy} title={t("tool.clipboard.copyTitle")}>
            {copied ? t("tool.clipboard.copied") : t("tool.clipboard.copy")}
          </button>
          {entry.kind === "text" &&
            peers.length > 0 &&
            (sentTo ? (
              <span className="badge badge-ok">{t("tool.clipboard.sentTo", { name: sentTo })}</span>
            ) : (
              <select
                className="clip-send small"
                defaultValue=""
                title={t("tool.clipboard.sendTitle")}
                onChange={(e) => {
                  const p = peers.find((x) => x.deviceId === e.target.value);
                  if (p) send(p.deviceId, p.name);
                  e.target.value = "";
                }}
              >
                <option value="" disabled>
                  {t("tool.clipboard.sendTo")}
                </option>
                {peers.map((p) => (
                  <option key={p.deviceId} value={p.deviceId}>
                    {p.name}
                  </option>
                ))}
              </select>
            ))}
          <button
            className={`small ${entry.pinned ? "" : "ghost"}`}
            onClick={pin}
            title={entry.pinned ? t("tool.clipboard.unpin") : t("tool.clipboard.pin")}
          >
            {entry.pinned ? "📌" : "📍"}
          </button>
          <button className="small ghost" onClick={del} title={t("tool.clipboard.deleteTitle")}>
            ✕
          </button>
        </span>
      </div>
      {entry.kind === "text" && <TextBody entry={entry} />}
      {entry.kind === "image" && <ImageBody entry={entry} />}
      {entry.kind === "files" && <FilesBody entry={entry} />}
    </li>
  );
}

export function Clipboard() {
  const { t } = useTranslation();
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
        <h2>{t("tool.clipboard.title")}</h2>
        {hist && (
          <div className="clip-toolbar">
            <button
              className={hist.enabled ? "small" : "small danger"}
              onClick={toggleEnabled}
              title={hist.enabled ? t("tool.clipboard.pauseTitle") : t("tool.clipboard.resumeTitle")}
            >
              {hist.enabled ? t("tool.clipboard.paused") : t("tool.clipboard.resume")}
            </button>
            <button className="small" onClick={() => clear(true)} title={t("tool.clipboard.clearKeepTitle")}>
              {t("tool.clipboard.clear")}
            </button>
            <button className="small danger" onClick={() => clear(false)} title={t("tool.clipboard.clearAllTitle")}>
              {t("tool.clipboard.clearAll")}
            </button>
          </div>
        )}
      </div>

      <p className="hint">
        {t("tool.clipboard.intro")}
        {hist && !hist.enabled && t("tool.clipboard.capturePaused")}
      </p>

      {hist && !hist.supported && (
        <div className="banner banner-error">{t("tool.clipboard.notSupported")}</div>
      )}

      {hist && hist.supported && hist.entries.length === 0 && (
        <div className="empty">{t("tool.clipboard.empty")}</div>
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
