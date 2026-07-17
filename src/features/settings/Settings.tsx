import { useEffect, useState } from "react";
import { api, API_BASE, post } from "../../lib/api";
import { ToolsPanel } from "./ToolsPanel";
import { Toggle } from "../../components/Toggle";
import { applyTheme, getTheme, type Theme } from "../../lib/theme";
import type { LanInfo } from "../../lib/types";

const THEMES: { id: Theme; label: string }[] = [
  { id: "auto", label: "Auto (sistema)" },
  { id: "light", label: "Chiaro" },
  { id: "dark", label: "Scuro" },
];

export function Settings() {
  const [lan, setLan] = useState<LanInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showQr, setShowQr] = useState(false);
  const [theme, setTheme] = useState<Theme>(getTheme());

  useEffect(() => {
    api<LanInfo>("/api/lan").then((r) => {
      if (r.ok) setLan(r.data);
      else setError(r.error.message);
    });
  }, []);

  const chooseTheme = (t: Theme) => {
    setTheme(t);
    applyTheme(t);
  };

  const toggleRemote = async (enabled: boolean) => {
    if (!lan) return;
    const r = await post<{ remoteControlEnabled: boolean }>("/api/config/remote-control", {
      enabled,
    });
    if (r.ok) setLan({ ...lan, remoteControlEnabled: r.data.remoteControlEnabled });
  };

  const toggleAntiIdle = async (enabled: boolean) => {
    if (!lan) return;
    const r = await post<{ antiIdleEnabled: boolean }>("/api/config/anti-idle", { enabled });
    if (r.ok) setLan({ ...lan, antiIdleEnabled: r.data.antiIdleEnabled });
  };

  return (
    <div className="settings">
      <h2>Impostazioni</h2>

      <section>
        <h3>Aspetto</h3>
        <div className="segmented">
          {THEMES.map((t) => (
            <button
              key={t.id}
              className={theme === t.id ? "active" : ""}
              onClick={() => chooseTheme(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>
      </section>

      <section>
        <h3>Comportamento</h3>
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-title">Anti-inattività</div>
            <div className="hint">
              Dopo 5 minuti di inattività muove il mouse ogni 3 minuti, così lo schermo non si
              spegne e le chat non ti segnano assente. Se torni attivo, si ferma da solo.
              {" "}
              <em>Su macOS serve il permesso «Accessibilità» per RickyDEVTool.</em>
            </div>
          </div>
          <Toggle
            checked={lan?.antiIdleEnabled ?? false}
            onChange={toggleAntiIdle}
            disabled={!lan}
            label="Anti-inattività"
          />
        </div>
      </section>

      <section>
        <h3>Accesso da smartphone (LAN)</h3>
        {error && <div className="banner banner-error">{error}</div>}
        {!lan && !error && <div className="empty">Caricamento…</div>}
        {lan && (
          <>
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

            {lan.lanEnabled && lan.urls.length > 0 && (
              <div className="qr-block">
                <button onClick={() => setShowQr(!showQr)}>
                  {showQr ? "Nascondi QR" : "Mostra QR di abbinamento"}
                </button>
                {showQr && (
                  <div className="qr-panel">
                    <img
                      className="qr"
                      src={`${API_BASE}/api/lan/qr.svg`}
                      alt="QR di abbinamento"
                      width={180}
                      height={180}
                    />
                    <p className="hint">
                      Scansiona dal telefono: contiene indirizzo e token di abbinamento.
                    </p>
                  </div>
                )}
              </div>
            )}

            <div className="setting-row">
              <div className="setting-text">
                <div className="setting-title">Controllo remoto</div>
                <div className="hint">
                  Consenti azioni (kill, run, git) dai device abbinati. Se spento, il telefono è
                  in sola lettura. Espulsione e formattazione dischi restano sempre solo da questo
                  computer.
                </div>
              </div>
              <Toggle
                checked={lan.remoteControlEnabled}
                onChange={toggleRemote}
                label="Controllo remoto"
              />
            </div>
          </>
        )}
      </section>

      <ToolsPanel />
    </div>
  );
}
