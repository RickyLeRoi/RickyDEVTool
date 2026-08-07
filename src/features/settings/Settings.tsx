import { useEffect, useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { api, API_BASE, post } from "../../lib/api";
import { Modal } from "../../components/Modal";
import { Toggle } from "../../components/Toggle";
import { AlertSettings } from "./AlertSettings";
import { AiSettings } from "./AiSettings";
import { AntiIdlePanel } from "./AntiIdlePanel";
import { applyTheme, getTheme, type Theme } from "../../lib/theme";
import { getLang, setLang, LANGS, type Lang } from "../../lib/i18n";
import { useTrayIntentStore } from "../../stores/trayIntentStore";
import type { LanInfo, PairedDevice } from "../../lib/types";

const THEMES: Theme[] = ["auto", "light", "dark"];
const LANG_LABELS: Record<Lang, string> = { it: "Italiano", en: "English" };

export function Settings() {
  const { t } = useTranslation();
  const [lan, setLan] = useState<LanInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showQr, setShowQr] = useState(false);
  const [theme, setTheme] = useState<Theme>(getTheme());
  const [lang, setLangState] = useState<Lang>(getLang());
  const [hubCode, setHubCode] = useState<string | null>(null);
  const [hubCodeDraft, setHubCodeDraft] = useState("");
  const [hubCodeError, setHubCodeError] = useState<string | null>(null);
  const [devices, setDevices] = useState<PairedDevice[]>([]);
  // 20260806 RG il QR è un <img>: dopo una rotazione va rifatta la richiesta, non letta la cache.
  const [qrSeq, setQrSeq] = useState(0);

  const loadDevices = () =>
    api<{ sessions: PairedDevice[] }>("/api/pair/sessions").then((r) => {
      if (r.ok) setDevices(r.data.sessions);
    });

  useEffect(() => {
    api<LanInfo>("/api/lan").then((r) => {
      if (r.ok) setLan(r.data);
      else setError(r.error.message);
    });
    api<{ code: string }>("/api/config/hub-code").then((r) => {
      if (r.ok) {
        setHubCode(r.data.code);
        setHubCodeDraft(r.data.code);
      }
    });
    loadDevices();
  }, []);

  const revoke = async (device: PairedDevice) => {
    await api(`/api/pair/sessions/${device.id}`, { method: "DELETE" });
    loadDevices();
  };

  const rotateToken = async () => {
    await post("/api/pair/rotate", {});
    setQrSeq((n) => n + 1);
  };

  const saveHubCode = async (body: { code?: string }) => {
    setHubCodeError(null);
    const r = await post<{ code: string }>("/api/config/hub-code", body);
    if (r.ok) {
      setHubCode(r.data.code);
      setHubCodeDraft(r.data.code);
    } else {
      setHubCodeError(r.error.message);
    }
  };

  const traySeq = useTrayIntentStore((s) => s.seq);
  useEffect(() => {
    const { section, extra } = useTrayIntentStore.getState();
    if (section === "settings" && extra === "qr") setShowQr(true);
  }, [traySeq]);

  const chooseTheme = (t: Theme) => {
    setTheme(t);
    applyTheme(t, true);
  };

  const chooseLang = (l: Lang) => {
    setLangState(l);
    setLang(l);
  };

  const toggleRemote = async (enabled: boolean) => {
    if (!lan) return;
    const r = await post<{ remoteControlEnabled: boolean }>("/api/config/remote-control", {
      enabled,
    });
    if (r.ok) setLan({ ...lan, remoteControlEnabled: r.data.remoteControlEnabled });
  };

  return (
    <div className="settings">
      <h2>{t("settings.title")}</h2>

      <section>
        <h3>{t("settings.language")}</h3>
        <div className="segmented">
          {LANGS.map((l) => (
            <button key={l} className={lang === l ? "active" : ""} onClick={() => chooseLang(l)}>
              {LANG_LABELS[l]}
            </button>
          ))}
        </div>
      </section>

      <section>
        <h3>{t("theme.appearance")}</h3>
        <div className="segmented">
          {THEMES.map((id) => (
            <button
              key={id}
              className={theme === id ? "active" : ""}
              onClick={() => chooseTheme(id)}
            >
              {t(`theme.${id}` as const)}
            </button>
          ))}
        </div>
      </section>

      <AlertSettings />

      <AiSettings />

      <AntiIdlePanel />

      <section>
        <h3>{t("settings.lanSection")}</h3>
        {error && <div className="banner banner-error">{error}</div>}
        {!lan && !error && <div className="empty">{t("common.loading")}</div>}
        {lan && (
          <>
            <div className="lan-status">
              {t("settings.lanStatus")}{" "}
              {lan.lanEnabled ? (
                <span className="badge badge-ok">{t("settings.lanActive", { port: lan.port })}</span>
              ) : (
                <span className="badge">{t("settings.lanLocalhostOnly")}</span>
              )}
            </div>
            <ul className="lan-urls">
              {lan.urls.map((u) => (
                <li key={u}>
                  <code>{u}</code>
                </li>
              ))}
              {lan.urls.length === 0 && <li>{t("settings.noLanIp")}</li>}
            </ul>

            {lan.lanEnabled && lan.urls.length > 0 && (
              <button onClick={() => setShowQr(true)}>{t("settings.showQr")}</button>
            )}

            <div className="setting-row">
              <div className="setting-text">
                <div className="setting-title">{t("settings.pairedDevices")}</div>
                <div className="hint">{t("settings.pairedDevicesHint")}</div>
              </div>
            </div>
            {devices.length === 0 ? (
              <div className="empty">{t("settings.noPairedDevices")}</div>
            ) : (
              <ul className="paired-devices">
                {devices.map((d) => (
                  <li key={d.id}>
                    <div className="paired-device-text">
                      <strong>{d.name}</strong>
                      <span className="dim">
                        {t("settings.pairedOn", {
                          date: new Date(d.createdAt).toLocaleDateString(getLang()),
                        })}
                        {d.lastSeen
                          ? t("settings.lastSeen", {
                              time: new Date(d.lastSeen).toLocaleTimeString(getLang()),
                            })
                          : t("settings.neverSeen")}
                      </span>
                    </div>
                    <button className="ghost" onClick={() => revoke(d)}>
                      {t("settings.revoke")}
                    </button>
                  </li>
                ))}
              </ul>
            )}

            <div className="setting-row">
              <div className="setting-text">
                <div className="setting-title">{t("settings.remoteControl")}</div>
                <div className="hint">{t("settings.remoteControlHint")}</div>
              </div>
              <Toggle
                checked={lan.remoteControlEnabled}
                onChange={toggleRemote}
                label={t("settings.remoteControl")}
              />
            </div>
          </>
        )}
      </section>

      <section>
        <h3>{t("settings.dropSection")}</h3>
        <p className="hint">
          <Trans i18nKey="settings.dropIntro" components={{ b: <strong /> }} />
        </p>
        {hubCodeError && <div className="banner banner-error">{hubCodeError}</div>}
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-title">{t("settings.hubCode")}</div>
            <div className="hint">
              {hubCode ? t("settings.hubCodeHintSet") : t("settings.hubCodeHintUnset")}
            </div>
          </div>
          <input
            className="input-mono"
            value={hubCodeDraft}
            placeholder={t("settings.hubCodePlaceholder")}
            onChange={(e) => setHubCodeDraft(e.target.value)}
            spellCheck={false}
          />
        </div>
        <div className="setting-actions">
          <button
            disabled={hubCodeDraft === (hubCode ?? "")}
            onClick={() => saveHubCode({ code: hubCodeDraft })}
          >
            {t("common.save")}
          </button>
          <button className="ghost" onClick={() => saveHubCode({})}>
            {t("settings.generateNew")}
          </button>
          {hubCode && (
            <button className="ghost" onClick={() => saveHubCode({ code: "" })}>
              {t("settings.deactivate")}
            </button>
          )}
        </div>
      </section>

      {showQr && (
        <Modal
          title={t("settings.qrTitle")}
          onCancel={() => setShowQr(false)}
          className="qr-dialog"
        >
          <img
            className="qr"
            src={`${API_BASE}/api/lan/qr.svg?v=${qrSeq}`}
            alt={t("settings.qrAlt")}
            width={220}
            height={220}
          />
          <p className="hint">{t("settings.qrHint")}</p>
          <div className="setting-actions">
            <button className="ghost" onClick={rotateToken}>
              {t("settings.qrRegenerate")}
            </button>
          </div>
          <p className="hint">
            <Trans i18nKey="settings.qrRegenerateHint" components={{ b: <strong /> }} />
          </p>
        </Modal>
      )}
    </div>
  );
}
