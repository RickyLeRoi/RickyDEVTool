import { useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { Toggle } from "../../components/Toggle";
import { isTauri } from "../../lib/appWindow";
import type { AccessibilityStatus, LanInfo } from "../../lib/types";

export function AntiIdlePanel() {
  const { t } = useTranslation();
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

  const locked = !!lan?.remote && !lan?.remoteControlEnabled;
  const needsAccessibility = !!lan?.antiIdleEnabled && !!access?.supported && !access.trusted;

  return (
    <section>
      <h3>{t("antiIdle.title")}</h3>
      <div className="setting-row">
        <div className="setting-text">
          <div className="setting-title">{t("antiIdle.moveMouse")}</div>
          <div className="hint">{t("antiIdle.moveMouseHint")}</div>
        </div>
        <Toggle
          checked={lan?.antiIdleEnabled ?? false}
          onChange={toggleAntiIdle}
          disabled={!lan || locked}
          label={t("antiIdle.label")}
        />
      </div>
      {locked && (
        <div className="hint hint-locked">
          <Trans i18nKey="antiIdle.locked" components={{ b: <strong /> }} />
        </div>
      )}
      {needsAccessibility && (
        <div className="banner banner-warn">
          <div>
            <Trans i18nKey="antiIdle.accessRequired" components={{ b: <strong /> }} />
            {recheckedStillOff && (
              <div className="banner-subnote">
                <Trans i18nKey="antiIdle.accessStillOff" components={{ b: <strong /> }} />
              </div>
            )}
          </div>
          <div className="banner-actions">
            <button onClick={() => post("/api/system/open-accessibility", {})}>
              {t("antiIdle.openAccessibility")}
            </button>
            <button onClick={recheckAccess}>{t("antiIdle.recheck")}</button>
            {recheckedStillOff && isTauri && (
              <button className="primary" onClick={relaunchApp}>
                {t("antiIdle.relaunch")}
              </button>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
