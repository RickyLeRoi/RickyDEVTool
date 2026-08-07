import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { Toggle } from "../../components/Toggle";
import { hideToTray } from "../../lib/appWindow";
import type { LanInfo } from "../../lib/types";

export function WindowPanel() {
  const { t } = useTranslation();
  const [lan, setLan] = useState<LanInfo | null>(null);

  useEffect(() => {
    api<LanInfo>("/api/lan").then((r) => {
      if (r.ok) setLan(r.data);
    });
  }, []);

  const toggleCloseToTray = async (enabled: boolean) => {
    if (!lan) return;
    const r = await post<{ closeToTray: boolean }>("/api/config/close-to-tray", { enabled });
    if (r.ok) setLan({ ...lan, closeToTray: r.data.closeToTray });
  };

  return (
    <section>
      <h3>{t("windowPanel.title")}</h3>
      <div className="setting-row">
        <div className="setting-text">
          <div className="setting-title">{t("windowPanel.closeToTray")}</div>
          <div className="hint">
            {lan?.closeToTray === false
              ? t("windowPanel.closeToTrayOffHint")
              : t("windowPanel.closeToTrayOnHint")}
          </div>
        </div>
        <Toggle
          checked={lan?.closeToTray ?? true}
          onChange={toggleCloseToTray}
          disabled={!lan}
          label={t("windowPanel.closeToTray")}
        />
      </div>
      <div className="setting-row">
        <div className="setting-text">
          <div className="setting-title">{t("windowPanel.minimize")}</div>
          <div className="hint">{t("windowPanel.minimizeHint")}</div>
        </div>
        <button onClick={hideToTray}>{t("windowPanel.minimizeAction")}</button>
      </div>
    </section>
  );
}
