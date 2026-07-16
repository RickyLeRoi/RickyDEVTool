import { useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import type { ApiError, GitBranch, GitRepoInfo } from "../../lib/types";

interface BranchPickerProps {
  path: string;
  dirty: boolean;
  onCheckout: (info: GitRepoInfo) => void;
}

function fmtDate(ms: number) {
  return new Date(ms).toLocaleDateString("it-IT", {
    day: "2-digit",
    month: "short",
    year: "2-digit",
  });
}

export function BranchPicker({ path, dirty, onCheckout }: BranchPickerProps) {
  const [open, setOpen] = useState(false);
  const [branches, setBranches] = useState<GitBranch[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<ApiError | null>(null);

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
        {open ? "▾" : "▸"} Branch{current ? `: ${current}` : ""}
      </button>
      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}
      {open && (
        <div className="branch-list">
          {!branches && <div className="empty">Carico i branch…</div>}
          {branches?.map((b) => (
            <div key={b.name} className="branch-row">
              <div className="branch-main">
                <span
                  className={`branch-name ${b.staleWeeks >= 4 ? "stale" : ""}`}
                  title={
                    b.staleWeeks >= 4
                      ? `ultima commit ${b.staleWeeks} settimane fa`
                      : undefined
                  }
                >
                  {b.isCurrent && "● "}
                  {b.name}
                  {b.isRemoteOnly && <span className="badge"> remote</span>}
                </span>
                <span className="branch-sub">
                  {b.lastCommit.shortHash} · {fmtDate(b.lastCommit.date)} ·{" "}
                  {b.lastCommit.authorName} — {b.lastCommit.subject}
                </span>
              </div>
              {!b.isCurrent && (
                <button
                  className="small"
                  disabled={dirty || busy !== null}
                  title={dirty ? "working tree non pulito" : undefined}
                  onClick={() => checkout(b)}
                >
                  {busy === b.name ? "…" : "Checkout"}
                </button>
              )}
            </div>
          ))}
          {branches?.length === 0 && <div className="empty">Nessun branch.</div>}
        </div>
      )}
    </div>
  );
}
