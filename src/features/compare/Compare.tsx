import { useMemo, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { fmtBytes } from "../../lib/format";
import type { CompareResult, DiffEntry, DiffStatus, DirListing } from "../../lib/types";

const STORE_KEY = "rdt-compare-paths";
const DEFAULT_EXCLUDES = ".git, node_modules";

interface StoredPaths {
  left: string;
  right: string;
  excludes: string;
}

function loadPaths(): StoredPaths {
  try {
    const parsed = JSON.parse(localStorage.getItem(STORE_KEY) ?? "null");
    if (parsed && typeof parsed === "object") {
      return {
        left: typeof parsed.left === "string" ? parsed.left : "",
        right: typeof parsed.right === "string" ? parsed.right : "",
        excludes: typeof parsed.excludes === "string" ? parsed.excludes : DEFAULT_EXCLUDES,
      };
    }
  } catch {
  }
  return { left: "", right: "", excludes: DEFAULT_EXCLUDES };
}

const STATUS_LABEL_KEYS = {
  onlyLeft: "tool.compare.statusOnlyLeft",
  onlyRight: "tool.compare.statusOnlyRight",
  different: "tool.compare.statusDifferent",
} as const;

const FILTERS = [
  { id: "all", labelKey: "tool.compare.filterAll" },
  { id: "onlyLeft", labelKey: "tool.compare.filterOnlyLeft" },
  { id: "onlyRight", labelKey: "tool.compare.filterOnlyRight" },
  { id: "different", labelKey: "tool.compare.filterDifferent" },
] as const;

function PathField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (path: string) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [listing, setListing] = useState<DirListing | null>(null);
  const [error, setError] = useState<string | null>(null);

  const browse = async (path: string | null) => {
    const query = path ? `?path=${encodeURIComponent(path)}` : "";
    const r = await api<DirListing>(`/api/fs/dirs${query}`);
    if (r.ok) {
      setListing(r.data);
      setError(null);
    } else setError(r.error.message);
  };

  const toggle = () => {
    const next = !open;
    setOpen(next);
    if (next) browse(value.trim() || null);
  };

  return (
    <div className="compare-path">
      <label className="form-row">
        <span>{label}</span>
        <input
          value={value}
          onChange={(e) => onChange(e.target.value)}
          placeholder={t("tool.compare.folderPathPlaceholder")}
          spellCheck={false}
        />
        <button type="button" className="small" onClick={toggle}>
          {open ? t("common.close") : t("tool.compare.browse")}
        </button>
      </label>
      {open && (
        <div className="browser">
          <div className="browser-path">
            <code>{listing?.path ?? "…"}</code>
            <span className="browser-actions">
              <button
                className="small"
                disabled={!listing}
                onClick={() => {
                  if (!listing) return;
                  onChange(listing.path);
                  setOpen(false);
                }}
              >
                {t("tool.compare.useThisFolder")}
              </button>
            </span>
          </div>
          {error && <div className="banner banner-error">{error}</div>}
          <ul className="browser-list">
            {listing?.parent && (
              <li>
                <button onClick={() => browse(listing.parent)}>..</button>
              </li>
            )}
            {listing?.dirs.map((d) => (
              <li key={d.path}>
                <button onClick={() => browse(d.path)}>📁 {d.name}</button>
              </li>
            ))}
            {listing && listing.dirs.length === 0 && <li className="dim">{t("tool.compare.noSubfolders")}</li>}
          </ul>
        </div>
      )}
    </div>
  );
}

function sizeCell(entry: DiffEntry, size: number | null, folderLabel: string) {
  if (size == null) return <span className="compare-missing">—</span>;
  if (entry.isDir) return <span className="dim">{folderLabel}</span>;
  return <>{fmtBytes(size)}</>;
}

