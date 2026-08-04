import { useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import type { ApiError, GitCommit, GitRepoInfo } from "../../lib/types";

interface CommitListProps {
  path: string;
  dirty: boolean;
  onCheckout: (info: GitRepoInfo) => void;
  refName: string | null;
}

const PAGE = 50;

function fmtDate(ms: number) {
  return new Date(ms).toLocaleDateString("it-IT", {
    day: "2-digit",
    month: "short",
    year: "2-digit",
  });
}

export function CommitList({ path, dirty, onCheckout, refName }: CommitListProps) {
  const [open, setOpen] = useState(false);
  const [commits, setCommits] = useState<GitCommit[] | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const [atEnd, setAtEnd] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<ApiError | null>(null);

  const fetchPage = async (skip: number): Promise<GitCommit[]> => {
    const refQ = refName ? `&ref=${encodeURIComponent(refName)}` : "";
    const r = await api<{ commits: GitCommit[] }>(
      `/api/git/commits?path=${encodeURIComponent(path)}&limit=${PAGE}&skip=${skip}${refQ}`,
    );
    if (r.ok) {
      setError(null);
      return r.data.commits;
    }
    setError(r.error);
    return [];
  };

  useEffect(() => {
    let cancelled = false;
    setCommits(null);
    setAtEnd(false);
    setError(null);
    if (refName == null) {
      setOpen(false);
      return;
    }
    setOpen(true);
    (async () => {
      const first = await fetchPage(0);
      if (!cancelled) {
        setCommits(first);
        setAtEnd(first.length < PAGE);
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refName, path]);

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

  const reloadFromTop = async () => {
    const first = await fetchPage(0);
    setCommits(first);
    setAtEnd(first.length < PAGE);
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

  const runAndReload = async (endpoint: string, c: GitCommit, confirmMsg: string) => {
    if (!window.confirm(confirmMsg)) return;
    setBusy(c.hash);
    setError(null);
    const r = await post<GitRepoInfo>(endpoint, { path, hash: c.hash });
    setBusy(null);
    if (r.ok) {
      onCheckout(r.data);
      await reloadFromTop();
    } else setError(r.error);
  };

  const revert = (c: GitCommit) =>
    runAndReload("/api/git/revert", c, `Creare un commit che annulla «${c.subject}»?`);
  const cherryPick = (c: GitCommit) =>
    runAndReload("/api/git/cherry-pick", c, `Applicare (cherry-pick) «${c.subject}» su HEAD?`);

  const rowDisabled = dirty || busy !== null;
  const dirtyTitle = dirty ? "working tree non pulito" : undefined;

  return (
    <div className="commit-list-wrap">
      <button onClick={toggle}>
        {open ? "▾" : "▸"} Cronologia commit
        {refName && <span className="dim"> — {refName}</span>}
      </button>
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
              <div className="git-row-actions">
                <button
                  className="git-act"
                  disabled={rowDisabled}
                  title={dirtyTitle ?? "Checkout (detached HEAD) su questo commit"}
                  onClick={() => checkout(c)}
                >
                  {busy === c.hash ? "…" : "⤓"}
                </button>
                <button
                  className="git-act"
                  disabled={rowDisabled}
                  title={dirtyTitle ?? "Revert: crea un commit che annulla questo"}
                  onClick={() => revert(c)}
                >
                  ↩
                </button>
                <button
                  className="git-act"
                  disabled={rowDisabled}
                  title={dirtyTitle ?? "Cherry-pick: applica questo commit su HEAD"}
                  onClick={() => cherryPick(c)}
                >
                  🍒
                </button>
              </div>
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
              Azioni git disabilitate: working tree non pulito (committa o stasha prima).
            </div>
          )}
        </div>
      )}
    </div>
  );
}
