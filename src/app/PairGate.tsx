import { useEffect, useState, type ReactNode } from "react";
import { api, post } from "../lib/api";

type GateState = "checking" | "ok" | "needs-pairing";

/**
 * Gestisce l'abbinamento dei device LAN:
 * - se l'URL contiene #pair=<token> (arrivo da QR) esegue il pairing e pulisce l'hash;
 * - se l'API risponde 401 mostra la schermata di inserimento token.
 * Da localhost/desktop passa sempre.
 */
export function PairGate({ children }: { children: ReactNode }) {
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
      post("/api/pair", { token: match[1] }).then(() => {
        history.replaceState(null, "", window.location.pathname);
        check();
      });
    } else {
      check();
    }
  }, []);

  if (state === "checking") {
    return <div className="fullscreen-msg">Connessione…</div>;
  }
  if (state === "needs-pairing") {
    return (
      <div className="fullscreen-msg">
        <h2>Abbina questo dispositivo</h2>
        <p>Scansiona il QR nelle Impostazioni del desktop, oppure inserisci il token:</p>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            const r = await post("/api/pair", { token: token.trim() });
            if (r.ok) check();
            else setFailed(true);
          }}
        >
          <input
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder="token di abbinamento"
            autoFocus
          />
          <button type="submit">Abbina</button>
        </form>
        {failed && <p className="banner-error-text">Token non valido.</p>}
      </div>
    );
  }
  return <>{children}</>;
}
