import { useEffect, useState } from "react";
import { api, API_BASE, post } from "../../lib/api";
import { ToolsPanel } from "./ToolsPanel";
import type { LanInfo } from "../../lib/types";

export function Settings() {
  const [lan, setLan] = useState<LanInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api<LanInfo>("/api/lan").then((r) => {
      if (r.ok) setLan(r.data);
      else setError(r.error.message);
    });
  }, []);

  const toggleRemote = async () => {
    if (!lan) return;
    const r = await post<{ remoteControlEnabled: boolean }>(
      "/api/config/remote-control",
      { enabled: !lan.remoteControlEnabled },
    );
    if (r.ok) setLan({ ...lan, remoteControlEnabled: r.data.remoteControlEnabled });
  };

  return (
    <div className="settings">
      <h2>Impostazioni</h2>

      <section>
        <h3>Accesso da smartphone (LAN)</h3>
        {error && <div className="banner banner-error">{error}</div>}
        {!lan && !error && <div className="empty">Caricamento…</div>}
        {lan && (
          <div className="lan-panel">
            <div>
              <div className="lan-status">
                Stato:{" "}
                {lan.lanEnabled ? (
                  <span className="badge badge-ok">attivo su porta {lan.port}</span>
                ) : (
                  <span className="badge">solo localhost</span>
                )}
              </div>
              <ul className="lan-urls">
                {lan.urls.map((u) => (
                  <li key={u}>
                    <code>{u}</code>
                  </li>
                ))}
                {lan.urls.length === 0 && <li>Nessun IP LAN rilevato</li>}
              </ul>
              <p className="hint">
                Scansiona il QR dal telefono: contiene l'indirizzo e il token di
                abbinamento.
              </p>
              <label className="checkbox">
                <input
                  type="checkbox"
                  checked={lan.remoteControlEnabled}
                  onChange={toggleRemote}
                />
                Controllo remoto: consenti azioni (kill, run, git) dai device
                abbinati. Se spento, il telefono è in sola lettura.
              </label>
            </div>
            {lan.lanEnabled && lan.urls.length > 0 && (
              <img
                className="qr"
                src={`${API_BASE}/api/lan/qr.svg`}
                alt="QR di abbinamento"
                width={180}
                height={180}
              />
            )}
          </div>
        )}
      </section>

      <ToolsPanel />
    </div>
  );
}
