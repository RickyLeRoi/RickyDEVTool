import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { api, post } from "../lib/api";
import { getDeviceName } from "../lib/device";

type GateState = "checking" | "ok" | "needs-pairing";

export function PairGate({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const [state, setState] = useState<GateState>("checking");
  const [token, setToken] = useState("");
  const [failed, setFailed] = useState(false);

  const check = async () => {
    const r = await api("/api/health");
    setState(r.ok ? "ok" : "needs-pairing");
  };

  useEffect(() => {
    const match = window.location.hash.match(/#pair=([0-9a-f]+)/);
    if (match) {
      post("/api/pair", { token: match[1], deviceName: getDeviceName() }).then(() => {
        // 20260806 ++ RG #Security il token non deve restare nella barra degli indirizzi né in cronologia.
        history.replaceState(null, "", window.location.pathname);
        check();
      });
    } else {
      check();
    }
  }, []);

  if (state === "checking") {
    return <div className="fullscreen-msg">{t("pairGate.connecting")}</div>;
  }
  if (state === "needs-pairing") {
    return (
      <div className="fullscreen-msg">
        <h2>{t("pairGate.title")}</h2>
        <p>{t("pairGate.intro")}</p>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            const r = await post("/api/pair", {
              token: token.trim(),
              deviceName: getDeviceName(),
            });
            if (r.ok) check();
            else setFailed(true);
          }}
        >
          <input
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder={t("pairGate.tokenPlaceholder")}
            autoFocus
          />
          <button type="submit">{t("pairGate.pair")}</button>
        </form>
        {failed && <p className="banner-error-text">{t("pairGate.invalidToken")}</p>}
      </div>
    );
  }
  return <>{children}</>;
}
