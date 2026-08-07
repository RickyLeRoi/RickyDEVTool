import { useState } from "react";
import { Trans, useTranslation } from "react-i18next";
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
  const { t } = useTranslation();
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
      title={t("projects.deleteBranch.title", { name: branch.name })}
      onCancel={() => onClose(null)}
      error={needForce ? null : error}
      busy={busy}
      confirm={{
        label: busy
          ? t("projects.deleteBranch.deleting")
          : needForce
            ? t("projects.deleteBranch.forceDelete")
            : alsoRemote
              ? t("projects.deleteBranch.deleteLocalRemote")
              : t("common.delete"),
        onClick: () => submit(needForce),
        danger: true,
        disabled: blocked,
      }}
    >
      {hasRemote ? (
        <>
          <p className="dim">{t("projects.deleteBranch.alsoOnRemote", { ref: branch.remoteRef })}</p>
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
            <Trans i18nKey="projects.deleteBranch.onlyLocal" components={{ b: <strong /> }} />
          </label>
          <label className="radio-row">
            <input
              type="radio"
              name="delete-scope"
              checked={alsoRemote}
              onChange={() => setAlsoRemote(true)}
            />
            <Trans
              i18nKey="projects.deleteBranch.alsoRemote"
              values={{ ref: branch.remoteRef }}
              components={{ b: <strong /> }}
            />
          </label>

          {alsoRemote && (
            <div className="banner banner-error delete-remote-confirm">
              {t("projects.deleteBranch.remoteWarn")}
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={confirmRemote}
                  onChange={(e) => setConfirmRemote(e.target.checked)}
                />
                {t("projects.deleteBranch.confirmRemote")}
              </label>
            </div>
          )}
        </>
      ) : (
        <p className="dim">{t("projects.deleteBranch.localWillDelete", { name: branch.name })}</p>
      )}

      {needForce && (
        <div className="banner banner-warn">{t("projects.deleteBranch.notMerged")}</div>
      )}
    </Modal>
  );
}
