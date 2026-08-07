import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { api, post } from "../../lib/api";
import { BranchPicker } from "./BranchPicker";
import { CommitList } from "./CommitList";
import type { ApiError, GitRepoInfo, GitWarning } from "../../lib/types";

function warningText(w: GitWarning, t: TFunction): string {
  switch (w.kind) {
    case "no-upstream":
      return t("projects.git.warnNoUpstream");
    case "diverged":
      return t("projects.git.warnDiverged", { ahead: w.ahead, behind: w.behind });
    case "detached-head":
      return t("projects.git.warnDetached");
    case "merge-in-progress":
      return t("projects.git.warnMerge");
    case "stale-fetch":
      return t("projects.git.warnStaleFetch", { days: w.days });
  }
}

export function GitPanel({ path }: { path: string }) {
  const { t } = useTranslation();
  const [info, setInfo] = useState<GitRepoInfo | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [busy, setBusy] = useState<"fetch" | "pull" | null>(null);
  const [summary, setSummary] = useState<string | null>(null);
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
      setSummary(t("projects.git.fetchDone"));
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
      setSummary(r.data.summary || t("projects.git.pullDone"));
    } else setError(r.error);
  };

  if (!info && !error) return <div className="empty">{t("projects.git.reading")}</div>;

  return (
    <div className="git-panel">
      <h3>{t("projects.git.title")}</h3>
      {info && (
        <>
          <div className="git-status-line">
            <span className="badge badge-branch">
              {info.currentBranch ?? t("projects.git.detachedAt", { at: info.detachedAt ?? "?" })}
            </span>
            {info.ahead !== null && (
              <span className="dim">
                ↑{info.ahead} ↓{info.behind}
              </span>
            )}
            {info.dirty ? (
              <span className="badge badge-warn">{t("projects.git.dirtyFiles", { count: info.dirtyFiles })}</span>
            ) : (
              <span className="badge badge-ok">{t("projects.git.clean")}</span>
            )}
          </div>
          {info.warnings.length > 0 && (
            <div className="git-warnings">
              {info.warnings.map((w, i) => (
                <span key={i} className="badge badge-warn">
                  ⚠ {warningText(w, t)}
                </span>
              ))}
            </div>
          )}
          <div className="git-actions">
            <button onClick={doFetch} disabled={busy !== null}>
              {busy === "fetch" ? t("projects.git.fetching") : t("projects.git.fetch")}
            </button>
            <button onClick={doPull} disabled={busy !== null || info.dirty}>
              {busy === "pull" ? t("projects.git.pulling") : t("projects.git.pull")}
            </button>
            {info.dirty && (
              <span className="dim">{t("projects.git.pullDisabled")}</span>
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
              setSummary(
                t("projects.git.checkoutTo", {
                  branch: updated.currentBranch ?? t("projects.git.detached"),
                }),
              );
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
                  ? t("projects.git.checkoutTo", { branch: updated.currentBranch })
                  : t("projects.git.detachedAt", { at: updated.detachedAt ?? "?" }),
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
