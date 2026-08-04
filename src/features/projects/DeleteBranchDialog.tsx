import { useState } from "react";
import { Modal } from "../../components/Modal";
import { post } from "../../lib/api";
import { useSubmit } from "../../lib/useSubmit";
import type { GitBranch } from "../../lib/types";

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
  const { busy, error, run } = useSubmit();

  const blocked = alsoRemote && !confirmRemote;

  const submit = async (force: boolean) => {
    const r = await run(
      () =>
        post<{ branches: GitBranch[] }>("/api/git/delete-branch", {
          path,
          branch: branch.name,
          force,
          remote: alsoRemote && branch.remoteRef ? remoteName(branch.remoteRef) : null,
        }),
      (data) => onClose(data.branches),
    );
    if (!r.ok && !force && /not fully merged|non.*mergiat/i.test(r.error.message)) {
      setNeedForce(true);
    }
  };

  return (
    <Modal
      title={`Elimina branch «${branch.name}»`}
      onCancel={() => onClose(null)}
      error={needForce ? null : error}
      busy={busy}
      confirm={{
        label: busy
          ? "Elimino…"
          : needForce
            ? "Forza eliminazione"
            : alsoRemote
              ? "Elimina locale e remoto"
              : "Elimina",
        onClick: () => submit(needForce),
        danger: true,
        disabled: blocked,
      }}
    >
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
    </Modal>
  );
}
