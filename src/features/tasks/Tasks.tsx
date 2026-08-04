import { useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { ws } from "../../lib/ws";
import { TaskLog } from "../../components/TaskLog";
import type { TaskInfo } from "../../lib/types";

function fmtTime(ms: number) {
  return new Date(ms).toLocaleTimeString("it-IT", {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function StateBadge({ state, exitCode }: { state: TaskInfo["state"]; exitCode: number | null }) {
  if (state === "running") return <span className="badge badge-branch">in esecuzione</span>;
  if (state === "exited" && exitCode === 0)
    return <span className="badge badge-ok">completato</span>;
  return <span className="badge badge-warn">uscito con {exitCode ?? "?"}</span>;
}

export function Tasks() {
  const [tasks, setTasks] = useState<TaskInfo[] | null>(null);
  const [openId, setOpenId] = useState<string | null>(null);

  const load = async () => {
    const r = await api<{ tasks: TaskInfo[] }>("/api/tasks");
    if (r.ok) setTasks(r.data.tasks);
  };

  useEffect(() => {
    load();
    return ws.subscribe("tasks", (event) => {
      if (event.topic === "tasks") setTasks((event.payload as { tasks: TaskInfo[] }).tasks);
    });
  }, []);

  const clearFinished = async () => {
    const r = await post<{ tasks: TaskInfo[] }>("/api/tasks/clear-finished", {});
    if (r.ok) {
      setTasks(r.data.tasks);
      if (openId && !r.data.tasks.some((t) => t.id === openId)) setOpenId(null);
    }
  };

  const openTask = tasks?.find((t) => t.id === openId) ?? null;
  const hasFinished = (tasks ?? []).some((t) => t.state !== "running");

  return (
    <div>
      <div className="section-header">
        <h2>Task</h2>
        {hasFinished && (
          <button className="small" onClick={clearFinished}>
            Pulisci terminati
          </button>
        )}
      </div>

      <p className="hint">
        Comandi avviati dall'app (npm/yarn, dotnet, traceroute, log docker). L'output resta
        bufferizzato: puoi riaprire il log anche dopo la fine.
      </p>

      {!tasks && <div className="empty">Carico i task…</div>}
      {tasks && tasks.length === 0 && <div className="empty">Nessun task avviato.</div>}

      {tasks && tasks.length > 0 && (
        <table className="proc-table task-table">
          <tbody>
            {tasks.map((t) => (
              <tr
                key={t.id}
                className={t.id === openId ? "row-editing" : "row-clickable"}
                onClick={() => setOpenId(t.id === openId ? null : t.id)}
              >
                <td>
                  <span className="task-label">{t.label}</span>
                  <div className="dim task-cwd" title={t.cwd}>
                    {t.cwd}
                  </div>
                </td>
                <td className="num dim">{fmtTime(t.startedAt)}</td>
                <td className="num">
                  <StateBadge state={t.state} exitCode={t.exitCode} />
                </td>
                <td className="num">
                  <button
                    className="small ghost"
                    onClick={(e) => {
                      e.stopPropagation();
                      setOpenId(t.id === openId ? null : t.id);
                    }}
                  >
                    {t.id === openId ? "Chiudi" : "Log"}
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      {openTask && (
        <div className="task-open-log">
          <TaskLog key={openTask.id} task={openTask} />
        </div>
      )}
    </div>
  );
}
