import { useEffect, useState } from "react";
import { api, API_BASE } from "../../lib/api";
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
                abbinamento. Il telefono resta in sola lettura finché il
                controllo remoto non verrà abilitato (v1).
              </p>
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
    </div>
  );
}
