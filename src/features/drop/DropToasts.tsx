import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { API_BASE, post } from "../../lib/api";
import { getDeviceSecret } from "../../lib/device";
import { fmtBytes } from "../../lib/format";
import { DEVICE_SECRET_HEADER, DROP_MAX_TOASTS } from "../../lib/constants";
import { useDropStore } from "../../stores/dropStore";

// 20260806 ++ RG #Security il segreto del device viaggia in un header, non nella URL: un <a href>
// finirebbe nella cronologia e nei log. Quindi fetch + blob.
async function scarica(transferId: string, name: string, t: TFunction): Promise<string | null> {
  try {
    const res = await fetch(`${API_BASE}/api/drop/download/${transferId}`, {
      headers: { [DEVICE_SECRET_HEADER]: getDeviceSecret() },
    });
    if (!res.ok) {
      const body = await res.json().catch(() => null);
      return body?.error?.message ?? t("drop.downloadFailedHttp", { status: res.status });
    }
    const url = URL.createObjectURL(await res.blob());
    const a = document.createElement("a");
    a.href = url;
    a.download = name;
    a.click();
    URL.revokeObjectURL(url);
    return null;
  } catch (e) {
    return e instanceof Error ? e.message : t("drop.downloadFailed");
  }
}

export function DropToasts() {
  const { t } = useTranslation();
  const { incoming, dismiss } = useDropStore();
  const [errors, setErrors] = useState<Record<string, string>>({});

  if (incoming.length === 0) return null;

  return (
    <div className="drop-toasts">
      {incoming.slice(0, DROP_MAX_TOASTS).map((item) => {
        const d = item.data;
        return (
          <div key={item.id} className="drop-toast">
            <button className="drop-toast-close" onClick={() => dismiss(item.id)}>
              ✕
            </button>
            {d.kind === "file" ? (
              <>
                <div className="drop-toast-title">{t("drop.toastFile", { from: d.fromName })}</div>
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
                        {t("common.open")}
                      </button>
                      <button
                        className="drop-toast-action ghost"
                        onClick={() => post(`/api/drop/reveal/${encodeURIComponent(d.name)}`, {})}
                      >
                        {t("drop.openInFolder")}
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
                        const error = await scarica(d.transferId, d.name, t);
                        if (error) setErrors((e) => ({ ...e, [item.id]: error }));
                        else dismiss(item.id);
                      }}
                    >
                      {t("drop.download")}
                    </button>
                  </>
                )}
              </>
            ) : (
              <>
                <div className="drop-toast-title">
                  {d.kind === "clipboard"
                    ? t("drop.toastClipboard", { from: d.fromName })
                    : t("drop.toastText", { from: d.fromName })}
                </div>
                <div className="drop-toast-body drop-text">{d.text}</div>
                {d.kind === "clipboard" && (
                  <div className="hint">{t("drop.addedToClipboard")}</div>
                )}
                <button
                  className="drop-toast-action"
                  onClick={() => {
                    navigator.clipboard.writeText(d.text);
                    dismiss(item.id);
                  }}
                >
                  {t("common.copy")}
                </button>
              </>
            )}
          </div>
        );
      })}
    </div>
  );
}
