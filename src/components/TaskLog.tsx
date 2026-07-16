import { useEffect, useRef, useState } from "react";
import { post } from "../lib/api";
import { ws } from "../lib/ws";
import type { TaskEvent, TaskInfo } from "../lib/types";

interface TaskLogProps {
  task: TaskInfo;
  onDone?: (ok: boolean) => void;
}

/** Pannello log di un task: stream stdout/stderr via WS + bottone Stop. */
export function TaskLog({ task, onDone }: TaskLogProps) {
  const [lines, setLines] = useState<{ stream: string; line: string }[]>([]);
  const [running, setRunning] = useState(task.state === "running");
  const [exitCode, setExitCode] = useState<number | null>(task.exitCode);
  const boxRef = useRef<HTMLPreElement>(null);
  const doneRef = useRef(onDone);
  doneRef.current = onDone;

  useEffect(() => {
    return ws.subscribe(`task:${task.id}`, (event) => {
      const payload = event.payload as TaskEvent;
      if (payload.event === "line") {
        setLines((prev) => [...prev.slice(-499), { stream: payload.stream, line: payload.line }]);
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
            Stop
          </button>
        ) : (
          <span className={`badge ${exitCode === 0 ? "badge-ok" : "badge-warn"}`}>
            {exitCode === 0 ? "completato" : `uscito con codice ${exitCode ?? "?"}`}
          </span>
        )}
      </div>
      <pre ref={boxRef} className="task-log-box">
        {lines.map((l, i) => (
          <div key={i} className={l.stream === "err" ? "log-err" : undefined}>
            {l.line}
          </div>
        ))}
        {lines.length === 0 && <span className="dim">in attesa di output…</span>}
      </pre>
    </div>
  );
}
