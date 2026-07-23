import { useCallback, useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { TaskLog } from "../../components/TaskLog";
import type { ApiError, LaunchBundle, LaunchStep, TaskInfo } from "../../lib/types";

const emptyStep = (): LaunchStep => ({ label: "", command: "", cwd: "" });

function BundleEditor({
  initial,
  onSaved,
  onCancel,
}: {
  initial: LaunchBundle | null;
  onSaved: (bundles: LaunchBundle[]) => void;
  onCancel: () => void;
}) {
  const [name, setName] = useState(initial?.name ?? "");
  const [steps, setSteps] = useState<LaunchStep[]>(
    initial ? initial.steps.map((s) => ({ ...s })) : [emptyStep()],
  );
  const [error, setError] = useState<ApiError | null>(null);
  const [busy, setBusy] = useState(false);

  const setStep = (i: number, patch: Partial<LaunchStep>) =>
    setSteps((ss) => ss.map((s, j) => (j === i ? { ...s, ...patch } : s)));

  const save = async () => {
    setBusy(true);
    setError(null);
    const r = await post<{ bundles: LaunchBundle[] }>("/api/launch/bundles", {
      id: initial?.id,
      name,
      steps,
    });
    setBusy(false);
    if (r.ok) onSaved(r.data.bundles);
    else setError(r.error);
  };

  return (
    <div className="launch-editor">
      <label className="form-row">
        <span>Nome profilo</span>
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="es. Stack completo"
          autoFocus
        />
      </label>

      <div className="launch-steps">
        {steps.map((s, i) => (
          <div key={i} className="launch-step-edit">
            <div className="launch-step-head">
              <span className="dim">Step {i + 1}</span>
              {steps.length > 1 && (
                <button
                  className="small ghost"
                  onClick={() => setSteps((ss) => ss.filter((_, j) => j !== i))}
                >
                  rimuovi
                </button>
              )}
            </div>
            <input
              className="launch-cmd"
              value={s.command}
              onChange={(e) => setStep(i, { command: e.target.value })}
              placeholder="comando shell — es. npm run dev"
              spellCheck={false}
            />
            <div className="launch-step-row">
              <input
                value={s.label}
                onChange={(e) => setStep(i, { label: e.target.value })}
                placeholder="etichetta (opzionale)"
              />
              <input
                className="launch-cwd"
                value={s.cwd}
                onChange={(e) => setStep(i, { cwd: e.target.value })}
                placeholder="cartella di lavoro (path assoluto)"
                spellCheck={false}
              />
            </div>
          </div>
        ))}
        <button className="small ghost" onClick={() => setSteps((ss) => [...ss, emptyStep()])}>
          + aggiungi step
        </button>
      </div>

      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}

      <div className="dialog-actions">
        <button onClick={onCancel} disabled={busy}>
          Annulla
        </button>
        <button className="primary" onClick={save} disabled={busy || !name.trim()}>
          {busy ? "Salvo…" : "Salva profilo"}
        </button>
      </div>
    </div>
  );
}

export function Launch() {
  const [bundles, setBundles] = useState<LaunchBundle[] | null>(null);
  const [editing, setEditing] = useState<LaunchBundle | null | "new">(null);
  const [runError, setRunError] = useState<string | null>(null);
  const [runTasks, setRunTasks] = useState<{ bundle: string; tasks: TaskInfo[] } | null>(null);

  const load = useCallback(async () => {
    const r = await api<{ bundles: LaunchBundle[] }>("/api/launch/bundles");
    if (r.ok) setBundles(r.data.bundles);
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const run = async (b: LaunchBundle) => {
    setRunError(null);
    const r = await post<{ tasks: TaskInfo[]; errors: string[] }>("/api/launch/run", { id: b.id });
    if (r.ok) {
      setRunTasks({ bundle: b.name, tasks: r.data.tasks });
      if (r.data.errors.length > 0) setRunError(r.data.errors.join(" · "));
    } else setRunError(r.error.message);
  };

  const del = async (b: LaunchBundle) => {
    if (!window.confirm(`Eliminare il profilo «${b.name}»?`)) return;
    const r = await post<{ bundles: LaunchBundle[] }>("/api/launch/bundles/delete", { id: b.id });
    if (r.ok) setBundles(r.data.bundles);
  };

  return (
    <div>
      <div className="section-header">
        <h2>Avvii compositi</h2>
        {editing === null && <button onClick={() => setEditing("new")}>Nuovo profilo</button>}
      </div>

      <p className="hint">
        Un profilo lancia più comandi insieme (es. backend + frontend + docker), ciascuno come task
        nella sezione Task. I comandi girano nella shell di sistema.
      </p>

      {editing !== null ? (
        <BundleEditor
          initial={editing === "new" ? null : editing}
          onCancel={() => setEditing(null)}
          onSaved={(bs) => {
            setBundles(bs);
            setEditing(null);
          }}
        />
      ) : (
        <>
          {bundles && bundles.length === 0 && (
            <div className="empty">Nessun profilo: crea il primo con "Nuovo profilo".</div>
          )}
          {bundles && bundles.length > 0 && (
            <div className="launch-list">
              {bundles.map((b) => (
                <div key={b.id} className="launch-card">
                  <div className="launch-card-head">
                    <strong>{b.name}</strong>
                    <span className="launch-card-actions">
                      <button className="small primary" onClick={() => run(b)}>
                        ▶ Avvia ({b.steps.length})
                      </button>
                      <button className="small" onClick={() => setEditing(b)}>
                        Modifica
                      </button>
                      <button className="small danger" onClick={() => del(b)}>
                        Elimina
                      </button>
                    </span>
                  </div>
                  <ul className="launch-card-steps">
                    {b.steps.map((s, i) => (
                      <li key={i}>
                        <span className="launch-step-label">{s.label || "step"}</span>
                        <code>{s.command}</code>
                        <span className="dim launch-step-cwd">{s.cwd}</span>
                      </li>
                    ))}
                  </ul>
                </div>
              ))}
            </div>
          )}

          {runError && <div className="banner banner-error">{runError}</div>}

          {runTasks && (
            <div className="launch-run">
              <div className="section-header">
                <h3>Avviato: {runTasks.bundle}</h3>
                <button className="small" onClick={() => setRunTasks(null)}>
                  Chiudi
                </button>
              </div>
              {runTasks.tasks.length === 0 && <div className="dim">Nessun task avviato.</div>}
              {runTasks.tasks.map((t) => (
                <div key={t.id} className="launch-run-task">
                  <div className="dim">{t.label}</div>
                  <TaskLog key={t.id} task={t} />
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}
