import { useMemo, useState } from "react";
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

const STATUS_LABEL: Record<DiffStatus, string> = {
  onlyLeft: "solo a sinistra",
  onlyRight: "solo a destra",
  different: "dimensioni diverse",
};

const FILTERS: { id: "all" | DiffStatus; label: string }[] = [
  { id: "all", label: "Tutte" },
  { id: "onlyLeft", label: "◀ Solo sinistra" },
  { id: "onlyRight", label: "Solo destra ▶" },
  { id: "different", label: "≠ Diverse" },
];

function PathField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (path: string) => void;
}) {
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
          placeholder="percorso della cartella"
          spellCheck={false}
        />
        <button type="button" className="small" onClick={toggle}>
          {open ? "Chiudi" : "Sfoglia…"}
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
                Usa questa cartella
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
            {listing && listing.dirs.length === 0 && <li className="dim">nessuna sottocartella</li>}
          </ul>
        </div>
      )}
    </div>
  );
}

function sizeCell(entry: DiffEntry, size: number | null) {
  if (size == null) return <span className="compare-missing">—</span>;
  if (entry.isDir) return <span className="dim">cartella</span>;
  return <>{fmtBytes(size)}</>;
}

export function Compare() {
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
          ? "copiato a destra"
          : action === "toLeft"
            ? "copiato a sinistra"
            : `eliminato a ${side === "left" ? "sinistra" : "destra"}`;
      setDone((prev) => ({ ...prev, [entry.relPath]: label }));
    } else {
      setRowError({ rel: entry.relPath, message: r.error.message });
    }
  };

  const copy = (entry: DiffEntry, action: "toRight" | "toLeft") => {
    if (entry.status === "different") {
      const where = action === "toRight" ? "destra" : "sinistra";
      if (!confirm(`Sovrascrivere "${entry.relPath}" a ${where} con la versione dell'altro ramo?`))
        return;
    }
    apply(entry, action);
  };

  const remove = (entry: DiffEntry, side: "left" | "right") => {
    const root = side === "left" ? result?.left : result?.right;
    const what = entry.isDir ? "la cartella (e tutto il contenuto)" : "il file";
    if (!confirm(`Eliminare ${what}\n${root}\\${entry.relPath}\n\nL'operazione non è annullabile.`))
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
        <PathField label="Ramo sinistro" value={paths.left} onChange={(p) => update({ left: p })} />
        <PathField label="Ramo destro" value={paths.right} onChange={(p) => update({ right: p })} />
        <label className="form-row">
          <span title="Nomi di file/cartelle da saltare, separati da virgola">Ignora nomi</span>
          <input
            value={paths.excludes}
            onChange={(e) => update({ excludes: e.target.value })}
            placeholder=".git, node_modules  (vuoto = confronta tutto)"
            spellCheck={false}
          />
          <button
            onClick={run}
            disabled={busy || !paths.left.trim() || !paths.right.trim()}
          >
            {busy ? "Confronto…" : "Confronta"}
          </button>
        </label>
      </div>

      {error && <div className="banner banner-error">{error}</div>}

      {result && (
        <>
          <div className="compare-summary">
            <span>
              {result.entries.length} differenze su {result.compared} voci ·{" "}
              <span className="dim">{result.identical} identiche</span>
            </span>
            <div className="segmented">
              {FILTERS.map((f) => (
                <button
                  key={f.id}
                  className={filter === f.id ? "active" : ""}
                  onClick={() => setFilter(f.id)}
                >
                  {f.label}
                  {f.id !== "all" && <span className="dim"> {counts[f.id as DiffStatus]}</span>}
                </button>
              ))}
            </div>
            <button className="small" onClick={run} disabled={busy}>
              Ricontrolla
            </button>
          </div>

          {result.entries.length > 0 && (
            <div className="hint compare-legend">
              Su ogni riga: <b>→</b> porta a destra · <b>←</b> porta a sinistra · <b>⊘</b> ignora ·{" "}
              <b>🗑</b> elimina. Una cartella si apre con <b>▸</b> se vuoi decidere file per file
              invece che sul blocco intero.
            </div>
          )}

          {result.truncated && (
            <div className="banner banner-warn">
              Troppe differenze: l'elenco è stato troncato. Restringi le cartelle o aggiungi
              qualche nome da ignorare.
            </div>
          )}

          {result.entries.length === 0 && (
            <div className="empty">Le due alberature sono identiche.</div>
          )}

          {result.entries.length > 0 && visible.length === 0 && (
            <div className="empty">Nessuna differenza in questo filtro.</div>
          )}

          {visible.length > 0 && (
            <div className="table-scroll">
              <table className="proc-table compare-table">
                <thead>
                  <tr>
                    <th>Percorso</th>
                    <th className="num">Sinistra</th>
                    <th className="num">Destra</th>
                    <th className="num">Azioni</th>
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
                          <span className={`compare-badge ${e.status}`} title={STATUS_LABEL[e.status]}>
                            {e.status === "onlyLeft" ? "◀" : e.status === "onlyRight" ? "▶" : "≠"}
                          </span>
                          {e.isDir ? (
                            <button
                              className="compare-expand"
                              title={isOpen ? "Chiudi la cartella" : "Apri: agisci sui singoli file"}
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
                        <td className="num dim">{sizeCell(e, e.leftSize)}</td>
                        <td className="num dim">{sizeCell(e, e.rightSize)}</td>
                        <td className="num">
                          {askDelete === e.relPath ? (
                            <div className="compare-actions">
                              <span className="dim">Elimina da:</span>
                              <button className="small danger" onClick={() => remove(e, "left")}>
                                sinistra
                              </button>
                              <button className="small danger" onClick={() => remove(e, "right")}>
                                destra
                              </button>
                              <button className="small ghost" onClick={() => setAskDelete(null)}>
                                ✕
                              </button>
                            </div>
                          ) : (
                            <div className="compare-actions">
                              <button
                                className="compare-btn"
                                title={
                                  e.isDir
                                    ? "Porta a destra tutta la cartella (aprila per scegliere i singoli file)"
                                    : "Porta a destra (copia dal ramo sinistro)"
                                }
                                aria-label="Porta a destra"
                                disabled={busyRow || !!resolved || e.leftSize == null}
                                onClick={() => copy(e, "toRight")}
                              >
                                →
                              </button>
                              <button
                                className="compare-btn"
                                title={
                                  e.isDir
                                    ? "Porta a sinistra tutta la cartella (aprila per scegliere i singoli file)"
                                    : "Porta a sinistra (copia dal ramo destro)"
                                }
                                aria-label="Porta a sinistra"
                                disabled={busyRow || !!resolved || e.rightSize == null}
                                onClick={() => copy(e, "toLeft")}
                              >
                                ←
                              </button>
                              <button
                                className="compare-btn"
                                title="Ignora questa differenza"
                                aria-label="Ignora"
                                disabled={busyRow}
                                onClick={() => setIgnored((prev) => [...prev, e.relPath])}
                              >
                                ⊘
                              </button>
                              <button
                                className="compare-btn danger"
                                title={e.isDir ? "Elimina tutta la cartella" : "Elimina"}
                                aria-label="Elimina"
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
              {ignored.length} differenze ignorate ·{" "}
              <button className="small ghost" onClick={() => setIgnored([])}>
                mostra di nuovo
              </button>
            </div>
          )}
        </>
      )}

      {!result && !error && (
        <div className="empty">
          Indica due cartelle e premi Confronta: vedrai cosa ha in più il ramo di sinistra, cosa
          quello di destra e i file con lo stesso percorso ma dimensioni diverse.
        </div>
      )}
    </div>
  );
}
