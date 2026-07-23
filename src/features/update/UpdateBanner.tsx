import { useEffect, useState } from "react";
import type { Update, DownloadEvent } from "@tauri-apps/plugin-updater";

type Phase = "idle" | "available" | "downloading" | "error";

// Auto-updater: endpoint in tauri.conf.json. Attivo SOLO nella finestra desktop.
export function UpdateBanner() {
  const [phase, setPhase] = useState<Phase>("idle");
  const [update, setUpdate] = useState<Update | null>(null);
  const [progress, setProgress] = useState(0);
  const [errMsg, setErrMsg] = useState<string | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    if (!("__TAURI_INTERNALS__" in window)) return; // solo finestra desktop
    let cancelled = false;
    (async () => {
      try {
        const { check } = await import("@tauri-apps/plugin-updater");
        const found = await check();
        if (!cancelled && found) {
          setUpdate(found);
          setPhase("available");
        }
      } catch (e) {
        console.warn("Controllo aggiornamenti fallito:", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const install = async () => {
    if (!update) return;
    setPhase("downloading");
    setErrMsg(null);
    try {
      let total = 0;
      let done = 0;
      await update.downloadAndInstall((ev: DownloadEvent) => {
        switch (ev.event) {
          case "Started":
            total = ev.data.contentLength ?? 0;
            break;
          case "Progress":
            done += ev.data.chunkLength;
            if (total > 0) setProgress(Math.min(100, Math.round((done / total) * 100)));
            break;
          case "Finished":
            setProgress(100);
            break;
        }
      });
      // Riavvia sulla nuova versione.
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    } catch (e) {
      setErrMsg(e instanceof Error ? e.message : String(e));
      setPhase("error");
    }
  };

  if (phase === "idle" || dismissed || !update) return null;

  return (
    <div className="update-banner" role="alert">
      <div className="update-banner-body">
        {phase === "available" && (
          <>
            <div className="update-banner-title">
              Aggiornamento disponibile: <strong>v{update.version}</strong>
            </div>
            {update.body && <div className="update-banner-notes">{update.body}</div>}
          </>
        )}
        {phase === "downloading" && (
          <div className="update-banner-title">
            Download in corso… {progress}%
            <div className="update-progress">
              <div className="update-progress-fill" style={{ width: `${progress}%` }} />
            </div>
          </div>
        )}
        {phase === "error" && (
          <div className="update-banner-title">
            Aggiornamento fallito: <span className="dim">{errMsg}</span>
          </div>
        )}
      </div>
      <div className="update-banner-actions">
        {phase === "available" && (
          <>
            <button className="primary small" onClick={install}>
              Installa e riavvia
            </button>
            <button className="small" onClick={() => setDismissed(true)}>
              Più tardi
            </button>
          </>
        )}
        {phase === "error" && (
          <>
            <button className="small" onClick={install}>
              Riprova
            </button>
            <button className="small" onClick={() => setDismissed(true)}>
              Chiudi
            </button>
          </>
        )}
      </div>
    </div>
  );
}
