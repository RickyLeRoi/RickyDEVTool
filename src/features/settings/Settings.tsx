import { useEffect, useState } from "react";
import { api, API_BASE, post } from "../../lib/api";
import { ToolsPanel } from "./ToolsPanel";
import { Toggle } from "../../components/Toggle";
import { applyTheme, getTheme, type Theme } from "../../lib/theme";
import { useTrayIntentStore } from "../../stores/trayIntentStore";
import type { AccessibilityStatus, LanInfo } from "../../lib/types";

const THEMES: { id: Theme; label: string }[] = [
  { id: "auto", label: "Auto (sistema)" },
  { id: "light", label: "Chiaro" },
  { id: "dark", label: "Scuro" },
];

const isTauri = "__TAURI_INTERNALS__" in window;

export function Settings() {
  const [lan, setLan] = useState<LanInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showQr, setShowQr] = useState(false);
  const [theme, setTheme] = useState<Theme>(getTheme());
  const [access, setAccess] = useState<AccessibilityStatus | null>(null);
  // true dopo un "Ricontrolla" che trova il permesso ancora non concesso: su
  // macOS spesso serve riavviare l'app perché il processo veda il cambio.
  const [recheckedStillOff, setRecheckedStillOff] = useState(false);

  useEffect(() => {
    api<LanInfo>("/api/lan").then((r) => {
      if (r.ok) setLan(r.data);
      else setError(r.error.message);
    });
    refreshAccess();
  }, []);

  // "Mostra QR di abbinamento" dal menu del tray.
  const traySeq = useTrayIntentStore((s) => s.seq);
  useEffect(() => {
    const { section, extra } = useTrayIntentStore.getState();
    if (section === "settings" && extra === "qr") setShowQr(true);
  }, [traySeq]);

  const refreshAccess = () =>
    api<AccessibilityStatus>("/api/system/accessibility").then((r) => {
      if (r.ok) setAccess(r.data);
      return r.ok ? r.data : null;
    });

  // Ricontrolla su richiesta esplicita: se ancora spento, mostra l'avviso "riavvia".
  const recheckAccess = async () => {
    const data = await refreshAccess();
    setRecheckedStillOff(!!data && data.supported && !data.trusted);
  };

  const relaunchApp = async () => {
    const { relaunch } = await import("@tauri-apps/plugin-process");
    await relaunch();
  };

  const chooseTheme = (t: Theme) => {
    setTheme(t);
    applyTheme(t, true);
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
    if (r.ok) {
      setLan({ ...lan, antiIdleEnabled: r.data.antiIdleEnabled });
      if (enabled) {
        setRecheckedStillOff(false);
        refreshAccess(); // il permesso potrebbe essere cambiato
      }
    }
  };

  // Dal telefono i toggle di comportamento restano bloccati finché non è attivo
  // il controllo remoto: sbloccarlo (qui sotto) li rende operativi.
  const locked = !!lan?.remote && !lan?.remoteControlEnabled;

  // Avvisa solo se serve davvero: anti-idle attivo, permesso rilevante e non concesso.
  const needsAccessibility =
    !!lan?.antiIdleEnabled && !!access?.supported && !access.trusted;

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
            </div>
          </div>
          <Toggle
            checked={lan?.antiIdleEnabled ?? false}
            onChange={toggleAntiIdle}
            disabled={!lan || locked}
            label="Anti-inattività"
          />
        </div>
        {locked && (
          <div className="hint hint-locked">
            🔒 Dal telefono i comandi sono in sola lettura: attiva il <strong>Controllo
            remoto</strong> qui sotto per gestire questo interruttore.
          </div>
        )}
        {needsAccessibility && (
          <div className="banner banner-warn">
            <div>
              <strong>Permesso Accessibilità richiesto.</strong> Per muovere il mouse, macOS
              chiede di autorizzare RickyDEVTool in Impostazioni di Sistema → Privacy e sicurezza
              → Accessibilità. Finché non lo concedi, l'anti-inattività resta senza effetto.
              {recheckedStillOff && (
                <div className="banner-subnote">
                  Risulta ancora non concesso. Se l'hai appena attivato, macOS spesso applica il
                  permesso solo dopo aver <strong>riavviato l'app</strong>.
                </div>
              )}
            </div>
            <div className="banner-actions">
              <button onClick={() => post("/api/system/open-accessibility", {})}>
                Apri Accessibilità
              </button>
              <button onClick={recheckAccess}>Ho attivato, ricontrolla</button>
              {recheckedStillOff && isTauri && (
                <button className="primary" onClick={relaunchApp}>
                  Riavvia RickyDEVTool
                </button>
              )}
            </div>
          </div>
        )}
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
              <button onClick={() => setShowQr(true)}>Mostra QR di abbinamento</button>
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

      {showQr && (
        <div className="overlay" onClick={() => setShowQr(false)}>
          <div className="dialog qr-dialog" onClick={(e) => e.stopPropagation()}>
            <h3>Abbina uno smartphone</h3>
            <img
              className="qr"
              src={`${API_BASE}/api/lan/qr.svg`}
              alt="QR di abbinamento"
              width={220}
              height={220}
            />
            <p className="hint">
              Scansiona dal telefono: contiene indirizzo e token di abbinamento. Il telefono resta
              in sola lettura finché non attivi il controllo remoto.
            </p>
            <div className="dialog-actions">
              <button onClick={() => setShowQr(false)}>Chiudi</button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
