import { useCallback, useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { TaskLog } from "../../components/TaskLog";
import type { ApiError, DotnetProject, TaskInfo } from "../../lib/types";

// L'OS del backend, per disabilitare le azioni Windows-only.
let cachedOs: string | null = null;
async function backendOs(): Promise<string> {
  if (!cachedOs) {
    const r = await api<{ os: string }>("/api/health");
    cachedOs = r.ok ? r.data.os : "unknown";
  }
  return cachedOs;
}

export function DotnetPanel({ path }: { path: string }) {
  const [project, setProject] = useState<DotnetProject | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [task, setTask] = useState<TaskInfo | null>(null);
  const [os, setOs] = useState<string>("unknown");

  const load = useCallback(async () => {
    const r = await api<DotnetProject>(`/api/dotnet/info?path=${encodeURIComponent(path)}`);
    if (r.ok) {
      setProject(r.data);
      setError(null);
    } else setError(r.error);
  }, [path]);

  useEffect(() => {
    setProject(null);
    setTask(null);
    load();
    backendOs().then(setOs);
  }, [load]);

  const select = async (startupProject: string | null, profile: string | null) => {
    const r = await post<DotnetProject>("/api/dotnet/select", {
      path,
      startupProject,
      profile,
    });
    if (r.ok) setProject(r.data);
  };

  const run = async (action: string) => {
    const r = await post<TaskInfo>("/api/dotnet/run", { path, action });
    if (r.ok) {
      setTask(r.data);
      setError(null);
    } else setError(r.error);
  };

  const openInVs = () =>
    post("/api/tools/visualstudio/launch", { target: project?.slnPath ?? path });

  if (!project && !error) return <div className="empty">Leggo la solution…</div>;

  const executables = project?.projects.filter((p) => p.isExecutable) ?? [];
  const startup = project?.projects.find(
    (p) => p.csprojPath === project.startupProjectPath,
  );
  const running = task?.state === "running";
  const isWindows = os === "windows";

  return (
    <div className="node-panel">
      <h3>
        .NET
        {project?.slnPath && (
          <span className="dim"> — {project.slnPath.split(/[\\/]/).pop()}</span>
        )}
      </h3>
      {project && (
        <>
          <div className="node-actions">
            <label className="dim">
              Avvio:{" "}
              <select
                value={project.startupProjectPath ?? ""}
                onChange={(e) => select(e.target.value || null, null)}
              >
                <option value="" disabled>
                  progetto…
                </option>
                {executables.map((p) => (
                  <option key={p.csprojPath} value={p.csprojPath}>
                    {p.name} ({p.targetFrameworks.join(", ")})
                  </option>
                ))}
              </select>
            </label>
            {startup && startup.launchProfiles.length > 0 && (
              <label className="dim">
                Profilo:{" "}
                <select
                  value={project.selectedProfile ?? ""}
                  onChange={(e) =>
                    select(project.startupProjectPath, e.target.value || null)
                  }
                >
                  {startup.launchProfiles.map((lp) => (
                    <option
                      key={lp.name}
                      value={lp.name}
                      disabled={!lp.runnableCrossPlatform}
                    >
                      {lp.name}
                      {!lp.runnableCrossPlatform ? " (solo VS/Windows)" : ""}
                    </option>
                  ))}
                </select>
              </label>
            )}
          </div>
          <div className="node-actions">
            <button
              onClick={() => run("run")}
              disabled={running || !project.startupProjectPath}
              title={!project.startupProjectPath ? "scegli il progetto di avvio" : undefined}
            >
              Run
            </button>
            <button onClick={() => run("rebuild")} disabled={running}>
              Rebuild
            </button>
            <button onClick={() => run("clean")} disabled={running}>
              Clean
            </button>
            <button
              onClick={openInVs}
              disabled={!isWindows}
              title={
                isWindows
                  ? "Apri la solution in Visual Studio"
                  : "Visual Studio è disponibile solo su Windows"
              }
            >
              Open in VS
            </button>
          </div>
          {executables.length === 0 && (
            <div className="banner banner-error">
              Nessun progetto eseguibile nella solution (solo librerie).
            </div>
          )}
        </>
      )}
      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}
      {task && <TaskLog key={task.id} task={task} />}
    </div>
  );
}
