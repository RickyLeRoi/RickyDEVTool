import { useCallback, useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { TaskLog } from "../../components/TaskLog";
import type { ApiError, SshHost, TaskInfo } from "../../lib/types";

interface Draft {
  id?: string;
  name: string;
  host: string;
  defaultCommand: string;
}

const EMPTY: Draft = { name: "", host: "", defaultCommand: "uptime" };

// Comandi rapidi proposti come pillole sotto ogni host.
const PRESETS = ["uptime", "df -h", "free -h", "docker ps", "systemctl --failed"];

function HostForm({
  draft,
  onChange,
  onSave,
  onCancel,
  busy,
}: {
  draft: Draft;
  onChange: (d: Draft) => void;
  onSave: () => void;
  onCancel: () => void;
  busy: boolean;
}) {
  return (
    <div className="snippet-form">
      <label className="form-row">
        <span>Nome</span>
        <input
          value={draft.name}
          onChange={(e) => onChange({ ...draft, name: e.target.value })}
          placeholder="es. Homelab"
        />
      </label>
      <label className="form-row">
        <span>Host</span>
        <input
          value={draft.host}
          onChange={(e) => onChange({ ...draft, host: e.target.value })}
          placeholder="user@host, host o alias ssh"
        />
      </label>
      <label className="form-row">
        <span title="Proposto nel campo comando">Comando iniziale</span>
        <input
          value={draft.defaultCommand}
          onChange={(e) => onChange({ ...draft, defaultCommand: e.target.value })}
          placeholder="opzionale — es. uptime"
        />
      </label>
      <div className="dialog-actions">
        <button onClick={onCancel}>Annulla</button>
        <button
          className="primary"
          onClick={onSave}
          disabled={busy || !draft.name.trim() || !draft.host.trim()}
        >
          {busy ? "Salvo…" : "Salva"}
        </button>
      </div>
    </div>
  );
}

function HostCard({
  host,
  onEdit,
  onDelete,
  onRun,
}: {
  host: SshHost;
  onEdit: () => void;
  onDelete: () => void;
  onRun: (command: string) => void;
}) {
  const [command, setCommand] = useState(host.defaultCommand || "uptime");

  return (
    <div className="snippet-card ssh-card">
      <div className="snippet-info">
        <div className="snippet-name">
          {host.name} <span className="dim">· {host.host}</span>
        </div>
        <form
          className="net-form ssh-run-form"
          onSubmit={(e) => {
            e.preventDefault();
            onRun(command);
          }}
        >
          <input
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            placeholder="comando da eseguire via ssh"
          />
          <button className="primary" disabled={!command.trim()}>
            ▶ Esegui
          </button>
        </form>
        <div className="ssh-presets">
          {PRESETS.map((p) => (
            <button key={p} className="small ghost" onClick={() => setCommand(p)}>
              {p}
            </button>
          ))}
        </div>
      </div>
      <div className="snippet-actions">
        <button className="small" onClick={onEdit}>
          Modifica
        </button>
        <button className="small danger" onClick={onDelete}>
          Elimina
        </button>
      </div>
    </div>
  );
}

export function Ssh() {
  const [hosts, setHosts] = useState<SshHost[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [busy, setBusy] = useState(false);
  const [task, setTask] = useState<{ info: TaskInfo; name: string } | null>(null);

  const load = useCallback(async () => {
    const r = await api<{ hosts: SshHost[] }>("/api/ssh/hosts");
    if (r.ok) setHosts(r.data.hosts ?? []);
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const save = async () => {
    if (!draft) return;
    setBusy(true);
    setError(null);
    const r = await post<{ hosts: SshHost[] }>("/api/ssh/hosts", draft);
    setBusy(false);
    if (r.ok) {
      setHosts(r.data.hosts);
      setDraft(null);
    } else setError(r.error);
  };

  const remove = async (id: string) => {
    if (!confirm("Eliminare questo host?")) return;
    const r = await post<{ hosts: SshHost[] }>("/api/ssh/hosts/delete", { id });
    if (r.ok) setHosts(r.data.hosts);
  };

  const run = async (host: SshHost, command: string) => {
    setError(null);
    const r = await post<TaskInfo>("/api/ssh/run", { id: host.id, command });
    if (r.ok) setTask({ info: r.data, name: `${host.name}: ${command}` });
    else setError(r.error);
  };

  return (
    <div className="ssh">
      <div className="section-header">
        <h2>SSH</h2>
        {!draft && (
          <button className="small" onClick={() => setDraft({ ...EMPTY })}>
            + Nuovo host
          </button>
        )}
      </div>
      <p className="hint">
        Esegue comandi non interattivi sugli host salvati (uptime, df, restart di servizi, docker
        ps…), con output in streaming. Serve accesso <strong>a chiave senza password</strong>
        (BatchMode), come per il Docker remoto <code>ssh://</code>.
      </p>

      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}

      {draft && (
        <HostForm
          draft={draft}
          onChange={setDraft}
          onSave={save}
          onCancel={() => setDraft(null)}
          busy={busy}
        />
      )}

      {hosts.length === 0 && !draft && (
        <div className="empty">Nessun host. Aggiungine uno con “+ Nuovo host”.</div>
      )}

      <div className="snippet-list">
        {hosts.map((h) => (
          <HostCard
            key={h.id}
            host={h}
            onEdit={() => setDraft({ ...h })}
            onDelete={() => remove(h.id)}
            onRun={(command) => run(h, command)}
          />
        ))}
      </div>

      {task && (
        <div className="snippet-output">
          <div className="section-header">
            <h3>Output · {task.name}</h3>
            <button className="small" onClick={() => setTask(null)}>
              Chiudi
            </button>
          </div>
          <TaskLog key={task.info.id} task={task.info} />
        </div>
      )}
    </div>
  );
}
