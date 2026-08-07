import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, post } from "../lib/api";
import { ws } from "../lib/ws";
import type { TaskEvent, TaskInfo } from "../lib/types";

interface TaskLogProps {
  task: TaskInfo;
  onDone?: (ok: boolean) => void;
}

export function TaskLog({ task, onDone }: TaskLogProps) {
  const { t } = useTranslation();
  const [lines, setLines] = useState<{ stream: string; line: string }[]>([]);
  const [running, setRunning] = useState(task.state === "running");
  const [exitCode, setExitCode] = useState<number | null>(task.exitCode);
  const boxRef = useRef<HTMLPreElement>(null);
  const doneRef = useRef(onDone);
  doneRef.current = onDone;

  useEffect(() => {
    let active = true;
    api<{ lines: { stream: "out" | "err"; line: string }[] }>(`/api/tasks/${task.id}/log`).then(
      (r) => {
        if (active && r.ok) setLines((prev) => (prev.length === 0 ? r.data.lines : prev));
      },
    );
    return () => {
      active = false;
    };
  }, [task.id]);

  useEffect(() => {
    return ws.subscribe(`task:${task.id}`, (event) => {
      const payload = event.payload as TaskEvent;
      if (payload.event === "line") {
        setLines((prev) => [...prev.slice(-4999), { stream: payload.stream, line: payload.line }]);
      } else if (payload.event === "exit") {
        setRunning(false);
        setExitCode(payload.exitCode);
        doneRef.current?.(payload.ok);
      }
    });
  }, [task.id]);

  useEffect(() => {
    const box = boxRef.current;
    if (box) box.scrollTop = box.scrollHeight;
  }, [lines]);

  return (
    <div className="task-log">
      <div className="task-log-header">
        <span className="dim" title={task.cwd}>
          {task.label}
        </span>
        {running ? (
          <button className="danger small" onClick={() => post(`/api/tasks/${task.id}/stop`, {})}>
            {t("taskLog.stop")}
          </button>
        ) : (
          <span className={`badge ${exitCode === 0 ? "badge-ok" : "badge-warn"}`}>
            {exitCode === 0
              ? t("taskLog.completed")
              : t("taskLog.exitedWithCode", { code: exitCode ?? "?" })}
          </span>
        )}
      </div>
      <pre ref={boxRef} className="task-log-box">
        {lines.map((l, i) => (
          <div key={i} className={l.stream === "err" ? "log-err" : undefined}>
            {l.line}
          </div>
        ))}
        {lines.length === 0 && <span className="dim">{t("taskLog.waitingOutput")}</span>}
      </pre>
    </div>
  );
}
