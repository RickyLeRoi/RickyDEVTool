import { useCallback, useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { BranchPicker } from "./BranchPicker";
import { CommitList } from "./CommitList";
import type { ApiError, GitRepoInfo, GitWarning } from "../../lib/types";

function warningText(w: GitWarning): string {
  switch (w.kind) {
    case "no-upstream":
      return "nessun upstream configurato";
    case "diverged":
      return `diverged: ↑${w.ahead} ↓${w.behind}`;
    case "detached-head":
      return "detached HEAD";
    case "merge-in-progress":
      return "merge in corso";
    case "stale-fetch":
      return `ultimo fetch ${w.days} giorni fa`;
  }
}

export function GitPanel({ path }: { path: string }) {
  const [info, setInfo] = useState<GitRepoInfo | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [busy, setBusy] = useState<"fetch" | "pull" | null>(null);
  const [summary, setSummary] = useState<string | null>(null);
  // Branch di cui mostrare i commit; null = HEAD (branch corrente).
  const [commitsRef, setCommitsRef] = useState<string | null>(null);

  const load = useCallback(async () => {
    const r = await api<GitRepoInfo>(`/api/git/info?path=${encodeURIComponent(path)}`);
    if (r.ok) {
      setInfo(r.data);
      setError(null);
    } else setError(r.error);
  }, [path]);

  useEffect(() => {
    setInfo(null);
    setSummary(null);
    setCommitsRef(null);
    load();
  }, [load]);

  const doFetch = async () => {
    setBusy("fetch");
    setSummary(null);
    const r = await post<GitRepoInfo>("/api/git/fetch", { path });
    setBusy(null);
    if (r.ok) {
      setInfo(r.data);
      setError(null);
      setSummary("Fetch completato");
    } else setError(r.error);
  };

  const doPull = async () => {
    setBusy("pull");
    setSummary(null);
    const r = await post<{ info: GitRepoInfo; summary: string }>("/api/git/pull", { path });
    setBusy(null);
    if (r.ok) {
      setInfo(r.data.info);
      setError(null);
      setSummary(r.data.summary || "Pull completato");
    } else setError(r.error);
  };

  if (!info && !error) return <div className="empty">Leggo lo stato git…</div>;

  return (
    <div className="git-panel">
      <h3>Git</h3>
      {info && (
        <>
          <div className="git-status-line">
            <span className="badge badge-branch">
              {info.currentBranch ?? `detached @ ${info.detachedAt ?? "?"}`}
            </span>
            {info.ahead !== null && (
              <span className="dim">
                ↑{info.ahead} ↓{info.behind}
              </span>
            )}
            {info.dirty ? (
              <span className="badge badge-warn">● {info.dirtyFiles} file modificati</span>
            ) : (
              <span className="badge badge-ok">pulito</span>
            )}
          </div>
          {info.warnings.length > 0 && (
            <div className="git-warnings">
              {info.warnings.map((w, i) => (
                <span key={i} className="badge badge-warn">
                  ⚠ {warningText(w)}
                </span>
              ))}
            </div>
          )}
          <div className="git-actions">
            <button onClick={doFetch} disabled={busy !== null}>
              {busy === "fetch" ? "Fetch…" : "Fetch"}
            </button>
            <button onClick={doPull} disabled={busy !== null || info.dirty}>
              {busy === "pull" ? "Pull…" : "Pull origin"}
            </button>
            {info.dirty && (
              <span className="dim">pull disabilitato: working tree non pulito</span>
            )}
          </div>
          <BranchPicker
            path={path}
            dirty={info.dirty}
            selectedRef={commitsRef}
            onSelectBranch={setCommitsRef}
            onCheckout={(updated) => {
              setInfo(updated);
              setCommitsRef(null);
              setSummary(`Checkout su ${updated.currentBranch ?? "detached"}`);
            }}
          />
          <CommitList
            path={path}
            dirty={info.dirty}
            refName={commitsRef}
            onCheckout={(updated) => {
              setInfo(updated);
              setCommitsRef(null);
              setSummary(
                updated.currentBranch
                  ? `Checkout su ${updated.currentBranch}`
                  : `Detached HEAD @ ${updated.detachedAt ?? "?"}`,
              );
            }}
          />
        </>
      )}
      {summary && <div className="dim">{summary}</div>}
      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}
    </div>
  );
}
