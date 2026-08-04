import { useCallback, useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { TaskLog } from "../../components/TaskLog";
import type { ApiError, RunnerAction, RunnerInfo, TaskInfo } from "../../lib/types";

const KIND_META: Record<string, { icon: string; title: string }> = {
  python: { icon: "🐍", title: "Python" },
  rust: { icon: "🦀", title: "Rust" },
  tauri: { icon: "🖥️", title: "Tauri" },
  flutter: { icon: "🐦", title: "Flutter" },
};

const PRIMARY: RunnerAction["category"][] = ["run"];

export function RunnerPanel({ path, kind }: { path: string; kind: string }) {
  const [info, setInfo] = useState<RunnerInfo | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [task, setTask] = useState<TaskInfo | null>(null);

  const load = useCallback(async () => {
    const r = await api<RunnerInfo>(
      `/api/runner/info?kind=${encodeURIComponent(kind)}&path=${encodeURIComponent(path)}`,
    );
    if (r.ok) {
      setInfo(r.data);
      setError(null);
    } else setError(r.error);
  }, [path, kind]);

  useEffect(() => {
    setInfo(null);
    setTask(null);
    load();
  }, [load]);

  const run = async (action: RunnerAction) => {
    const r = await post<TaskInfo>("/api/runner/run", { path, kind, actionId: action.id });
    if (r.ok) {
      setTask(r.data);
      setError(null);
    } else setError(r.error);
  };

  const meta = KIND_META[kind] ?? { icon: "▢", title: kind };
  const running = task?.state === "running";

  return (
    <div className="node-panel">
      <h3>
        {meta.icon} {meta.title}
        {info && <span className="dim"> — {info.tool}</span>}
      </h3>

      {info?.notes.map((n) => (
        <div key={n} className="hint">
          {n}
        </div>
      ))}

      {info && (
        <div className="node-actions">
          {info.actions.map((a) => (
            <button
              key={a.id}
              className={PRIMARY.includes(a.category) ? "" : "small"}
              disabled={running}
              title={`${a.program} ${a.args.join(" ")}`}
              onClick={() => run(a)}
            >
              {a.label}
            </button>
          ))}
          {info.actions.length === 0 && (
            <span className="dim">Nessuna azione disponibile per questa cartella.</span>
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
