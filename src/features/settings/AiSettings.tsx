import { useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { Toggle } from "../../components/Toggle";
import { LocalNetworkBanner } from "../../components/LocalNetworkBanner";
import type { AiMode, AiStatus } from "../../lib/types";

const STRATEGY_IDS = ["balanced", "fast", "local"] as const;
const MODE_IDS: AiMode[] = ["local", "remote"];

interface Draft {
  remoteUrl: string;
  port: string;
  command: string;
  systemPrompt: string;
}

function draftOf(status: AiStatus): Draft {
  return {
    remoteUrl: status.remoteUrl ?? "",
    port: String(status.configuredPort),
    command: status.command ?? "",
    systemPrompt: status.systemPrompt,
  };
}

function KeyRow({
  label,
  env,
  isSet,
  busy,
  onSave,
  onClear,
}: {
  label: string;
  env: string;
  isSet: boolean;
  busy: boolean;
  onSave: (value: string) => void;
  onClear: () => void;
}) {
  const { t } = useTranslation();
  const [value, setValue] = useState("");

  const save = () => {
    if (!value.trim()) return;
    onSave(value.trim());
    setValue("");
  };

  return (
    <div className="ai-key-row">
      <span className="ai-key-label" title={env}>
        {label}
        {isSet && <span className="badge badge-ok">{t("ai.keySet")}</span>}
      </span>
      <input
        type="password"
        value={value}
        placeholder={isSet ? t("ai.keyPlaceholderSet") : t("ai.keyPlaceholderUnset")}
        autoComplete="off"
        spellCheck={false}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            save();
          }
        }}
      />
      <button className="small" onClick={save} disabled={busy || !value.trim()}>
        {t("common.save")}
      </button>
      <button className="small danger" onClick={onClear} disabled={busy || !isSet}>
        {t("common.remove")}
      </button>
    </div>
  );
}

