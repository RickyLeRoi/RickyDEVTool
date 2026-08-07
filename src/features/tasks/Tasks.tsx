import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { ws } from "../../lib/ws";
import { getLang } from "../../lib/i18n";
import { TaskLog } from "../../components/TaskLog";
import type { TaskInfo } from "../../lib/types";

function fmtTime(ms: number) {
  return new Date(ms).toLocaleTimeString(getLang(), {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function StateBadge({ state, exitCode }: { state: TaskInfo["state"]; exitCode: number | null }) {
  const { t } = useTranslation();
  if (state === "running") return <span className="badge badge-branch">{t("tasks.running")}</span>;
  if (state === "exited" && exitCode === 0)
    return <span className="badge badge-ok">{t("tasks.completed")}</span>;
  return <span className="badge badge-warn">{t("tasks.exitedWith", { code: exitCode ?? "?" })}</span>;
}

export function Tasks() {
  const { t } = useTranslation();
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
      if (openId && !r.data.tasks.some((task) => task.id === openId)) setOpenId(null);
    }
  };

  const openTask = tasks?.find((task) => task.id === openId) ?? null;
  const hasFinished = (tasks ?? []).some((task) => task.state !== "running");

  return (
    <div>
      <div className="section-header">
        <h2>{t("nav.tasks")}</h2>
        {hasFinished && (
          <button className="small" onClick={clearFinished}>
            {t("tasks.clearFinished")}
          </button>
        )}
      </div>

      <p className="hint">{t("tasks.intro")}</p>

      {!tasks && <div className="empty">{t("tasks.loading")}</div>}
      {tasks && tasks.length === 0 && <div className="empty">{t("tasks.none")}</div>}

      {tasks && tasks.length > 0 && (
        <table className="proc-table task-table">
          <tbody>
            {tasks.map((task) => (
              <tr
                key={task.id}
                className={task.id === openId ? "row-editing" : "row-clickable"}
                onClick={() => setOpenId(task.id === openId ? null : task.id)}
              >
                <td>
                  <span className="task-label">{task.label}</span>
                  <div className="dim task-cwd" title={task.cwd}>
                    {task.cwd}
                  </div>
                </td>
                <td className="num dim">{fmtTime(task.startedAt)}</td>
                <td className="num">
                  <StateBadge state={task.state} exitCode={task.exitCode} />
                </td>
                <td className="num">
                  <button
                    className="small ghost"
                    onClick={(e) => {
                      e.stopPropagation();
                      setOpenId(task.id === openId ? null : task.id);
                    }}
                  >
                    {task.id === openId ? t("common.close") : t("tasks.logBtn")}
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
