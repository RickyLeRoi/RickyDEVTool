import { useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { Toggle } from "../../components/Toggle";
import type { AlertThresholds } from "../../lib/types";

/** Soglie configurabili degli alert (CPU/RAM/temperatura/batteria). */
export function AlertSettings() {
  const [t, setT] = useState<AlertThresholds | null>(null);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api<AlertThresholds>("/api/alerts/config").then((r) => {
      if (r.ok && typeof r.data.cpuPct === "number") setT(r.data);
    });
  }, []);

  const save = async (next: AlertThresholds) => {
    const prev = t;
    setT(next);
    setError(null);
    const r = await post<AlertThresholds>("/api/alerts/config", next);
    if (r.ok) {
      setT(r.data);
      setSaved(true);
      setTimeout(() => setSaved(false), 1200);
    } else {
      // Salvataggio rifiutato (es. dal telefono senza controllo remoto): ripristina
      // il valore reale e spiega perché, invece di mostrare una soglia mai salvata.
      if (prev) setT(prev);
      setError(r.error.message);
    }
  };

  return (
    <section>
      <h3>Alert {saved && <span className="badge badge-ok">salvato</span>}</h3>
      {error && <div className="banner banner-error">{error}</div>}
      {!t && <div className="empty">Caricamento…</div>}
      {t && (
        <div className="alert-settings">
          <label className="form-row alert-field">
            <span>CPU sostenuta oltre</span>
            <input
              type="number"
              min={10}
              max={100}
              value={t.cpuPct}
              onChange={(e) => save({ ...t, cpuPct: Number(e.target.value) })}
            />
            <span className="dim">% per &gt;60s</span>
          </label>

          <label className="form-row alert-field">
            <span>RAM oltre</span>
            <input
              type="number"
              min={10}
              max={100}
              value={t.memPct}
              onChange={(e) => save({ ...t, memPct: Number(e.target.value) })}
            />
            <span className="dim">%</span>
          </label>

          <div className="setting-row">
            <div className="setting-text">
              <div className="setting-title">Temperatura alta</div>
              <div className="hint">
                Avvisa quando un sensore supera la soglia (solo dove la piattaforma espone le
                temperature).
              </div>
            </div>
            <Toggle
              checked={t.tempEnabled}
              onChange={(v) => save({ ...t, tempEnabled: v })}
              label="Alert temperatura"
            />
          </div>
          {t.tempEnabled && (
            <label className="form-row alert-field">
              <span>Soglia temperatura</span>
              <input
                type="number"
                min={30}
                max={120}
                value={t.tempC}
                onChange={(e) => save({ ...t, tempC: Number(e.target.value) })}
              />
              <span className="dim">°C</span>
            </label>
          )}

          <div className="setting-row">
            <div className="setting-text">
              <div className="setting-title">Batteria scarica</div>
              <div className="hint">Avvisa quando la batteria scende sotto la soglia e non è in carica.</div>
            </div>
            <Toggle
              checked={t.batteryEnabled}
              onChange={(v) => save({ ...t, batteryEnabled: v })}
              label="Alert batteria"
            />
          </div>
          {t.batteryEnabled && (
            <label className="form-row alert-field">
              <span>Soglia batteria</span>
              <input
                type="number"
                min={1}
                max={100}
                value={t.batteryPct}
                onChange={(e) => save({ ...t, batteryPct: Number(e.target.value) })}
              />
              <span className="dim">%</span>
            </label>
          )}
        </div>
      )}
    </section>
  );
}
