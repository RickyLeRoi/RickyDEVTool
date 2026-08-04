import { useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { Toggle } from "../../components/Toggle";
import type { AiStatus } from "../../lib/types";

const STRATEGIES: { id: string; label: string; hint: string }[] = [
  { id: "balanced", label: "Bilanciata", hint: "usa chi ha più quota residua" },
  { id: "fast", label: "Veloce", hint: "preferisce i provider a bassa latenza" },
  { id: "local", label: "Locale", hint: "non esce mai dalla macchina" },
];

interface Draft {
  port: string;
  command: string;
  envFile: string;
  strategy: string;
  systemPrompt: string;
}

function draftOf(status: AiStatus): Draft {
  return {
    port: String(status.configuredPort),
    command: status.command ?? "",
    envFile: status.envFile ?? "",
    strategy: status.strategy,
    systemPrompt: status.systemPrompt,
  };
}

/** Configurazione di RickyAI: quale `of-free` avviare, con quali chiavi e come
 *  instradare. Solo dal desktop (la rotta è nel gruppo local-only), quindi dal
 *  telefono i salvataggi tornano indietro con il motivo. */
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
      // Il draft si allinea solo al primo caricamento: un aggiornamento di
      // stato non deve cancellare quello che l'utente sta scrivendo.
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

  const dirty =
    draft.port !== String(status.configuredPort) ||
    draft.command !== (status.command ?? "") ||
    draft.envFile !== (status.envFile ?? "") ||
    draft.systemPrompt !== status.systemPrompt;

  return (
    <section className="ai-settings">
      <h3>RickyAI {saved && <span className="badge badge-ok">salvato</span>}</h3>
      {error && <div className="banner banner-error">{error}</div>}

      <div className="setting-row">
        <div className="setting-text">
          <div className="setting-title">
            Avvia of-free all'accensione <span className="badge badge-beta">beta</span>
          </div>
          <div className="hint">
            Spento, la sezione RickyAI non compare affatto. Acceso, il tool avvia{" "}
            <code>of-free</code> su <code>127.0.0.1</code> a ogni accensione — o adotta l'istanza
            che stai già usando da terminale.
          </div>
        </div>
        <Toggle
          checked={status.enabled}
          onChange={(enabled) => save({ enabled })}
          label="Avvio automatico di of-free"
        />
      </div>

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

      <label className="form-row">
        <span>File delle chiavi</span>
        <input
          value={draft.envFile}
          placeholder="vuoto = ~/.onfeather/.env"
          onChange={(e) => setDraft({ ...draft, envFile: e.target.value })}
        />
      </label>

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
            save({
              port: Number(draft.port) || status.configuredPort,
              command: draft.command,
              envFile: draft.envFile,
              systemPrompt: draft.systemPrompt,
            })
          }
        >
          {busy ? "Salvo…" : "Salva e riavvia"}
        </button>
      </div>

      <div className="hint">
        Le chiavi dei provider vanno in <code>~/.onfeather/.env</code>. Senza nessuna chiave
        restano i modelli locali via Ollama. Ogni salvataggio riavvia <code>of-free</code>.
      </div>
    </section>
  );
}
