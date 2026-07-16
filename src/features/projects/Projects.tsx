import { useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { GitPanel } from "./GitPanel";
import type { DirListing, FolderScan, ProjectRef } from "../../lib/types";

const KIND_LABELS: Record<string, string> = {
  git: " git",
  node: "⬢ node",
  dotnet: ".NET",
};

function CommonActions({ path }: { path: string }) {
  const launch = (id: string) => post(`/api/tools/${id}/launch`, { target: path });
  return (
    <div className="common-actions">
      <button className="small" title="Apri in VS Code" onClick={() => launch("vscode")}>
        VS Code
      </button>
      <button className="small" title="Apri terminale qui" onClick={() => launch("terminal")}>
        Terminale
      </button>
      <button
        className="small"
        title="Copia percorso"
        onClick={() => navigator.clipboard.writeText(path)}
      >
        Copia path
      </button>
    </div>
  );
}

export function Projects() {
  const [pinned, setPinned] = useState<string[]>([]);
  const [listing, setListing] = useState<DirListing | null>(null);
  const [scan, setScan] = useState<FolderScan | null>(null);
  const [selected, setSelected] = useState<ProjectRef | null>(null);
  const [browsing, setBrowsing] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api<{ folders: string[] }>("/api/projects/pinned").then((r) => {
      if (r.ok) setPinned(r.data.folders);
    });
    browse(null);
  }, []);

  const browse = async (path: string | null) => {
    const query = path ? `?path=${encodeURIComponent(path)}` : "";
    const r = await api<DirListing>(`/api/fs/dirs${query}`);
    if (r.ok) {
      setListing(r.data);
      setError(null);
    } else setError(r.error.message);
  };

  const doScan = async (path: string) => {
    setScanning(true);
    setSelected(null);
    const r = await api<FolderScan>(`/api/projects/scan?path=${encodeURIComponent(path)}`);
    setScanning(false);
    if (r.ok) {
      setScan(r.data);
      setBrowsing(false);
      setError(null);
      if (r.data.projects.length === 1) setSelected(r.data.projects[0]);
    } else setError(r.error.message);
  };

  const togglePin = async (path: string) => {
    const action = pinned.includes(path) ? "remove" : "add";
    const r = await post<{ folders: string[] }>("/api/projects/pinned", { path, action });
    if (r.ok) setPinned(r.data.folders);
  };

  const shortName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

  return (
    <div>
      <div className="section-header">
        <h2>Progetti</h2>
        <button onClick={() => setBrowsing(!browsing)}>
          {browsing ? "Chiudi" : "Apri cartella…"}
        </button>
      </div>

      {pinned.length > 0 && (
        <div className="pin-chips">
          {pinned.map((p) => (
            <span
              key={p}
              className={`chip ${scan?.path === p ? "active" : ""}`}
              title={p}
            >
              <button className="chip-label" onClick={() => doScan(p)}>
                {shortName(p)}
              </button>
              <button className="chip-x" title="Rimuovi pin" onClick={() => togglePin(p)}>
                ✕
              </button>
            </span>
          ))}
        </div>
      )}

      {error && <div className="banner banner-error">{error}</div>}

      {browsing && listing && (
        <div className="browser">
          <div className="browser-path">
            <code>{listing.path}</code>
            <span>
              <button className="small" onClick={() => doScan(listing.path)} disabled={scanning}>
                {scanning ? "Scansiono…" : "Usa questa cartella"}
              </button>{" "}
              <button className="small" onClick={() => togglePin(listing.path)}>
                {pinned.includes(listing.path) ? "Unpin" : "Pin"}
              </button>
            </span>
          </div>
          <ul className="browser-list">
            {listing.parent && (
              <li>
                <button onClick={() => browse(listing.parent)}>..</button>
              </li>
            )}
            {listing.dirs.map((d) => (
              <li key={d.path}>
                <button onClick={() => browse(d.path)}>📁 {d.name}</button>
              </li>
            ))}
            {listing.dirs.length === 0 && <li className="dim">nessuna sottocartella</li>}
          </ul>
        </div>
      )}

      {scan && (
        <div className="scan-result">
          <div className="scan-header">
            <code>{scan.path}</code>
            {scan.truncated && (
              <span className="badge badge-warn">scansione parziale (cartella enorme)</span>
            )}
          </div>
          {scan.projects.length === 0 && (
            <div className="empty">
              Nessun progetto riconosciuto (git, Node.js, .NET) entro 3 livelli.
            </div>
          )}
          <div className="project-layout">
            {scan.projects.length > 0 && (
              <ul className="project-list">
                {scan.projects.map((p) => (
                  <li key={p.path}>
                    <button
                      className={`project-item ${selected?.path === p.path ? "active" : ""}`}
                      onClick={() => setSelected(p)}
                      title={p.path}
                    >
                      <span>{p.name}</span>
                      <span className="project-badges">
                        {p.kinds.map((k) => (
                          <span key={k} className="badge badge-app">
                            {KIND_LABELS[k]}
                          </span>
                        ))}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
            {selected && (
              <div className="project-detail">
                <div className="project-detail-header">
                  <h3 title={selected.path}>{selected.name}</h3>
                  <CommonActions path={selected.path} />
                </div>
                {selected.kinds.includes("git") && <GitPanel path={selected.path} />}
                {selected.kinds.includes("node") && (
                  <div className="placeholder-note">Azioni Node.js: in arrivo con la M5.</div>
                )}
                {selected.kinds.includes("dotnet") && (
                  <div className="placeholder-note">Azioni .NET: in arrivo con la M6.</div>
                )}
              </div>
            )}
          </div>
        </div>
      )}

      {!scan && !browsing && pinned.length === 0 && (
        <div className="empty">
          Nessuna cartella aperta: usa "Apri cartella…" per iniziare, poi pinna quelle che usi
          spesso.
        </div>
      )}
    </div>
  );
}
