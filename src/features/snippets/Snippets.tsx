import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { TaskLog } from "../../components/TaskLog";
import { EMPTY_SNIPPET_DRAFT } from "../../lib/defaults";
import type { ApiError, Snippet, TaskInfo } from "../../lib/types";

interface Draft {
  id?: string;
  name: string;
  command: string;
  cwd: string;
}

function SnippetForm({
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
        <span>{t("snippets.name")}</span>
        <input
          value={draft.name}
          onChange={(e) => onChange({ ...draft, name: e.target.value })}
          placeholder={t("snippets.namePlaceholder")}
        />
      </label>
      <label className="form-row form-row-col">
        <span>{t("snippets.command")}</span>
        <textarea
          value={draft.command}
          onChange={(e) => onChange({ ...draft, command: e.target.value })}
          placeholder={t("snippets.commandPlaceholder")}
          rows={2}
        />
      </label>
      <label className="form-row">
        <span title={t("snippets.folderTitle")}>{t("snippets.folder")}</span>
        <input
          value={draft.cwd}
          onChange={(e) => onChange({ ...draft, cwd: e.target.value })}
          placeholder={t("snippets.folderPlaceholder")}
        />
      </label>
      <div className="dialog-actions">
        <button onClick={onCancel}>{t("common.cancel")}</button>
        <button
          className="primary"
          onClick={onSave}
          disabled={busy || !draft.name.trim() || !draft.command.trim()}
        >
          {busy ? t("snippets.saving") : t("common.save")}
        </button>
      </div>
    </div>
  );
}

export function Snippets() {
  const { t } = useTranslation();
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [busy, setBusy] = useState(false);
  const [task, setTask] = useState<{ info: TaskInfo; name: string } | null>(null);

  const load = useCallback(async () => {
    const r = await api<{ snippets: Snippet[] }>("/api/snippets");
    if (r.ok) setSnippets(r.data.snippets ?? []);
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const save = async () => {
    if (!draft) return;
    setBusy(true);
    setError(null);
    const r = await post<{ snippets: Snippet[] }>("/api/snippets", draft);
    setBusy(false);
    if (r.ok) {
      setSnippets(r.data.snippets);
      setDraft(null);
    } else setError(r.error);
  };

  const remove = async (id: string) => {
    if (!confirm(t("snippets.deleteConfirm"))) return;
    const r = await post<{ snippets: Snippet[] }>("/api/snippets/delete", { id });
    if (r.ok) setSnippets(r.data.snippets);
  };

  const run = async (s: Snippet) => {
    setError(null);
    const r = await post<TaskInfo>("/api/snippets/run", { id: s.id });
    if (r.ok) setTask({ info: r.data, name: s.name });
    else setError(r.error);
  };

  return (
    <div className="snippets">
      <div className="section-header">
        <h2>{t("nav.snippets")}</h2>
        {!draft && (
          <button className="small" onClick={() => setDraft({ ...EMPTY_SNIPPET_DRAFT })}>
            {t("snippets.new")}
          </button>
        )}
      </div>
      <p className="hint">{t("snippets.intro")}</p>

      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}

      {draft && (
        <SnippetForm
          draft={draft}
          onChange={setDraft}
          onSave={save}
          onCancel={() => setDraft(null)}
          busy={busy}
        />
      )}

      {snippets.length === 0 && !draft && (
        <div className="empty">{t("snippets.empty")}</div>
      )}

      <div className="snippet-list">
        {snippets.map((s) => (
          <div key={s.id} className="snippet-card">
            <div className="snippet-info">
              <div className="snippet-name">{s.name}</div>
              <code className="snippet-cmd" title={s.command}>
                {s.command}
              </code>
              {s.cwd && <div className="dim snippet-cwd">📁 {s.cwd}</div>}
            </div>
            <div className="snippet-actions">
              <button className="primary small" onClick={() => run(s)}>
                {t("snippets.run")}
              </button>
              <button className="small" onClick={() => setDraft({ ...s })}>
                {t("common.edit")}
              </button>
              <button className="small danger" onClick={() => remove(s.id)}>
                {t("common.delete")}
              </button>
            </div>
          </div>
        ))}
      </div>

      {task && (
        <div className="snippet-output">
          <div className="section-header">
            <h3>{t("snippets.output", { name: task.name })}</h3>
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
