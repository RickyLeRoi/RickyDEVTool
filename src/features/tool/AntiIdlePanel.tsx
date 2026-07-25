import { useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { Toggle } from "../../components/Toggle";
import type { AccessibilityStatus, LanInfo } from "../../lib/types";

const isTauri = "__TAURI_INTERNALS__" in window;

/**
 * Interruttore anti-inattività (muove il mouse dopo minuti di idle) con la
 * gestione del permesso Accessibilità di macOS. Estratto dalle Impostazioni per
 * vivere come tab della pagina Tool: recupera da solo LAN e stato accessibilità.
 */
export function AntiIdlePanel() {
  const [lan, setLan] = useState<LanInfo | null>(null);
  const [access, setAccess] = useState<AccessibilityStatus | null>(null);
  const [recheckedStillOff, setRecheckedStillOff] = useState(false);

  useEffect(() => {
    api<LanInfo>("/api/lan").then((r) => {
      if (r.ok) setLan(r.data);
    });
    refreshAccess();
  }, []);

  const refreshAccess = () =>
    api<AccessibilityStatus>("/api/system/accessibility").then((r) => {
      if (r.ok) setAccess(r.data);
      return r.ok ? r.data : null;
    });

  const recheckAccess = async () => {
    const data = await refreshAccess();
    setRecheckedStillOff(!!data && data.supported && !data.trusted);
  };

  const relaunchApp = async () => {
    const { relaunch } = await import("@tauri-apps/plugin-process");
    await relaunch();
  };

  const toggleAntiIdle = async (enabled: boolean) => {
    if (!lan) return;
    const r = await post<{ antiIdleEnabled: boolean }>("/api/config/anti-idle", { enabled });
    if (r.ok) {
      setLan({ ...lan, antiIdleEnabled: r.data.antiIdleEnabled });
      if (enabled) {
        setRecheckedStillOff(false);
        refreshAccess();
      }
    }
  };

  // Dal telefono resta bloccato finché non è attivo il controllo remoto.
  const locked = !!lan?.remote && !lan?.remoteControlEnabled;
  const needsAccessibility =
    !!lan?.antiIdleEnabled && !!access?.supported && !access.trusted;

  return (
    <section className="tool-panel">
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
          remoto</strong> dalle Impostazioni per gestire questo interruttore.
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
  );
}
