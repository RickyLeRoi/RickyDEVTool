import { useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { Toggle } from "../../components/Toggle";
import type { PushConfig } from "../../lib/types";

const SEVERITIES: PushConfig["minSeverity"][] = ["info", "warning", "critical"];
const SEVERITY_LABELS: Record<string, string> = {
  info: "Tutti",
  warning: "Warning+",
  critical: "Solo critici",
};

export function PushPanel() {
  const [cfg, setCfg] = useState<PushConfig | null>(null);
  const [server, setServer] = useState("");
  const [topic, setTopic] = useState("");
  const [testMsg, setTestMsg] = useState<string | null>(null);

  useEffect(() => {
    api<PushConfig>("/api/push").then((r) => {
      if (r.ok) {
        setCfg(r.data);
        setServer(r.data.server);
        setTopic(r.data.topic);
      }
    });
  }, []);

  const save = async (patch: Partial<PushConfig>) => {
    const r = await post<PushConfig>("/api/push", patch);
    if (r.ok) setCfg(r.data);
  };

  const test = async () => {
    setTestMsg("Invio…");
    const r = await post("/api/push/test", {});
    setTestMsg(r.ok ? "Inviata: controlla il telefono" : `Errore: ${r.error.message}`);
    setTimeout(() => setTestMsg(null), 5000);
  };

  if (!cfg) return null;

  return (
    <section>
      <h3>Notifiche push (ntfy)</h3>
      <div className="setting-row">
        <div className="setting-text">
          <div className="setting-title">Invia gli alert al telefono</div>
          <div className="hint">
            Usa <a href="https://ntfy.sh" target="_blank" rel="noreferrer">ntfy</a>: installa
            l'app, iscriviti al topic qui sotto e riceverai gli alert (CPU, servizi down,
            certificati) anche ad app chiusa. Scegli un topic difficile da indovinare.
          </div>
        </div>
        <Toggle checked={cfg.enabled} onChange={(enabled) => save({ enabled })} label="Push" />
      </div>

      <label className="form-row">
        <span>Server ntfy</span>
        <input
          value={server}
          onChange={(e) => setServer(e.target.value)}
          onBlur={() => save({ server })}
          placeholder="https://ntfy.sh"
        />
      </label>
      <label className="form-row">
        <span>Topic</span>
        <input
          value={topic}
          onChange={(e) => setTopic(e.target.value)}
          onBlur={() => save({ topic })}
          placeholder="rickydev-a8f3z"
        />
      </label>
      <label className="form-row">
        <span>Soglia minima</span>
        <div className="segmented">
          {SEVERITIES.map((s) => (
            <button
              key={s}
              className={cfg.minSeverity === s ? "active" : ""}
              onClick={() => save({ minSeverity: s })}
            >
              {SEVERITY_LABELS[s]}
            </button>
          ))}
        </div>
      </label>
      <div className="push-test">
        <button onClick={test} disabled={!cfg.enabled}>
          Invia notifica di prova
        </button>
        {testMsg && <span className="dim"> {testMsg}</span>}
      </div>
    </section>
  );
}
