import { useState } from "react";
import { post } from "../../lib/api";
import type { ApiError, KillOutcome, PortProcess } from "../../lib/types";

interface KillDialogProps {
  process: PortProcess;
  onClose: (killed: boolean) => void;
}

export function KillDialog({ process, onClose }: KillDialogProps) {
  const needsTyped = process.killProtection === "typed-confirm";
  const [typed, setTyped] = useState("");
  const [force, setForce] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);

  const confirm = async () => {
    setBusy(true);
    setError(null);
    const r = await post<KillOutcome>("/api/processes/kill", {
      pid: process.pid,
      expectedName: process.name,
      expectedStartedAt: process.startedAt,
      force,
      confirmName: needsTyped ? typed.trim() : undefined,
    });
    setBusy(false);
    if (r.ok) onClose(true);
    else setError(r.error);
  };

  const typedOk = !needsTyped || typed.trim().toLowerCase() === process.name.toLowerCase();

  return (
    <div className="overlay" onClick={() => onClose(false)}>
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <h3>Termina processo</h3>
        <p>
          <strong>{process.name}</strong> (PID {process.pid}
          {process.user ? `, utente ${process.user}` : ""})
        </p>
        {needsTyped && (
          <>
            <p className="hint">
              Processo protetto: digita <code>{process.name}</code> per confermare.
            </p>
            <input
              value={typed}
              onChange={(e) => setTyped(e.target.value)}
              placeholder={process.name}
              autoFocus
            />
          </>
        )}
        <label className="checkbox">
          <input
            type="checkbox"
            checked={force}
            onChange={(e) => setForce(e.target.checked)}
          />
          Force kill (immediato, senza chiusura pulita)
        </label>
        {error && (
          <div className="banner banner-error">
            {error.message}
            {error.osHint && <div className="hint">{error.osHint}</div>}
          </div>
        )}
        <div className="dialog-actions">
          <button onClick={() => onClose(false)} disabled={busy}>
            Annulla
          </button>
          <button
            className="danger"
            onClick={confirm}
            disabled={busy || !typedOk}
          >
            {busy ? "Termino…" : force ? "Force kill" : "Termina"}
          </button>
        </div>
      </div>
    </div>
  );
}
