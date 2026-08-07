import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
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
};

const LAUNCHABLE = new Set(["vscode", "visualstudio", "terminal"]);

const SOURCE_KEYS: Record<string, string> = {
  wellKnownPath: "tools.sourceWellKnownPath",
  registry: "tools.sourceRegistry",
  PATH: "tools.sourcePath",
  userConfig: "tools.sourceUserConfig",
};

export function ToolsPanel() {
  const { t } = useTranslation();
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

  const toolLabel = (tool: DiscoveredTool) =>
    tool.id === "terminal" ? t("tools.terminal") : TOOL_LABELS[tool.id] ?? tool.id;

  return (
    <section>
      <div className="section-header">
        <h3>{t("tools.title")}</h3>
        <button onClick={() => load(true)} disabled={busy}>
          {busy ? t("tools.detectingBusy") : t("tools.detectAgain")}
        </button>
      </div>
      {!tools && <div className="empty">{t("tools.detecting")}</div>}
      {tools && (
        <table className="proc-table">
          <tbody>
            {tools.map((tool) => (
              <tr key={tool.id} title={tool.path ?? undefined}>
                <td>{toolLabel(tool)}</td>
                <td>
                  {tool.found ? (
                    <span className="badge badge-ok">{t("tools.found")}</span>
                  ) : (
                    <span className="badge">{t("tools.absent")}</span>
                  )}
                  {tool.source !== "none" && SOURCE_KEYS[tool.source] && (
                    <span className="dim">
                      {" "}
                      · {t(SOURCE_KEYS[tool.source] as "tools.sourcePath")}
                    </span>
                  )}
                </td>
                <td className="dim">
                  {tool.version ?? tool.platformNote ?? ""}
                  {tool.editions && tool.editions.length > 1 && (
                    <span> · {t("tools.editions", { count: tool.editions.length })}</span>
                  )}
                </td>
                <td className="num">
                  {editing === tool.id ? (
                    <span className="tool-edit">
                      <input
                        value={editPath}
                        onChange={(e) => setEditPath(e.target.value)}
                        placeholder={t("tools.pathPlaceholder")}
                        autoFocus
                      />
                      <button onClick={() => saveOverride(tool.id, editPath)}>
                        {t("common.save")}
                      </button>
                      {tool.source === "userConfig" && (
                        <button onClick={() => saveOverride(tool.id, null)}>
                          {t("tools.auto")}
                        </button>
                      )}
                      <button onClick={() => setEditing(null)}>✕</button>
                    </span>
                  ) : (
                    <>
                      {tool.found && LAUNCHABLE.has(tool.id) && (
                        <button className="small" onClick={() => launch(tool.id)}>
                          {t("common.open")}
                        </button>
                      )}{" "}
                      <button
                        className="small"
                        onClick={() => {
                          setEditing(tool.id);
                          setEditPath(tool.path ?? "");
                        }}
                      >
                        {t("tools.pathBtn")}
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
