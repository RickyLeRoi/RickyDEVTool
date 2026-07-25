import { useEffect, useState } from "react";
import { ws } from "../../lib/ws";
import type { SensorsSnapshot } from "../../lib/types";

function Bar({ pct, warn }: { pct: number; warn?: boolean }) {
  return (
    <div className="sensor-bar">
      <div
        className={`sensor-bar-fill ${warn ? "warn" : ""}`}
        style={{ width: `${Math.min(Math.max(pct, 0), 100)}%` }}
      />
    </div>
  );
}

/**
 * Temperature, batteria e GPU. Sottoscrive il topic "sensors" (poller attivo
 * solo mentre la dashboard è montata). Best-effort: mostra solo ciò che la
 * piattaforma espone e resta silenzioso quando non c'è nulla di utile.
 */
export function SensorsPanel() {
  const [s, setS] = useState<SensorsSnapshot | null>(null);

  useEffect(() => {
    return ws.subscribe("sensors", (event) => {
      setS(event.payload as SensorsSnapshot);
    });
  }, []);

  if (!s) return null;

  const hasTemp = s.temps.length > 0;
  const hasBattery = !!s.battery;
  const hasGpu = s.gpus.length > 0;
  if (!hasTemp && !hasBattery && !hasGpu) {
    return (
      <div className="sensors">
        <h3>Sensori</h3>
        <div className="dim">
          Nessun sensore esposto da questa piattaforma (comune su Apple Silicon senza permessi
          elevati e sui desktop senza batteria).
        </div>
      </div>
    );
  }

  return (
    <div className="sensors">
      <h3>Sensori</h3>
      <div className="sensor-grid">
        {hasBattery && s.battery && (
          <div className="sensor-card">
            <div className="sensor-head">
              <span>{s.battery.charging ? "🔌" : "🔋"} Batteria</span>
              <span className="sensor-value">{s.battery.percent.toFixed(0)}%</span>
            </div>
            <Bar pct={s.battery.percent} warn={!s.battery.charging && s.battery.percent <= 20} />
            <div className="dim">{s.battery.state}</div>
          </div>
        )}

        {hasTemp && (
          <div className="sensor-card">
            <div className="sensor-head">
              <span>🌡 Temperatura</span>
              {s.maxTempC != null && (
                <span className="sensor-value">{s.maxTempC.toFixed(0)}°C</span>
              )}
            </div>
            <div className="sensor-list">
              {s.temps.slice(0, 5).map((t) => (
                <div key={t.label} className="sensor-row">
                  <span className="dim sensor-row-label" title={t.label}>
                    {t.label}
                  </span>
                  <span>{t.celsius.toFixed(0)}°C</span>
                </div>
              ))}
            </div>
          </div>
        )}

        {s.gpus.map((g, i) => (
          <div key={`${g.name}-${i}`} className="sensor-card">
            <div className="sensor-head">
              <span title={`fonte: ${g.source}`}>🎮 {g.name}</span>
              {g.utilizationPct != null && (
                <span className="sensor-value">{g.utilizationPct.toFixed(0)}%</span>
              )}
            </div>
            {g.utilizationPct != null ? (
              <Bar pct={g.utilizationPct} />
            ) : (
              <div className="dim">Utilizzo live non disponibile su questa piattaforma</div>
            )}
            <div className="dim sensor-gpu-sub">
              {g.memUsedMb != null && g.memTotalMb != null && (
                <span>
                  VRAM {(g.memUsedMb / 1024).toFixed(1)}/{(g.memTotalMb / 1024).toFixed(1)} GB
                </span>
              )}
              {g.memUsedMb == null && g.memTotalMb != null && (
                <span>VRAM {(g.memTotalMb / 1024).toFixed(1)} GB</span>
              )}
              {g.tempC != null && <span>{g.tempC.toFixed(0)}°C</span>}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
