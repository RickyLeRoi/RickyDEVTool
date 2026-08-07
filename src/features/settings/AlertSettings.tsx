import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { Toggle } from "../../components/Toggle";
import type { AlertThresholds } from "../../lib/types";

export function AlertSettings() {
  const { t } = useTranslation();
  const [cfg, setCfg] = useState<AlertThresholds | null>(null);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api<AlertThresholds>("/api/alerts/config").then((r) => {
      if (r.ok && typeof r.data.cpuPct === "number") setCfg(r.data);
    });
  }, []);

  const save = async (next: AlertThresholds) => {
    const prev = cfg;
    setCfg(next);
    setError(null);
    const r = await post<AlertThresholds>("/api/alerts/config", next);
    if (r.ok) {
      setCfg(r.data);
      setSaved(true);
      setTimeout(() => setSaved(false), 1200);
    } else {
      if (prev) setCfg(prev);
      setError(r.error.message);
    }
  };

  return (
    <section>
      <h3>
        {t("alerts.title")} {saved && <span className="badge badge-ok">{t("alerts.saved")}</span>}
      </h3>
      {error && <div className="banner banner-error">{error}</div>}
      {!cfg && <div className="empty">{t("common.loading")}</div>}
      {cfg && (
        <div className="alert-settings">
          <label className="form-row alert-field">
            <span>{t("alerts.cpuOver")}</span>
            <input
              type="number"
              min={10}
              max={100}
              value={cfg.cpuPct}
              onChange={(e) => save({ ...cfg, cpuPct: Number(e.target.value) })}
            />
            <span className="dim">{t("alerts.cpuUnit")}</span>
          </label>

          <label className="form-row alert-field">
            <span>{t("alerts.ramOver")}</span>
            <input
              type="number"
              min={10}
              max={100}
              value={cfg.memPct}
              onChange={(e) => save({ ...cfg, memPct: Number(e.target.value) })}
            />
            <span className="dim">{t("alerts.pctUnit")}</span>
          </label>

          <div className="setting-row">
            <div className="setting-text">
              <div className="setting-title">{t("alerts.highTemp")}</div>
              <div className="hint">{t("alerts.highTempHint")}</div>
            </div>
            <Toggle
              checked={cfg.tempEnabled}
              onChange={(v) => save({ ...cfg, tempEnabled: v })}
              label={t("alerts.tempAlertLabel")}
            />
          </div>
          {cfg.tempEnabled && (
            <label className="form-row alert-field">
              <span>{t("alerts.tempThreshold")}</span>
              <input
                type="number"
                min={30}
                max={120}
                value={cfg.tempC}
                onChange={(e) => save({ ...cfg, tempC: Number(e.target.value) })}
              />
              <span className="dim">{t("alerts.tempUnit")}</span>
            </label>
          )}

          <div className="setting-row">
            <div className="setting-text">
              <div className="setting-title">{t("alerts.lowBattery")}</div>
              <div className="hint">{t("alerts.lowBatteryHint")}</div>
            </div>
            <Toggle
              checked={cfg.batteryEnabled}
              onChange={(v) => save({ ...cfg, batteryEnabled: v })}
              label={t("alerts.batteryAlertLabel")}
            />
          </div>
          {cfg.batteryEnabled && (
            <label className="form-row alert-field">
              <span>{t("alerts.batteryThreshold")}</span>
              <input
                type="number"
                min={1}
                max={100}
                value={cfg.batteryPct}
                onChange={(e) => save({ ...cfg, batteryPct: Number(e.target.value) })}
              />
              <span className="dim">{t("alerts.pctUnit")}</span>
            </label>
          )}
        </div>
      )}
    </section>
  );
}