export function AiSettings() {
  const { t } = useTranslation();
  const [status, setStatus] = useState<AiStatus | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);

  const load = async () => {
    const r = await api<AiStatus>("/api/ai/status");
    if (r.ok && typeof r.data.state === "string") {
      setStatus(r.data);
      setDraft((prev) => prev ?? draftOf(r.data));
    }
  };

  useEffect(() => {
    load();
  }, []);

  const save = async (patch: Record<string, unknown>) => {
    setBusy(true);
    setError(null);
    const r = await post<AiStatus>("/api/ai/config", patch);
    setBusy(false);
    if (r.ok) {
      setStatus(r.data);
      setDraft(draftOf(r.data));
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
    } else {
      setError(r.error.message);
    }
  };

  if (!status || !draft) {
    return (
      <section>
        <h3>{t("ai.title")}</h3>
        <div className="empty">{t("common.loading")}</div>
      </section>
    );
  }

  const remote = status.mode === "remote";
  const dirty = remote
    ? draft.remoteUrl !== (status.remoteUrl ?? "") ||
      draft.systemPrompt !== status.systemPrompt
    : draft.port !== String(status.configuredPort) ||
      draft.command !== (status.command ?? "") ||
      draft.systemPrompt !== status.systemPrompt;

  return (
    <section className="ai-settings">
      <h3>
        {t("ai.title")} {saved && <span className="badge badge-ok">{t("ai.saved")}</span>}
      </h3>

      <div className="setting-row">
        <div className="setting-text">
          <div className="setting-title">
            {t("ai.enable")} <span className="badge badge-beta">{t("ai.beta")}</span>
          </div>
          <div className="hint">
            <Trans i18nKey="ai.enableHint" components={{ b: <strong /> }} />
          </div>
        </div>
        <Toggle
          checked={status.enabled}
          onChange={(enabled) => save({ enabled })}
          label={t("ai.enableLabel")}
        />
      </div>

      {error && <div className="banner banner-error">{error}</div>}

      {status.enabled && (
        <>
          <div className="form-row">
            <span>{t("ai.whereOfFree")}</span>
            <div className="segmented">
              {MODE_IDS.map((id) => (
                <button
                  key={id}
                  className={status.mode === id ? "active" : ""}
                  title={t(`ai.mode${id === "local" ? "Local" : "Remote"}Hint` as const)}
                  disabled={busy}
                  onClick={() => save({ mode: id })}
                >
                  {t(`ai.mode${id === "local" ? "Local" : "Remote"}` as const)}
                </button>
              ))}
            </div>
          </div>

          {remote ? (
            <>
              <LocalNetworkBanner what="RickyAI" />
              <label className="form-row">
                <span>{t("ai.serviceAddress")}</span>
                <input
                  value={draft.remoteUrl}
                  placeholder={t("ai.serviceAddressPlaceholder")}
                  onChange={(e) => setDraft({ ...draft, remoteUrl: e.target.value })}
                />
              </label>
              <div className="ai-keys">
                <KeyRow
                  label={t("ai.apiKey")}
                  env="Authorization: Bearer"
                  isSet={status.remoteKeySet}
                  busy={busy}
                  onSave={(value) => save({ remoteKey: value })}
                  onClear={() => save({ remoteKey: "" })}
                />
                <div className="hint">{t("ai.apiKeyHint")}</div>
              </div>
              <div className="hint">
                <Trans i18nKey="ai.remoteHelp1" components={{ b: <strong />, code: <code /> }} />
                <br />
                <Trans i18nKey="ai.remoteHelp2" components={{ code: <code /> }} />
              </div>
            </>
          ) : (
            <>
              <div className="form-row">
                <span>{t("ai.strategy")}</span>
                <div className="segmented">
                  {STRATEGY_IDS.map((id) => (
                    <button
                      key={id}
                      className={status.strategy === id ? "active" : ""}
                      title={t(
                        `ai.strategy${id[0].toUpperCase()}${id.slice(1)}Hint` as
                          | "ai.strategyBalancedHint"
                          | "ai.strategyFastHint"
                          | "ai.strategyLocalHint",
                      )}
                      disabled={busy}
                      onClick={() => save({ strategy: id })}
                    >
                      {t(
                        `ai.strategy${id[0].toUpperCase()}${id.slice(1)}` as
                          | "ai.strategyBalanced"
                          | "ai.strategyFast"
                          | "ai.strategyLocal",
                      )}
                    </button>
                  ))}
                </div>
              </div>

              <label className="form-row">
                <span>{t("ai.port")}</span>
                <input
                  type="number"
                  min={1024}
                  max={65535}
                  value={draft.port}
                  onChange={(e) => setDraft({ ...draft, port: e.target.value })}
                />
              </label>

              <label className="form-row">
                <span>{t("ai.ofFreeBinary")}</span>
                <input
                  value={draft.command}
                  placeholder={t("ai.ofFreeBinaryPlaceholder")}
                  onChange={(e) => setDraft({ ...draft, command: e.target.value })}
                />
              </label>

              <div className="ai-keys">
                <div className="form-row">
                  <span>{t("ai.providerKeys")}</span>
                </div>
                {status.providerKeys.map((p) => (
                  <KeyRow
                    key={p.env}
                    label={p.label}
                    env={p.env}
                    isSet={status.keysSet.includes(p.env)}
                    busy={busy}
                    onSave={(value) => save({ keys: { [p.env]: value } })}
                    onClear={() => save({ keys: { [p.env]: "" } })}
                  />
                ))}
                <div className="hint">{t("ai.providerKeysHint")}</div>
              </div>
            </>
          )}

          <label className="form-row">
            <span>{t("ai.systemPrompt")}</span>
            <textarea
              rows={2}
              value={draft.systemPrompt}
              placeholder={t("ai.systemPromptPlaceholder")}
              onChange={(e) => setDraft({ ...draft, systemPrompt: e.target.value })}
            />
          </label>

          <div className="dialog-actions">
            <button onClick={() => setDraft(draftOf(status))} disabled={!dirty || busy}>
              {t("common.cancel")}
            </button>
            <button
              className="primary"
              disabled={!dirty || busy}
              onClick={() =>
                save(
                  remote
                    ? { remoteUrl: draft.remoteUrl, systemPrompt: draft.systemPrompt }
                    : {
                        port: Number(draft.port) || status.configuredPort,
                        command: draft.command,
                        systemPrompt: draft.systemPrompt,
                      },
                )
              }
            >
              {busy ? t("ai.saving") : t("ai.saveRestart")}
            </button>
          </div>
        </>
      )}
    </section>
  );
}
