import { useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { api, post } from "../lib/api";
import { isTauri } from "../lib/appWindow";
import type { LocalNetworkStatus } from "../lib/types";

// 20260805 RG il popup di macOS si vede una volta sola.
export function LocalNetworkBanner({ what }: { what: string }) {
  const { t } = useTranslation();
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
        <Trans i18nKey="localNetwork.required" values={{ what }} components={{ b: <strong /> }} />
        {recheckedStillOff && (
          <div className="banner-subnote">
            <Trans i18nKey="localNetwork.stillOff" components={{ b: <strong /> }} />
          </div>
        )}
      </div>
      <div className="banner-actions">
        <button onClick={() => post("/api/system/open-local-network", {})}>
          {t("localNetwork.open")}
        </button>
        <button onClick={recheck}>{t("localNetwork.recheck")}</button>
        {recheckedStillOff && isTauri && (
          <button className="primary" onClick={relaunchApp}>
            {t("localNetwork.relaunch")}
          </button>
        )}
      </div>
    </div>
  );
}
