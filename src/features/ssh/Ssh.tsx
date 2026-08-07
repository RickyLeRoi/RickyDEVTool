import { useCallback, useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { TaskLog } from "../../components/TaskLog";
import { SSH_COMMAND_PRESETS } from "../../lib/constants";
import { EMPTY_SSH_DRAFT } from "../../lib/defaults";
import type { ApiError, SshHost, TaskInfo } from "../../lib/types";

interface Draft {
  id?: string;
  name: string;
  host: string;
  defaultCommand: string;
}

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
  const { t } = useTranslation();
  return (
    <div className="snippet-form">
      <label className="form-row">
        <span>{t("ssh.name")}</span>
        <input
          value={draft.name}
          onChange={(e) => onChange({ ...draft, name: e.target.value })}
          placeholder={t("ssh.namePlaceholder")}
        />
      </label>
      <label className="form-row">
        <span>{t("ssh.host")}</span>
        <input
          value={draft.host}
          onChange={(e) => onChange({ ...draft, host: e.target.value })}
          placeholder={t("ssh.hostPlaceholder")}
        />
      </label>
      <label className="form-row">
        <span title={t("ssh.initialCommandTitle")}>{t("ssh.initialCommand")}</span>
        <input
          value={draft.defaultCommand}
          onChange={(e) => onChange({ ...draft, defaultCommand: e.target.value })}
          placeholder={t("ssh.initialCommandPlaceholder")}
        />
      </label>
      <div className="dialog-actions">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button
          className="primary"
          onClick={onSave}
          disabled={busy || !draft.name.trim() || !draft.host.trim()}
        >
          {busy ? t("ssh.saving") : t("common.save")}
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
  const { t } = useTranslation();
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
            placeholder={t("ssh.commandPlaceholder")}
          />
          <button className="primary" disabled={!command.trim()}>
            {t("ssh.run")}
          </button>
        </form>
        <div className="ssh-presets">
          {SSH_COMMAND_PRESETS.map((p) => (
            <button key={p} className="small ghost" onClick={() => setCommand(p)}>
              {p}
            </button>
          ))}
        </div>
      </div>
      <div className="snippet-actions">
        <button className="small" onClick={onEdit}>
          {t("common.edit")}
        </button>
        <button className="small danger" onClick={onDelete}>
          {t("common.delete")}
        </button>
      </div>
    </div>
  );
}

export function Ssh() {
  const { t } = useTranslation();
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
    if (!confirm(t("ssh.deleteConfirm"))) return;
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
        <h2>{t("nav.ssh")}</h2>
        {!draft && (
          <button className="small" onClick={() => setDraft({ ...EMPTY_SSH_DRAFT })}>
            {t("ssh.newHost")}
          </button>
        )}
      </div>
      <p className="hint">
        <Trans i18nKey="ssh.intro" components={{ b: <strong />, code: <code /> }} />
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
        <div className="empty">{t("ssh.empty")}</div>
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
            <h3>{t("ssh.output", { name: task.name })}</h3>
            <button className="small" onClick={() => setTask(null)}>
              {t("common.close")}
            </button>
          </div>
          <TaskLog key={task.info.id} task={task.info} />
        </div>
      )}
    </div>
  );
}