export function Compare() {
  const { t } = useTranslation();
  const [paths, setPaths] = useState<StoredPaths>(loadPaths);
  const [result, setResult] = useState<CompareResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<"all" | DiffStatus>("all");
  const [ignored, setIgnored] = useState<string[]>([]);
  const [done, setDone] = useState<Record<string, string>>({});
  const [pending, setPending] = useState<string | null>(null);
  const [askDelete, setAskDelete] = useState<string | null>(null);
  const [rowError, setRowError] = useState<{ rel: string; message: string } | null>(null);
  const [expanded, setExpanded] = useState<string[]>([]);
  const [children, setChildren] = useState<Record<string, DiffEntry[]>>({});
  const [loadingKids, setLoadingKids] = useState<string | null>(null);

  const update = (patch: Partial<StoredPaths>) => {
    setPaths((prev) => {
      const next = { ...prev, ...patch };
      localStorage.setItem(STORE_KEY, JSON.stringify(next));
      return next;
    });
  };

  const excludeList = () =>
    paths.excludes
      .split(",")
      .map((e) => e.trim())
      .filter(Boolean);

  const run = async () => {
    setBusy(true);
    setError(null);
    setRowError(null);
    const r = await post<CompareResult>("/api/fs/compare", {
      left: paths.left.trim(),
      right: paths.right.trim(),
      excludes: excludeList(),
    });
    setBusy(false);
    if (r.ok) {
      setResult(r.data);
      setIgnored([]);
      setDone({});
      setExpanded([]);
      setChildren({});
      update({ left: r.data.left, right: r.data.right });
    } else {
      setResult(null);
      setError(r.error.message);
    }
  };

  const toggleFolder = async (entry: DiffEntry) => {
    if (!result) return;
    if (expanded.includes(entry.relPath)) {
      setExpanded((prev) => prev.filter((p) => p !== entry.relPath));
      return;
    }
    setExpanded((prev) => [...prev, entry.relPath]);
    if (children[entry.relPath]) return;
    setLoadingKids(entry.relPath);
    const r = await post<{ entries: DiffEntry[] }>("/api/fs/compare/children", {
      left: result.left,
      right: result.right,
      relPath: entry.relPath,
      excludes: excludeList(),
    });
    setLoadingKids(null);
    if (r.ok) setChildren((prev) => ({ ...prev, [entry.relPath]: r.data.entries }));
    else setRowError({ rel: entry.relPath, message: r.error.message });
  };

  const apply = async (entry: DiffEntry, action: "toRight" | "toLeft" | "delete", side?: "left" | "right") => {
    if (!result) return;
    setPending(entry.relPath);
    setRowError(null);
    const r = await post("/api/fs/compare/apply", {
      left: result.left,
      right: result.right,
      relPath: entry.relPath,
      action,
      side,
    });
    setPending(null);
    setAskDelete(null);
    if (r.ok) {
      const label =
        action === "toRight"
          ? t("tool.compare.copiedRight")
          : action === "toLeft"
            ? t("tool.compare.copiedLeft")
            : side === "left"
              ? t("tool.compare.deletedLeft")
              : t("tool.compare.deletedRight");
      setDone((prev) => ({ ...prev, [entry.relPath]: label }));
    } else {
      setRowError({ rel: entry.relPath, message: r.error.message });
    }
  };

  const copy = (entry: DiffEntry, action: "toRight" | "toLeft") => {
    if (entry.status === "different") {
      const where = action === "toRight" ? t("tool.compare.right") : t("tool.compare.left");
      if (!confirm(t("tool.compare.overwriteConfirm", { path: entry.relPath, where }))) return;
    }
    apply(entry, action);
  };

  const remove = (entry: DiffEntry, side: "left" | "right") => {
    const root = side === "left" ? result?.left : result?.right;
    const what = entry.isDir
      ? t("tool.compare.removeConfirmDir")
      : t("tool.compare.removeConfirmFile");
    if (!confirm(t("tool.compare.removeConfirm", { what, path: `${root}\\${entry.relPath}` })))
      return;
    apply(entry, "delete", side);
  };

  const visible = useMemo(() => {
    if (!result) return [];
    const walk = (entries: DiffEntry[], depth: number): { entry: DiffEntry; depth: number }[] =>
      entries.flatMap((entry) => {
        if (ignored.includes(entry.relPath)) return [];
        const kids = expanded.includes(entry.relPath) ? children[entry.relPath] : undefined;
        return [
          { entry, depth },
          ...(kids ? walk(kids, depth + 1) : []),
        ];
      });
    const top = result.entries.filter((e) => filter === "all" || e.status === filter);
    return walk(top, 0);
  }, [result, ignored, filter, expanded, children]);

  const counts = useMemo(() => {
    const base = { onlyLeft: 0, onlyRight: 0, different: 0 };
    for (const e of result?.entries ?? []) base[e.status] += 1;
    return base;
  }, [result]);

  return (
    <div className="tool-panel compare-tool">
      <div className="compare-form">
        <PathField label={t("tool.compare.leftBranch")} value={paths.left} onChange={(p) => update({ left: p })} />
        <PathField label={t("tool.compare.rightBranch")} value={paths.right} onChange={(p) => update({ right: p })} />
        <label className="form-row">
          <span title={t("tool.compare.ignoreNamesTitle")}>{t("tool.compare.ignoreNames")}</span>
          <input
            value={paths.excludes}
            onChange={(e) => update({ excludes: e.target.value })}
            placeholder={t("tool.compare.ignorePlaceholder")}
            spellCheck={false}
          />
          <button
            onClick={run}
            disabled={busy || !paths.left.trim() || !paths.right.trim()}
          >
            {busy ? t("tool.compare.comparing") : t("tool.compare.compareBtn")}
          </button>
        </label>
      </div>

      {error && <div className="banner banner-error">{error}</div>}

      {result && (
        <>
          <div className="compare-summary">
            <span>
              {t("tool.compare.summaryDiffs", {
                diffs: result.entries.length,
                compared: result.compared,
              })}{" "}
              · <span className="dim">{t("tool.compare.summaryIdentical", { count: result.identical })}</span>
            </span>
            <div className="segmented">
              {FILTERS.map((f) => (
                <button
                  key={f.id}
                  className={filter === f.id ? "active" : ""}
                  onClick={() => setFilter(f.id)}
                >
                  {t(f.labelKey)}
                  {f.id !== "all" && <span className="dim"> {counts[f.id as DiffStatus]}</span>}
                </button>
              ))}
            </div>
            <button className="small" onClick={run} disabled={busy}>
              {t("tool.compare.recheck")}
            </button>
          </div>

          {result.entries.length > 0 && (
            <div className="hint compare-legend">
              <Trans i18nKey="tool.compare.legend" components={{ b: <b /> }} />
            </div>
          )}

          {result.truncated && (
            <div className="banner banner-warn">{t("tool.compare.truncated")}</div>
          )}

          {result.entries.length === 0 && (
            <div className="empty">{t("tool.compare.identical")}</div>
          )}

          {result.entries.length > 0 && visible.length === 0 && (
            <div className="empty">{t("tool.compare.noneInFilter")}</div>
          )}

          {visible.length > 0 && (
            <div className="table-scroll">
              <table className="proc-table compare-table">
                <thead>
                  <tr>
                    <th>{t("tool.compare.colPath")}</th>
                    <th className="num">{t("tool.compare.colLeft")}</th>
                    <th className="num">{t("tool.compare.colRight")}</th>
                    <th className="num">{t("tool.compare.colActions")}</th>
                  </tr>
                </thead>
                <tbody>
                  {visible.map(({ entry: e, depth }) => {
                    const resolved = done[e.relPath];
                    const busyRow = pending === e.relPath;
                    const isOpen = expanded.includes(e.relPath);
                    return (
                      <tr key={e.relPath} className={resolved ? "compare-done" : undefined}>
                        <td>
                          <span className="compare-indent" style={{ width: depth * 16 }} />
                          <span className={`compare-badge ${e.status}`} title={t(STATUS_LABEL_KEYS[e.status])}>
                            {e.status === "onlyLeft" ? "◀" : e.status === "onlyRight" ? "▶" : "≠"}
                          </span>
                          {e.isDir ? (
                            <button
                              className="compare-expand"
                              title={isOpen ? t("tool.compare.closeFolder") : t("tool.compare.openFolder")}
                              aria-expanded={isOpen}
                              onClick={() => toggleFolder(e)}
                            >
                              {loadingKids === e.relPath ? "…" : isOpen ? "▾" : "▸"}
                            </button>
                          ) : (
                            <span className="compare-expand" aria-hidden />
                          )}
                          <span className="compare-rel">
                            {e.isDir ? "📁 " : ""}
                            {depth > 0 ? e.relPath.split("/").pop() : e.relPath}
                          </span>
                          {resolved && <span className="badge">{resolved}</span>}
                          {rowError?.rel === e.relPath && (
                            <div className="banner banner-error compare-row-error">
                              {rowError.message}
                            </div>
                          )}
                        </td>
                        <td className="num dim">{sizeCell(e, e.leftSize, t("tool.compare.folder"))}</td>
                        <td className="num dim">{sizeCell(e, e.rightSize, t("tool.compare.folder"))}</td>
                        <td className="num">
                          {askDelete === e.relPath ? (
                            <div className="compare-actions">
                              <span className="dim">{t("tool.compare.deleteFrom")}</span>
                              <button className="small danger" onClick={() => remove(e, "left")}>
                                {t("tool.compare.left")}
                              </button>
                              <button className="small danger" onClick={() => remove(e, "right")}>
                                {t("tool.compare.right")}
                              </button>
                              <button className="small ghost" onClick={() => setAskDelete(null)}>
                                ✕
                              </button>
                            </div>
                          ) : (
                            <div className="compare-actions">
                              <button
                                className="compare-btn"
                                title={e.isDir ? t("tool.compare.toRightDirTitle") : t("tool.compare.toRightFileTitle")}
                                aria-label={t("tool.compare.toRight")}
                                disabled={busyRow || !!resolved || e.leftSize == null}
                                onClick={() => copy(e, "toRight")}
                              >
                                →
                              </button>
                              <button
                                className="compare-btn"
                                title={e.isDir ? t("tool.compare.toLeftDirTitle") : t("tool.compare.toLeftFileTitle")}
                                aria-label={t("tool.compare.toLeft")}
                                disabled={busyRow || !!resolved || e.rightSize == null}
                                onClick={() => copy(e, "toLeft")}
                              >
                                ←
                              </button>
                              <button
                                className="compare-btn"
                                title={t("tool.compare.ignoreTitle")}
                                aria-label={t("tool.compare.ignore")}
                                disabled={busyRow}
                                onClick={() => setIgnored((prev) => [...prev, e.relPath])}
                              >
                                ⊘
                              </button>
                              <button
                                className="compare-btn danger"
                                title={e.isDir ? t("tool.compare.deleteDirTitle") : t("tool.compare.delete")}
                                aria-label={t("tool.compare.delete")}
                                disabled={busyRow || !!resolved}
                                onClick={() => {
                                  // Presente da un lato solo: non c'è nulla da
                                  if (e.status === "onlyLeft") remove(e, "left");
                                  else if (e.status === "onlyRight") remove(e, "right");
                                  else setAskDelete(e.relPath);
                                }}
                              >
                                🗑
                              </button>
                            </div>
                          )}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}

          {ignored.length > 0 && (
            <div className="dim compare-ignored">
              {t("tool.compare.ignoredCount", { count: ignored.length })} ·{" "}
              <button className="small ghost" onClick={() => setIgnored([])}>
                {t("tool.compare.showAgain")}
              </button>
            </div>
          )}
        </>
      )}

      {!result && !error && <div className="empty">{t("tool.compare.emptyIntro")}</div>}
    </div>
  );
}
