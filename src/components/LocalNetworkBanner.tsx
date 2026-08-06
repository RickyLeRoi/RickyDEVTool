import { useEffect, useState } from "react";
import { api, post } from "../lib/api";
import type { LocalNetworkStatus } from "../lib/types";

const isTauri = "__TAURI_INTERNALS__" in window;

// 20260805 RG il popup di macOS si vede una volta sola.
export function LocalNetworkBanner({ what }: { what: string }) {
  const [status, setStatus] = useState<LocalNetworkStatus | null>(null);
  const [recheckedStillOff, setRecheckedStillOff] = useState(false);

  useEffect(() => {
    refresh();
  }, []);

  const refresh = () =>
    api<LocalNetworkStatus>("/api/system/local-network").then((r) => {
      if (r.ok) setStatus(r.data);
      return r.ok ? r.data : null;
    });

  const recheck = async () => {
    const data = await refresh();
    setRecheckedStillOff(!!data && data.supported && !data.granted);
  };

  const relaunchApp = async () => {
    const { relaunch } = await import("@tauri-apps/plugin-process");
    await relaunch();
  };

  if (!status?.supported || status.granted) return null;

  return (
    <div className="banner banner-warn">
      <div>
        <strong>Permesso Rete locale richiesto.</strong> Per raggiungere {what} su un altro
        computer, macOS chiede di autorizzare RickyDEVTool in Impostazioni di Sistema → Privacy e
        sicurezza → Rete locale. Finché non lo concedi il servizio risulta irraggiungibile, anche
        se è acceso e configurato bene.
        {recheckedStillOff && (
          <div className="banner-subnote">
            Risulta ancora non concesso. Se l'hai appena attivato, macOS applica il permesso solo
            dopo aver <strong>riavviato l'app</strong>. Se RickyDEVTool non compare proprio
            nell'elenco, l'app non è firmata: ricompilala con la versione aggiornata.
          </div>
        )}
      </div>
      <div className="banner-actions">
        <button onClick={() => post("/api/system/open-local-network", {})}>
          Apri Rete locale
        </button>
        <button onClick={recheck}>Ho attivato, ricontrolla</button>
        {recheckedStillOff && isTauri && (
          <button className="primary" onClick={relaunchApp}>
            Riavvia RickyDEVTool
          </button>
        )}
      </div>
    </div>
  );
}
