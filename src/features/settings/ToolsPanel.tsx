import { useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import type { DiscoveredTool } from "../../lib/types";

const TOOL_LABELS: Record<string, string> = {
  vscode: "Visual Studio Code",
  visualstudio: "Visual Studio",
  git: "Git",
  node: "Node.js",
  npm: "npm",
  yarn: "Yarn",
  pnpm: "pnpm",
  dotnet: ".NET SDK",
  docker: "Docker",
  terminal: "Terminale",
};

const LAUNCHABLE = new Set(["vscode", "visualstudio", "terminal"]);

const SOURCE_LABELS: Record<string, string> = {
  wellKnownPath: "path noto",
  registry: "registro",
  PATH: "PATH",
  userConfig: "manuale",
  none: "",
};

export function ToolsPanel() {
  const [tools, setTools] = useState<DiscoveredTool[] | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [editPath, setEditPath] = useState("");
  const [busy, setBusy] = useState(false);

  const load = async (refresh = false) => {
    setBusy(true);
    const r = await api<{ tools: DiscoveredTool[] }>(
      `/api/tools${refresh ? "?refresh=true" : ""}`,
    );
    setBusy(false);
    if (r.ok) setTools(r.data.tools);
  };

  useEffect(() => {
    load();
  }, []);

  const saveOverride = async (id: string, path: string | null) => {
    setBusy(true);
    const r = await post<{ tools: DiscoveredTool[] }>(`/api/tools/${id}/path`, { path });
    setBusy(false);
    setEditing(null);
    if (r.ok) setTools(r.data.tools);
  };

  const launch = (id: string) => post(`/api/tools/${id}/launch`, {});

  return (
    <section>
      <div className="section-header">
        <h3>Strumenti rilevati</h3>
        <button onClick={() => load(true)} disabled={busy}>
          {busy ? "Rilevo…" : "Rileva di nuovo"}
        </button>
      </div>
      {!tools && <div className="empty">Rilevamento…</div>}
      {tools && (
        <table className="proc-table">
          <tbody>
            {tools.map((t) => (
              <tr key={t.id} title={t.path ?? undefined}>
                <td>{TOOL_LABELS[t.id] ?? t.id}</td>
                <td>
                  {t.found ? (
                    <span className="badge badge-ok">trovato</span>
                  ) : (
                    <span className="badge">assente</span>
                  )}
                  {t.source !== "none" && (
                    <span className="dim"> · {SOURCE_LABELS[t.source]}</span>
                  )}
                </td>
                <td className="dim">
                  {t.version ?? t.platformNote ?? ""}
                  {t.editions && t.editions.length > 1 && (
                    <span> · {t.editions.length} edizioni</span>
                  )}
                </td>
                <td className="num">
                  {editing === t.id ? (
                    <span className="tool-edit">
                      <input
                        value={editPath}
                        onChange={(e) => setEditPath(e.target.value)}
                        placeholder="/percorso/eseguibile"
                        autoFocus
                      />
                      <button onClick={() => saveOverride(t.id, editPath)}>Salva</button>
                      {t.source === "userConfig" && (
                        <button onClick={() => saveOverride(t.id, null)}>Auto</button>
                      )}
                      <button onClick={() => setEditing(null)}>✕</button>
                    </span>
                  ) : (
                    <>
                      {t.found && LAUNCHABLE.has(t.id) && (
                        <button className="small" onClick={() => launch(t.id)}>
                          Apri
                        </button>
                      )}{" "}
                      <button
                        className="small"
                        onClick={() => {
                          setEditing(t.id);
                          setEditPath(t.path ?? "");
                        }}
                      >
                        Path…
                      </button>
                    </>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
