import { useCallback, useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { TaskLog } from "../../components/TaskLog";
import type { ApiError, NodeProject, TaskInfo } from "../../lib/types";

const PMS = ["npm", "yarn", "pnpm"] as const;

export function NodePanel({ path }: { path: string }) {
  const [project, setProject] = useState<NodeProject | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [task, setTask] = useState<TaskInfo | null>(null);
  const [pmMenuOpen, setPmMenuOpen] = useState(false);

  const load = useCallback(async () => {
    const r = await api<NodeProject>(`/api/node/info?path=${encodeURIComponent(path)}`);
    if (r.ok) {
      setProject(r.data);
      setError(null);
    } else setError(r.error);
  }, [path]);

  useEffect(() => {
    setProject(null);
    setTask(null);
    load();
  }, [load]);

  const run = async (script: string | null) => {
    const r = await post<TaskInfo>("/api/node/run", { path, script });
    if (r.ok) {
      setTask(r.data);
      setError(null);
    } else setError(r.error);
  };

  const setPm = async (pm: string | null) => {
    setPmMenuOpen(false);
    const r = await post<NodeProject>("/api/node/pm", { path, pm });
    if (r.ok) setProject(r.data);
  };

  if (!project && !error) return <div className="empty">Leggo package.json…</div>;

  const otherScripts = project
    ? Object.keys(project.scripts).filter((s) => s !== project.primaryStart)
    : [];

  return (
    <div className="node-panel">
      <h3>Node.js{project?.packageName ? ` — ${project.packageName}` : ""}</h3>
      {project && (
        <div className="node-actions">
          <span className="pm-badge">
            <button className="small" onClick={() => setPmMenuOpen(!pmMenuOpen)}>
              {project.packageManager} ▾
            </button>
            {project.pmSource === "default" && (
              <span className="badge badge-warn" title="nessun lockfile: npm assunto">
                assunto
              </span>
            )}
            {project.pmSource === "userOverride" && <span className="badge">manuale</span>}
            {pmMenuOpen && (
              <span className="pm-menu">
                {PMS.map((pm) => (
                  <button key={pm} className="small" onClick={() => setPm(pm)}>
                    {pm}
                  </button>
                ))}
                <button className="small" onClick={() => setPm(null)}>
                  auto
                </button>
              </span>
            )}
          </span>
          <button onClick={() => run(null)} disabled={task?.state === "running"}>
            Install
          </button>
          {project.primaryStart && (
            <button
              onClick={() => run(project.primaryStart)}
              disabled={task?.state === "running"}
              title={project.scripts[project.primaryStart]}
            >
              Start ({project.primaryStart})
            </button>
          )}
          {}
          {otherScripts.map((s) => (
            <button
              key={s}
              className="small script-btn"
              onClick={() => run(s)}
              disabled={task?.state === "running"}
              title={project.scripts[s]}
            >
              {s}
            </button>
          ))}
          {!project.nodeModulesPresent && (
            <span className="badge badge-warn">node_modules assente</span>
          )}
        </div>
      )}
      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}
      {task && <TaskLog key={task.id} task={task} onDone={() => load()} />}
    </div>
  );
}
