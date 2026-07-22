import { useState } from "react";
import { post } from "../../lib/api";
import type { ApiError, GitBranch } from "../../lib/types";

// Nome del remote dal ref completo: "origin/feature/x" → "origin".
function remoteName(remoteRef: string): string {
  return remoteRef.split("/")[0];
}

export function DeleteBranchDialog({
  branch,
  path,
  onClose,
}: {
  branch: GitBranch;
  path: string;
  onClose: (branches: GitBranch[] | null) => void;
}) {
  const hasRemote = !!branch.remoteRef;
  const [alsoRemote, setAlsoRemote] = useState(false);
  const [confirmRemote, setConfirmRemote] = useState(false);
  const [needForce, setNeedForce] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);

  // Se si elimina anche dal remoto serve la conferma extra esplicita.
  const blocked = alsoRemote && !confirmRemote;

  const submit = async (force: boolean) => {
    setBusy(true);
    setError(null);
    const r = await post<{ branches: GitBranch[] }>("/api/git/delete-branch", {
      path,
      branch: branch.name,
      force,
      remote: alsoRemote && branch.remoteRef ? remoteName(branch.remoteRef) : null,
    });
    setBusy(false);
    if (r.ok) {
      onClose(r.data.branches);
      return;
    }
    // git rifiuta i branch non uniti: mostra l'opzione di forzatura.
    if (!force && /not fully merged|non.*mergiat/i.test(r.error.message)) {
      setNeedForce(true);
    }
    setError(r.error);
  };

  return (
    <div className="overlay" onClick={() => onClose(null)}>
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <h3>Elimina branch «{branch.name}»</h3>

        {hasRemote ? (
          <>
            <p className="dim">Questo branch esiste anche sul remoto ({branch.remoteRef}).</p>
            <label className="radio-row">
              <input
                type="radio"
                name="delete-scope"
                checked={!alsoRemote}
                onChange={() => {
                  setAlsoRemote(false);
                  setConfirmRemote(false);
                }}
              />
              Elimina solo il branch <strong>locale</strong>
            </label>
            <label className="radio-row">
              <input
                type="radio"
                name="delete-scope"
                checked={alsoRemote}
                onChange={() => setAlsoRemote(true)}
              />
              Elimina <strong>anche dal remoto</strong> ({branch.remoteRef})
            </label>

            {alsoRemote && (
              <div className="banner banner-error delete-remote-confirm">
                ⚠ L'eliminazione dal remoto è irreversibile e visibile a tutti.
                <label className="checkbox">
                  <input
                    type="checkbox"
                    checked={confirmRemote}
                    onChange={(e) => setConfirmRemote(e.target.checked)}
                  />
                  Confermo di voler eliminare anche il branch remoto
                </label>
              </div>
            )}
          </>
        ) : (
          <p className="dim">Il branch locale «{branch.name}» verrà eliminato.</p>
        )}

        {needForce && (
          <div className="banner banner-warn">
            Il branch non è stato unito: forzando l'eliminazione perderai i commit non mergiati.
          </div>
        )}

        {error && !needForce && (
          <div className="banner banner-error">
            {error.message}
            {error.osHint && <div className="hint">{error.osHint}</div>}
          </div>
        )}

        <div className="dialog-actions">
          <button onClick={() => onClose(null)} disabled={busy}>
            Annulla
          </button>
          {needForce ? (
            <button className="danger" onClick={() => submit(true)} disabled={busy || blocked}>
              {busy ? "Elimino…" : "Forza eliminazione"}
            </button>
          ) : (
            <button className="danger" onClick={() => submit(false)} disabled={busy || blocked}>
              {busy ? "Elimino…" : alsoRemote ? "Elimina locale e remoto" : "Elimina"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
