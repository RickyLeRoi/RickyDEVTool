import { useEffect, useState } from "react";
import { ws } from "../../lib/ws";
import { API_BASE } from "../../lib/api";
import { usePortsStore } from "../../stores/portsStore";
import { KillDialog } from "./KillDialog";
import type { PortEntry, PortProcess, PortScan } from "../../lib/types";

const KNOWN_LABELS: Record<string, string> = {
  node: "node",
  dotnet: ".NET",
  docker: "docker",
  ssh: "ssh",
  plex: "plex",
  samba: "samba",
  iisexpress: "IIS",
  visualstudio: "VS",
  vscode: "VS Code",
  postgres: "pg",
  mysql: "mysql",
  redis: "redis",
  nginx: "nginx",
  python: "py",
  java: "java",
  chrome: "chrome",
};

function AppBadge({ app }: { app: string | null }) {
  if (!app) return null;
  return <span className="badge badge-app">{KNOWN_LABELS[app] ?? app}</span>;
}

function PortRow({ entry }: { entry: PortEntry }) {
  const [expanded, setExpanded] = useState(false);
  const [killing, setKilling] = useState<PortProcess | null>(null);

  const openInBrowser = () => {
    const base = API_BASE || window.location.origin;
    const host = new URL(base).hostname === "127.0.0.1" ? "localhost" : new URL(base).hostname;
    window.open(`http://${host}:${entry.port}`, "_blank");
  };

  return (
    <>
      <tr className="port-row" onClick={() => setExpanded(!expanded)}>
        <td className="num port-number">{entry.port}</td>
        <td>{entry.protocol}</td>
        <td className="dim">{entry.addresses.join(", ")}</td>
        <td>
          {entry.processes.map((p) => (
            <span key={p.pid} className="proc-chip">
              {p.name}
              <AppBadge app={p.knownApp} />
            </span>
          ))}
        </td>
        <td className="num dim">{expanded ? "▾" : "▸"}</td>
      </tr>
      {expanded && (
        <tr className="port-detail">
          <td colSpan={5}>
            <div className="port-actions">
              <button onClick={() => navigator.clipboard.writeText(`localhost:${entry.port}`)}>
                Copia localhost:{entry.port}
              </button>
              <button onClick={openInBrowser}>Apri nel browser</button>
            </div>
            <table className="proc-table inner">
              <tbody>
                {entry.processes.map((p) => (
                  <tr key={p.pid} title={p.exePath ?? undefined}>
                    <td>
                      {p.name}
                      <AppBadge app={p.knownApp} />
                      {p.killProtection === "typed-confirm" && (
                        <span className="badge badge-warn">protetto</span>
                      )}
                    </td>
                    <td className="num">PID {p.pid}</td>
                    <td>{p.user ?? "—"}</td>
                    <td className="num">
                      <button className="danger small" onClick={() => setKilling(p)}>
                        Kill
                      </button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </td>
        </tr>
      )}
      {killing && <KillDialog process={killing} onClose={() => setKilling(null)} />}
    </>
  );
}

export function Ports() {
  const { scan, error, setScan, setError } = usePortsStore();

  // La sottoscrizione parte quando la sezione è aperta e si spegne quando
  // si esce: il backend ferma il poller senza subscriber.
  useEffect(() => {
    return ws.subscribe("ports", (event) => {
      if (event.topic === "ports") setScan(event.payload as PortScan);
      else if (event.topic === "ports:error")
        setError((event.payload as { message: string }).message);
    });
  }, [setScan, setError]);

  return (
    <div>
      <div className="section-header">
        <h2>
          Porte in ascolto
          <span className="live-dot" title="monitoraggio attivo" />
        </h2>
        {scan && (
          <span className="dim">
            {scan.ports.length} porte · {scan.hiddenSystem} di sistema nascoste
          </span>
        )}
      </div>

      {error && <div className="banner banner-error">Errore scansione porte: {error}</div>}
      {!scan && !error && <div className="empty">Scansione in corso…</div>}
      {scan && scan.ports.length === 0 && (
        <div className="empty">Nessuna porta non di sistema in ascolto.</div>
      )}

      {scan && scan.ports.length > 0 && (
        <table className="proc-table">
          <thead>
            <tr>
              <th className="num">Porta</th>
              <th>Proto</th>
              <th>Bind</th>
              <th>Processi</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {scan.ports.map((entry) => (
              <PortRow key={`${entry.protocol}-${entry.port}`} entry={entry} />
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
