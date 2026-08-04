import { useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { Toggle } from "../../components/Toggle";
import type { AiMode, AiStatus } from "../../lib/types";

const STRATEGIES: { id: string; label: string; hint: string }[] = [
  { id: "balanced", label: "Bilanciata", hint: "usa chi ha più quota residua" },
  { id: "fast", label: "Veloce", hint: "preferisce i provider a bassa latenza" },
  { id: "local", label: "Locale", hint: "non esce mai dalla macchina" },
];

const MODES: { id: AiMode; label: string; hint: string }[] = [
  { id: "local", label: "Su questo computer", hint: "il tool avvia of-free in locale" },
  { id: "remote", label: "Servizio in rete", hint: "usa un of-free già in esecuzione altrove" },
];

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
        {isSet && <span className="badge badge-ok">impostata</span>}
      </span>
      <input
        type="password"
        value={value}
        placeholder={isSet ? "••••••••••••  (incolla per sostituire)" : "incolla la chiave"}
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
        Salva
      </button>
      <button className="small danger" onClick={onClear} disabled={busy || !isSet}>
        Rimuovi
      </button>
    </div>
  );
}

export function AiSettings() {
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
        <h3>RickyAI</h3>
        <div className="empty">Caricamento…</div>
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
      <h3>RickyAI {saved && <span className="badge badge-ok">salvato</span>}</h3>
      {error && <div className="banner banner-error">{error}</div>}

      <div className="setting-row">
        <div className="setting-text">
          <div className="setting-title">
            Abilita of-free <span className="badge badge-beta">beta</span>
          </div>
          <div className="hint">
            Spento, la sezione RickyAI non compare affatto. Acceso, la chat usa il motore scelto
            qui sotto.
          </div>
        </div>
        <Toggle
          checked={status.enabled}
          onChange={(enabled) => save({ enabled })}
          label="Abilita of-free"
        />
      </div>

      {status.enabled && (
        <>
          <div className="form-row">
            <span>Dove gira of-free</span>
            <div className="segmented">
              {MODES.map((m) => (
                <button
                  key={m.id}
                  className={status.mode === m.id ? "active" : ""}
                  title={m.hint}
                  disabled={busy}
                  onClick={() =>
                    save(
                      m.id === "remote"
                        ? { mode: m.id, remoteUrl: draft.remoteUrl }
                        : { mode: m.id },
                    )
                  }
                >
                  {m.label}
                </button>
              ))}
            </div>
          </div>

          {remote ? (
            <>
              <label className="form-row">
                <span>Indirizzo del servizio</span>
                <input
                  value={draft.remoteUrl}
                  placeholder="es. 192.168.1.50:4141"
                  onChange={(e) => setDraft({ ...draft, remoteUrl: e.target.value })}
                />
              </label>
              <div className="ai-keys">
                <KeyRow
                  label="Chiave API"
                  env="Authorization: Bearer"
                  isSet={status.remoteKeySet}
                  busy={busy}
                  onSave={(value) => save({ remoteKey: value })}
                  onClear={() => save({ remoteKey: "" })}
                />
                <div className="hint">
                  Serve solo se il servizio la richiede: of-free, Ollama e gli altri motori in LAN
                  di solito no, OpenRouter e le API hosted sì. Come le altre chiavi, dopo il
                  salvataggio non è più rileggibile da qui.
                </div>
              </div>
              <div className="hint">
                Va bene qualunque endpoint <strong>OpenAI-compatibile</strong>: un{" "}
                <code>of-free serve</code> (anche in Docker) su un altro computer, un Ollama, LM
                Studio, vLLM, o direttamente OpenRouter. Se è of-free trovi anche quote e routing
                fra provider; altrove si sceglie il modello dalla lista del servizio.
                <br />
                Un servizio in LAN deve essere in ascolto su <code>0.0.0.0</code> e non solo su{" "}
                <code>127.0.0.1</code>, altrimenti da qui non risponde — è il motivo per cui un
                container “non si vede”.
              </div>
            </>
          ) : (
            <>
              <div className="form-row">
                <span>Strategia di routing</span>
                <div className="segmented">
                  {STRATEGIES.map((s) => (
                    <button
                      key={s.id}
                      className={status.strategy === s.id ? "active" : ""}
                      title={s.hint}
                      disabled={busy}
                      onClick={() => save({ strategy: s.id })}
                    >
                      {s.label}
                    </button>
                  ))}
                </div>
              </div>

              <label className="form-row">
                <span>Porta</span>
                <input
                  type="number"
                  min={1024}
                  max={65535}
                  value={draft.port}
                  onChange={(e) => setDraft({ ...draft, port: e.target.value })}
                />
              </label>

              <label className="form-row">
                <span>Binario of-free</span>
                <input
                  value={draft.command}
                  placeholder="vuoto = cercato nel PATH"
                  onChange={(e) => setDraft({ ...draft, command: e.target.value })}
                />
              </label>

              <div className="ai-keys">
                <div className="form-row">
                  <span>Chiavi dei provider</span>
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
                <div className="hint">
                  Ogni chiave è facoltativa: of-free usa i provider che hai configurato e ripiega
                  su Ollama in locale. Una volta salvate non sono più rileggibili da qui — restano
                  nella config del tool (file leggibile solo dal tuo utente) e vengono passate a
                  of-free nel suo ambiente, mai su disco né sulla riga di comando.
                </div>
              </div>
            </>
          )}

          <label className="form-row">
            <span>Prompt di sistema</span>
            <textarea
              rows={2}
              value={draft.systemPrompt}
              placeholder="opzionale — come deve comportarsi RickyAI"
              onChange={(e) => setDraft({ ...draft, systemPrompt: e.target.value })}
            />
          </label>

          <div className="dialog-actions">
            <button onClick={() => setDraft(draftOf(status))} disabled={!dirty || busy}>
              Annulla
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
              {busy ? "Salvo…" : "Salva e riavvia"}
            </button>
          </div>
        </>
      )}
    </section>
  );
}
