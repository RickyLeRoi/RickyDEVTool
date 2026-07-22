import { useState } from "react";
import { api, post } from "../../lib/api";
import type { ApiError, GitCommit, GitRepoInfo } from "../../lib/types";

interface CommitListProps {
  path: string;
  dirty: boolean;
  onCheckout: (info: GitRepoInfo) => void;
}

const PAGE = 50;

function fmtDate(ms: number) {
  return new Date(ms).toLocaleDateString("it-IT", {
    day: "2-digit",
    month: "short",
    year: "2-digit",
  });
}

export function CommitList({ path, dirty, onCheckout }: CommitListProps) {
  const [open, setOpen] = useState(false);
  const [commits, setCommits] = useState<GitCommit[] | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [atEnd, setAtEnd] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<ApiError | null>(null);

  const fetchPage = async (skip: number): Promise<GitCommit[]> => {
    const r = await api<{ commits: GitCommit[] }>(
      `/api/git/commits?path=${encodeURIComponent(path)}&limit=${PAGE}&skip=${skip}`,
    );
    if (r.ok) {
      setError(null);
      return r.data.commits;
    }
    setError(r.error);
    return [];
  };

  const toggle = async () => {
    if (!open && !commits) {
      const first = await fetchPage(0);
      setCommits(first);
      setAtEnd(first.length < PAGE);
    }
    setOpen(!open);
  };

  const loadMore = async () => {
    if (!commits) return;
    setLoadingMore(true);
    const next = await fetchPage(commits.length);
    setLoadingMore(false);
    setCommits([...commits, ...next]);
    if (next.length < PAGE) setAtEnd(true);
  };

  const checkout = async (c: GitCommit) => {
    setBusy(c.hash);
    setError(null);
    const r = await post<GitRepoInfo>("/api/git/checkout-commit", { path, hash: c.hash });
    setBusy(null);
    if (r.ok) {
      setOpen(false);
      setCommits(null);
      setAtEnd(false);
      onCheckout(r.data);
    } else setError(r.error);
  };

  return (
    <div className="commit-list-wrap">
      <button onClick={toggle}>{open ? "▾" : "▸"} Cronologia commit</button>
      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}
      {open && (
        <div className="commit-list">
          {!commits && <div className="empty">Carico i commit…</div>}
          {commits?.map((c) => (
            <div key={c.hash} className="commit-row">
              <div className="commit-main">
                <span className="commit-subject" title={c.subject}>
                  {c.subject}
                </span>
                <span className="commit-sub">
                  <code>{c.shortHash}</code> · {fmtDate(c.date)} · {c.authorName}
                  {c.refs.map((r) => (
                    <span key={r} className="badge badge-branch commit-ref">
                      {r}
                    </span>
                  ))}
                </span>
              </div>
              <button
                className="small"
                disabled={dirty || busy !== null}
                title={
                  dirty
                    ? "working tree non pulito"
                    : "Checkout in detached HEAD su questo commit"
                }
                onClick={() => checkout(c)}
              >
                {busy === c.hash ? "…" : "Checkout"}
              </button>
            </div>
          ))}
          {commits && commits.length === 0 && <div className="empty">Nessun commit.</div>}
          {commits && commits.length > 0 && !atEnd && (
            <button className="small ghost commit-more" disabled={loadingMore} onClick={loadMore}>
              {loadingMore ? "Carico…" : "Carica altri"}
            </button>
          )}
          {dirty && (
            <div className="hint">
              Checkout disabilitato: working tree non pulito (il detached HEAD scarterebbe le
              modifiche).
            </div>
          )}
        </div>
      )}
    </div>
  );
}
