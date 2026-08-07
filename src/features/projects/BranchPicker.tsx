import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { getLang } from "../../lib/i18n";
import { DeleteBranchDialog } from "./DeleteBranchDialog";
import type { ApiError, GitBranch, GitRepoInfo } from "../../lib/types";

interface BranchPickerProps {
  path: string;
  dirty: boolean;
  onCheckout: (info: GitRepoInfo) => void;
  selectedRef: string | null;
  onSelectBranch: (name: string | null) => void;
}

function fmtDate(ms: number) {
  return new Date(ms).toLocaleDateString(getLang(), {
    day: "2-digit",
    month: "short",
    year: "2-digit",
  });
}

export function BranchPicker({
  path,
  dirty,
  onCheckout,
  selectedRef,
  onSelectBranch,
}: BranchPickerProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [branches, setBranches] = useState<GitBranch[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [deleting, setDeleting] = useState<GitBranch | null>(null);

  useEffect(() => {
    setBranches(null);
    setOpen(false);
  }, [path]);

  const load = async () => {
    const r = await api<{ branches: GitBranch[] }>(
      `/api/git/branches?path=${encodeURIComponent(path)}`,
    );
    if (r.ok) setBranches(r.data.branches);
    else setError(r.error);
  };

  const toggle = () => {
    if (!open && !branches) load();
    setOpen(!open);
  };

  const checkout = async (branch: GitBranch) => {
    setBusy(branch.name);
    setError(null);
    const r = await post<GitRepoInfo>("/api/git/checkout", { path, branch: branch.name });
    setBusy(null);
    if (r.ok) {
      setOpen(false);
      setBranches(null);
      onCheckout(r.data);
    } else setError(r.error);
  };

  const current = branches?.find((b) => b.isCurrent)?.name;

  return (
    <div className="branch-picker">
      <button onClick={toggle}>
        {open ? "▾" : "▸"} {t("projects.branches.branch")}
        {current ? `: ${current}` : ""}
      </button>
      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}
      {open && (
        <div className="branch-list">
          {!branches && <div className="empty">{t("projects.branches.loading")}</div>}
          {branches?.map((b) => (
            <div key={b.name} className="branch-row">
              <div className="git-row-actions">
                <button
                  className="git-act"
                  disabled={b.isCurrent || dirty || busy !== null}
                  title={
                    b.isCurrent
                      ? t("projects.branches.current")
                      : dirty
                        ? t("projects.branches.dirtyTree")
                        : t("projects.branches.checkoutBranch")
                  }
                  onClick={() => checkout(b)}
                >
                  {busy === b.name ? "…" : "⤓"}
                </button>
                {!b.isRemoteOnly && (
                  <button
                    className="git-act danger"
                    disabled={b.isCurrent || busy !== null}
                    title={
                      b.isCurrent
                        ? t("projects.branches.cantDeleteCurrent")
                        : t("projects.branches.deleteBranch")
                    }
                    onClick={() => setDeleting(b)}
                  >
                    🗑
                  </button>
                )}
              </div>
              <button
                className={`branch-main branch-select ${selectedRef === b.name ? "active" : ""}`}
                title={t("projects.branches.showCommits")}
                onClick={() => onSelectBranch(b.name)}
              >
                <span
                  className={`branch-name ${b.staleWeeks >= 4 ? "stale" : ""}`}
                  title={
                    b.staleWeeks >= 4
                      ? t("projects.branches.lastRemoteCommit", { weeks: b.staleWeeks })
                      : undefined
                  }
                >
                  {b.isCurrent && "● "}
                  {b.name}
                  {b.isRemoteOnly && <span className="badge"> {t("projects.branches.remote")}</span>}
                </span>
                <span className="branch-sub">
                  {b.lastCommit.shortHash} · {fmtDate(b.lastCommit.date)} ·{" "}
                  {b.lastCommit.authorName} — {b.lastCommit.subject}
                </span>
              </button>
            </div>
          ))}
          {branches?.length === 0 && <div className="empty">{t("projects.branches.none")}</div>}
        </div>
      )}

      {deleting && (
        <DeleteBranchDialog
          branch={deleting}
          path={path}
          onClose={(updated) => {
            setDeleting(null);
            if (updated) {
              setBranches(updated);
              if (selectedRef === deleting.name) onSelectBranch(null);
            }
          }}
        />
      )}
    </div>
  );
}
