import { useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { useTrayIntentStore } from "../../stores/trayIntentStore";
import type { DnsRecordSet, LanHost, PingResult, PortCheckResult } from "../../lib/types";

type Tool = "ping" | "dns" | "ports" | "scan";

const TOOLS: { id: Tool; label: string }[] = [
  { id: "ping", label: "Ping" },
  { id: "dns", label: "DNS" },
  { id: "ports", label: "Porte" },
  { id: "scan", label: "Scan LAN" },
];

function Ping() {
  const [host, setHost] = useState("1.1.1.1");
  const [res, setRes] = useState<PingResult | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    const r = await post<PingResult>("/api/net/ping", { host });
    setBusy(false);
    if (r.ok) setRes(r.data);
  };
  return (
    <div className="net-tool">
      <form
        className="net-form"
        onSubmit={(e) => {
          e.preventDefault();
          run();
        }}
      >
        <input value={host} onChange={(e) => setHost(e.target.value)} placeholder="host o IP" />
        <button disabled={busy}>{busy ? "Ping…" : "Ping"}</button>
      </form>
      {res && (
        <div className="net-result">
          {res.error ? (
            <div className="banner banner-error">{res.error}</div>
          ) : (
            <>
              <div>
                {res.received}/{res.sent} risposte
                {res.avgMs != null && <> · media {res.avgMs.toFixed(1)} ms</>}
              </div>
              <div className="dim">
                {res.timesMs.map((t, i) => (
                  <span key={i}>{t.toFixed(1)}ms </span>
                ))}
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}

function Dns() {
  const [name, setName] = useState("");
  const [records, setRecords] = useState<DnsRecordSet[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    setError(null);
    const r = await post<{ records: DnsRecordSet[] }>("/api/net/dns", { name });
    setBusy(false);
    if (r.ok) setRecords(r.data.records);
    else setError(r.error.message);
  };
  return (
    <div className="net-tool">
      <form
        className="net-form"
        onSubmit={(e) => {
          e.preventDefault();
          run();
        }}
      >
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="esempio.com" />
        <button disabled={busy || !name.trim()}>{busy ? "Risolvo…" : "Risolvi"}</button>
      </form>
      {error && <div className="banner banner-error">{error}</div>}
      {records && (
        <table className="proc-table net-result">
          <tbody>
            {records.map((r) => (
              <tr key={r.recordType}>
                <td className="dim">{r.recordType}</td>
                <td>{r.values.join(", ")}</td>
              </tr>
            ))}
            {records.length === 0 && (
              <tr>
                <td className="dim">Nessun record</td>
              </tr>
            )}
          </tbody>
        </table>
      )}
    </div>
  );
}

function Ports() {
  const [host, setHost] = useState("");
  const [portsStr, setPortsStr] = useState("22, 80, 443, 3000, 8080");
  const [results, setResults] = useState<PortCheckResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    const ports = portsStr
      .split(/[\s,]+/)
      .map((p) => parseInt(p, 10))
      .filter((p) => p > 0 && p < 65536);
    setBusy(true);
    setError(null);
    const r = await post<{ results: PortCheckResult[] }>("/api/net/portcheck", { host, ports });
    setBusy(false);
    if (r.ok) setResults(r.data.results);
    else setError(r.error.message);
  };
  return (
    <div className="net-tool">
      <form
        className="net-form"
        onSubmit={(e) => {
          e.preventDefault();
          run();
        }}
      >
        <input value={host} onChange={(e) => setHost(e.target.value)} placeholder="host o IP" />
        <input
          value={portsStr}
          onChange={(e) => setPortsStr(e.target.value)}
          placeholder="22, 80, 443"
        />
        <button disabled={busy || !host.trim()}>{busy ? "Controllo…" : "Controlla"}</button>
      </form>
      {error && <div className="banner banner-error">{error}</div>}
      {results && (
        <div className="net-result port-chips">
          {results.map((r) => (
            <span key={r.port} className={`port-pill ${r.open ? "open" : "closed"}`}>
              {r.port} {r.open ? `· ${r.latencyMs}ms` : "chiusa"}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function Scan({ autoRunSeq }: { autoRunSeq: number }) {
  const [hosts, setHosts] = useState<LanHost[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    setError(null);
    const r = await api<{ hosts: LanHost[] }>("/api/net/scan");
    setBusy(false);
    if (r.ok) setHosts(r.data.hosts);
    else setError(r.error.message);
  };

  // "Scansiona rete locale..." scelto dal menu del tray.
  useEffect(() => {
    if (autoRunSeq === 0) return;
    run();
  }, [autoRunSeq]);

  return (
    <div className="net-tool">
      <button disabled={busy} onClick={run}>
        {busy ? "Scansione in corso…" : "Scansiona la rete locale"}
      </button>
      {error && <div className="banner banner-error">{error}</div>}
      {hosts && (
        <table className="proc-table net-result">
          <thead>
            <tr>
              <th>IP</th>
              <th>Hostname</th>
              <th>MAC</th>
              <th className="num">Ping</th>
            </tr>
          </thead>
          <tbody>
            {hosts.map((h) => (
              <tr key={h.ip}>
                <td>
                  {h.ip}
                  {h.isSelf && <span className="badge">questo PC</span>}
                </td>
                <td className="dim">{h.hostname ?? "—"}</td>
                <td className="dim">{h.mac ?? "—"}</td>
                <td className="num dim">{h.latencyMs != null ? `${h.latencyMs.toFixed(0)}ms` : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

export function NetTools() {
  const [tool, setTool] = useState<Tool>("ping");
  const traySection = useTrayIntentStore((s) => s.section);
  const trayExtra = useTrayIntentStore((s) => s.extra);
  const traySeq = useTrayIntentStore((s) => s.seq);
  const trayWantsScan = traySection === "net" && trayExtra === "scan";

  useEffect(() => {
    if (trayWantsScan) setTool("scan");
  }, [trayWantsScan, traySeq]);

  return (
    <div>
      <div className="section-header">
        <h2>Rete</h2>
        <div className="segmented">
          {TOOLS.map((t) => (
            <button key={t.id} className={tool === t.id ? "active" : ""} onClick={() => setTool(t.id)}>
              {t.label}
            </button>
          ))}
        </div>
      </div>
      {tool === "ping" && <Ping />}
      {tool === "dns" && <Dns />}
      {tool === "ports" && <Ports />}
      {tool === "scan" && <Scan autoRunSeq={trayWantsScan ? traySeq : 0} />}
    </div>
  );
}
