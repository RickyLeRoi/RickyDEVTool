import { useEffect } from "react";
import { useUpdateStore } from "../../stores/updateStore";

export function UpdateBanner() {
  const phase = useUpdateStore((s) => s.phase);
  const progress = useUpdateStore((s) => s.progress);
  const error = useUpdateStore((s) => s.error);
  const dismissed = useUpdateStore((s) => s.dismissed);
  const check = useUpdateStore((s) => s.check);
  const install = useUpdateStore((s) => s.install);
  const dismiss = useUpdateStore((s) => s.dismiss);

  useEffect(() => {
    check();
  }, [check]);

  const visible = !dismissed && (phase === "available" || phase === "downloading" || phase === "error");
  if (!visible) return null;

  return (
    <div className="update-banner" role="alert">
      <div className="update-banner-body">
        {phase === "available" && (
          <div className="update-banner-title">È disponibile un aggiornamento</div>
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
            Aggiornamento fallito: <span className="update-banner-dim">{error}</span>
          </div>
        )}
      </div>
      <div className="update-banner-actions">
        {phase === "available" && (
          <>
            <button className="primary small" onClick={install}>
              Installa e riavvia
            </button>
            <button className="small ghost-on-accent" onClick={dismiss}>
              Più tardi
            </button>
          </>
        )}
        {phase === "error" && (
          <>
            <button className="small" onClick={install}>
              Riprova
            </button>
            <button className="small ghost-on-accent" onClick={dismiss}>
              Chiudi
            </button>
          </>
        )}
      </div>
    </div>
  );
}
