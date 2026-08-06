import { useEffect, useState } from "react";
import { api, API_BASE, post } from "../../lib/api";
import { Modal } from "../../components/Modal";
import { Toggle } from "../../components/Toggle";
import { AlertSettings } from "./AlertSettings";
import { AiSettings } from "./AiSettings";
import { AntiIdlePanel } from "./AntiIdlePanel";
import { applyTheme, getTheme, type Theme } from "../../lib/theme";
import { useTrayIntentStore } from "../../stores/trayIntentStore";
import type { LanInfo, PairedDevice } from "../../lib/types";

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
  const [hubCode, setHubCode] = useState<string | null>(null);
  const [hubCodeDraft, setHubCodeDraft] = useState("");
  const [hubCodeError, setHubCodeError] = useState<string | null>(null);
  const [devices, setDevices] = useState<PairedDevice[]>([]);
  // il QR è un <img>: dopo una rotazione va rifatta la richiesta, non riletta la cache
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

  const toggleRemote = async (enabled: boolean) => {
    if (!lan) return;
    const r = await post<{ remoteControlEnabled: boolean }>("/api/config/remote-control", {
      enabled,
    });
    if (r.ok) setLan({ ...lan, remoteControlEnabled: r.data.remoteControlEnabled });
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

      <AlertSettings />

      <AiSettings />

      <AntiIdlePanel />

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
                <div className="setting-title">Dispositivi abbinati</div>
                <div className="hint">
                  Ogni abbinamento è una sessione a sé: revocarne una non tocca le altre.
                </div>
              </div>
            </div>
            {devices.length === 0 ? (
              <div className="empty">Nessun dispositivo abbinato.</div>
            ) : (
              <ul className="paired-devices">
                {devices.map((d) => (
                  <li key={d.id}>
                    <div className="paired-device-text">
                      <strong>{d.name}</strong>
                      <span className="dim">
                        abbinato il {new Date(d.createdAt).toLocaleDateString()}
                        {d.lastSeen
                          ? ` · visto ${new Date(d.lastSeen).toLocaleTimeString()}`
                          : " · mai connesso da questo avvio"}
                      </span>
                    </div>
                    <button className="ghost" onClick={() => revoke(d)}>
                      Revoca
                    </button>
                  </li>
                ))}
              </ul>
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

      <section>
        <h3>Invio tra i tuoi computer (Drop)</h3>
        <p className="hint">
          Perché due computer si vedano e possano scambiarsi file, devono avere lo{" "}
          <strong>stesso codice hub</strong>. Senza codice la funzione è spenta e gli altri PC
          devono abbinarsi come un normale dispositivo LAN.
        </p>
        {hubCodeError && <div className="banner banner-error">{hubCodeError}</div>}
        <div className="setting-row">
          <div className="setting-text">
            <div className="setting-title">Codice hub</div>
            <div className="hint">
              {hubCode
                ? "Copialo e incollalo nelle Impostazioni dell'altro computer."
                : "Nessun codice impostato: l'invio tra computer è disattivato."}
            </div>
          </div>
          <input
            className="input-mono"
            value={hubCodeDraft}
            placeholder="es. k7f2-9m4x-tq81"
            onChange={(e) => setHubCodeDraft(e.target.value)}
            spellCheck={false}
          />
        </div>
        <div className="setting-actions">
          <button
            disabled={hubCodeDraft === (hubCode ?? "")}
            onClick={() => saveHubCode({ code: hubCodeDraft })}
          >
            Salva
          </button>
          <button className="ghost" onClick={() => saveHubCode({})}>
            Genera nuovo
          </button>
          {hubCode && (
            <button className="ghost" onClick={() => saveHubCode({ code: "" })}>
              Disattiva
            </button>
          )}
        </div>
      </section>

      {showQr && (
        <Modal
          title="Abbina uno smartphone"
          onCancel={() => setShowQr(false)}
          className="qr-dialog"
        >
          <img
            className="qr"
            src={`${API_BASE}/api/lan/qr.svg?v=${qrSeq}`}
            alt="QR di abbinamento"
            width={220}
            height={220}
          />
          <p className="hint">
            Scansiona dal telefono: contiene indirizzo e token di abbinamento. Il telefono resta
            in sola lettura finché non attivi il controllo remoto.
          </p>
          <div className="setting-actions">
            <button className="ghost" onClick={rotateToken}>
              Genera nuovo QR
            </button>
          </div>
          <p className="hint">
            Rigenerare il QR invalida quello vecchio (utile se ne è girato uno screenshot), ma{" "}
            <strong>non scollega</strong> i dispositivi già abbinati: per quelli usa Revoca.
          </p>
        </Modal>
      )}
    </div>
  );
}
