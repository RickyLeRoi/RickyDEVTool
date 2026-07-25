import { useEffect, useRef, useState } from "react";
import { post } from "../../lib/api";
import { useNavStore } from "../../stores/navStore";
import { useNetScanStore } from "../../stores/netScanStore";
import { Tabs, usePageTab, type TabDef } from "../../components/Tabs";
import { Services } from "../services/Services";
import { Ports as ListeningPorts } from "../ports/Ports";
import { TaskLog } from "../../components/TaskLog";
import type { DnsRecordSet, LanHost, PingResult, PortCheckResult, TaskInfo } from "../../lib/types";

// "listen" (Porte in ascolto) è il primo tab e apre di default.
const TOOLS: TabDef[] = [
  { id: "listen", label: "Porte in ascolto" },
  { id: "services", label: "Servizi" },
  { id: "ping", label: "Ping" },
  { id: "dns", label: "DNS" },
  { id: "portcheck", label: "Port check" },
  { id: "traceroute", label: "Traceroute" },
  { id: "scan", label: "Scan LAN" },
];

const PING_ATTEMPTS = 10;

function ProgressBar({ done, total }: { done: number; total: number }) {
  const pct = total > 0 ? Math.min(100, (done / total) * 100) : 0;
  return (
    <div className="progress-bar">
      <div className="progress-bar-fill" style={{ width: `${pct}%` }} />
    </div>
  );
}

function Ping() {
  const [host, setHost] = useState("1.1.1.1");
  const [attempts, setAttempts] = useState<{ ms: number | null }[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const cancelRef = useRef(false);

  useEffect(() => () => {
    cancelRef.current = true;
  }, []);

  const run = async () => {
    cancelRef.current = false;
    setBusy(true);
    setError(null);
    setAttempts([]);
    for (let i = 0; i < PING_ATTEMPTS; i++) {
      if (cancelRef.current) return;
      const r = await post<PingResult>("/api/net/ping", { host, count: 1 });
      if (cancelRef.current) return;
      if (r.ok) {
        setAttempts((prev) => [...prev, { ms: r.data.timesMs[0] ?? null }]);
      } else {
        setError(r.error.message);
        break;
      }
    }
    setBusy(false);
  };

  const received = attempts.filter((a) => a.ms != null).length;
  const times = attempts.map((a) => a.ms).filter((ms): ms is number => ms != null);
  const avg = times.length > 0 ? times.reduce((a, b) => a + b, 0) / times.length : null;

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
        <button disabled={busy}>
          {busy ? `Ping… (${attempts.length}/${PING_ATTEMPTS})` : "Ping"}
        </button>
      </form>
      {busy && <ProgressBar done={attempts.length} total={PING_ATTEMPTS} />}
      {error && <div className="banner banner-error">{error}</div>}
      {attempts.length > 0 && (
        <div className="net-result">
          <div>
            {received}/{attempts.length} risposte
            {avg != null && <> · media {avg.toFixed(1)} ms</>}
          </div>
          <div className="dim ping-attempts">
            {attempts.map((a, i) => (
              <span key={i} className={a.ms == null ? "ping-lost" : undefined}>
                {a.ms != null ? `${a.ms.toFixed(1)}ms` : "✕"}
              </span>
            ))}
          </div>
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

// Porte comuni, per il pulsante "Porte note" e per etichettare le porte
// aperte trovate da "Tutte le porte" con un nome plausibile (per numero di
// porta convenzionale, non un vero fingerprint del servizio).
const PORT_SERVICE_NAMES: Record<number, string> = {
  21: "FTP",
  22: "SSH",
  23: "Telnet",
  25: "SMTP",
  53: "DNS",
  67: "DHCP",
  68: "DHCP",
  69: "TFTP",
  80: "HTTP",
  110: "POP3",
  111: "RPCbind",
  123: "NTP",
  135: "RPC (Windows)",
  137: "NetBIOS",
  138: "NetBIOS",
  139: "NetBIOS/SMB",
  143: "IMAP",
  161: "SNMP",
  162: "SNMP trap",
  179: "BGP",
  389: "LDAP",
  443: "HTTPS",
  445: "SMB",
  465: "SMTPS",
  500: "IKE/VPN",
  514: "Syslog",
  515: "LPD",
  587: "SMTP (submission)",
  631: "IPP (CUPS)",
  636: "LDAPS",
  993: "IMAPS",
  995: "POP3S",
  1080: "SOCKS",
  1433: "SQL Server",
  1521: "Oracle DB",
  1723: "PPTP",
  2049: "NFS",
  2181: "ZooKeeper",
  2375: "Docker",
  2376: "Docker (TLS)",
  3000: "dev server",
  3128: "Squid proxy",
  3268: "LDAP GC",
  3306: "MySQL",
  3389: "RDP",
  3690: "Subversion",
  4000: "dev server",
  4369: "Erlang epmd",
  5000: "dev server",
  5060: "SIP",
  5222: "XMPP",
  5432: "PostgreSQL",
  5555: "ADB",
  5601: "Kibana",
  5672: "AMQP",
  5900: "VNC",
  5984: "CouchDB",
  6379: "Redis",
  6443: "Kubernetes API",
  7000: "Cassandra",
  7001: "Cassandra (TLS)",
  7077: "Spark",
  7199: "Cassandra JMX",
  8000: "HTTP-alt",
  8008: "HTTP-alt",
  8080: "HTTP proxy",
  8081: "HTTP-alt",
  8443: "HTTPS-alt",
  8500: "Consul",
  8888: "Jupyter",
  9000: "dev/PHP-FPM",
  9042: "Cassandra CQL",
  9090: "Prometheus",
  9092: "Kafka",
  9200: "Elasticsearch",
  9300: "Elasticsearch (transport)",
  9418: "Git",
  9999: "dev server",
  11211: "Memcached",
  15672: "RabbitMQ mgmt",
  16379: "Redis cluster",
  20000: "dev server",
  25565: "Minecraft",
  27017: "MongoDB",
  27018: "MongoDB",
  28017: "MongoDB HTTP",
  32400: "Plex",
};
const WELL_KNOWN_PORTS = Object.keys(PORT_SERVICE_NAMES).map(Number).sort((a, b) => a - b);
const ALL_PORTS = Array.from({ length: 65535 }, (_, i) => i + 1);
const PORT_BATCH_SIZE = 500;
const PORT_HISTORY_KEY = "rdt-portscan-history";

interface PortScanHistoryEntry {
  host: string;
  portsStr: string;
}

function loadPortHistory(): PortScanHistoryEntry[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(PORT_HISTORY_KEY) ?? "[]");
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function savePortHistory(entry: PortScanHistoryEntry): PortScanHistoryEntry[] {
  const prev = loadPortHistory().filter(
    (h) => !(h.host === entry.host && h.portsStr === entry.portsStr),
  );
  const next = [entry, ...prev].slice(0, 5);
  localStorage.setItem(PORT_HISTORY_KEY, JSON.stringify(next));
  return next;
}

/** Elenco libero (virgole/spazi) → porte valide, deduplicate e ordinate. */
function parsePortsInput(input: string): number[] {
  const set = new Set<number>();
  for (const token of input.split(/[\s,]+/)) {
    const n = parseInt(token, 10);
    if (n > 0 && n < 65536) set.add(n);
  }
  return [...set].sort((a, b) => a - b);
}

function Ports() {
  const [host, setHost] = useState("");
  const [portsStr, setPortsStr] = useState("22, 80, 443, 3000, 8080");
  const [results, setResults] = useState<PortCheckResult[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState({ done: 0, total: 0 });
  const [history, setHistory] = useState<PortScanHistoryEntry[]>(() => loadPortHistory());
  const cancelRef = useRef(false);

  useEffect(() => () => {
    cancelRef.current = true;
  }, []);

  const scan = async (ports: number[], historyLabel: string) => {
    if (!host.trim() || ports.length === 0) return;
    cancelRef.current = false;
    setBusy(true);
    setError(null);
    setResults([]); // pulisce SUBITO i risultati precedenti, prima di mostrare i nuovi
    setProgress({ done: 0, total: ports.length });

    const collected: PortCheckResult[] = [];
    for (let i = 0; i < ports.length; i += PORT_BATCH_SIZE) {
      if (cancelRef.current) return;
      const batch = ports.slice(i, i + PORT_BATCH_SIZE);
      const r = await post<{ results: PortCheckResult[] }>("/api/net/portcheck", { host, ports: batch });
      if (cancelRef.current) return;
      if (!r.ok) {
        setError(r.error.message);
        break;
      }
      collected.push(...r.data.results);
      setResults([...collected]);
      setProgress({ done: Math.min(i + PORT_BATCH_SIZE, ports.length), total: ports.length });
    }
    setBusy(false);
    setHistory(savePortHistory({ host, portsStr: historyLabel }));
  };

  const runManual = (e: React.FormEvent) => {
    e.preventDefault();
    scan(parsePortsInput(portsStr), portsStr);
  };
  const runKnown = () => {
    setPortsStr(WELL_KNOWN_PORTS.join(", "));
    scan(WELL_KNOWN_PORTS, "porte note");
  };
  const runAll = () => {
    setPortsStr("tutte le porte (1-65535)");
    scan(ALL_PORTS, "tutte le porte (1-65535)");
  };
  const stop = () => {
    cancelRef.current = true;
    setBusy(false);
  };
  const useHistoryEntry = (h: PortScanHistoryEntry) => {
    setHost(h.host);
    setPortsStr(h.portsStr);
  };

  // Con pochi risultati mostra anche le porte chiuse (utile per confermarle);
  // con scansioni grandi (tutte le porte / porte note) solo le aperte, altrimenti
  // migliaia di pillole "chiusa" renderebbero la pagina inutilizzabile.
  const showClosed = (results?.length ?? 0) <= 100;
  const displayResults = showClosed ? results : results?.filter((r) => r.open);
  const openCount = results?.filter((r) => r.open).length ?? 0;

  return (
    <div className="net-tool">
      <form className="net-form" onSubmit={runManual}>
        <input value={host} onChange={(e) => setHost(e.target.value)} placeholder="host o IP" />
        <input value={portsStr} onChange={(e) => setPortsStr(e.target.value)} placeholder="22, 80, 443" />
        <button disabled={busy || !host.trim()}>{busy ? "Controllo…" : "Controlla"}</button>
        <button type="button" className="small" disabled={busy || !host.trim()} onClick={runKnown}>
          Porte note
        </button>
        <button type="button" className="small" disabled={busy || !host.trim()} onClick={runAll}>
          Tutte le porte
        </button>
        {busy && (
          <button type="button" className="small danger" onClick={stop}>
            Ferma
          </button>
        )}
      </form>

      {history.length > 0 && (
        <div className="net-history">
          {history.map((h, i) => (
            <button key={i} type="button" className="small ghost" onClick={() => useHistoryEntry(h)}>
              {h.host} · {h.portsStr.length > 28 ? `${h.portsStr.slice(0, 28)}…` : h.portsStr}
            </button>
          ))}
        </div>
      )}

      {busy && (
        <>
          <ProgressBar done={progress.done} total={progress.total} />
          <div className="dim">
            {progress.done}/{progress.total} porte testate…
          </div>
        </>
      )}
      {error && <div className="banner banner-error">{error}</div>}
      {results && !busy && (
        <div className="dim">
          {openCount} porte aperte su {results.length} testate
        </div>
      )}
      {displayResults && displayResults.length > 0 && (
        <div className="net-result port-chips">
          {displayResults.map((r) => (
            <span key={r.port} className={`port-pill ${r.open ? "open" : "closed"}`}>
              {r.port}
              {r.open && PORT_SERVICE_NAMES[r.port] && <> · {PORT_SERVICE_NAMES[r.port]}</>}{" "}
              {r.open ? `· ${r.latencyMs}ms` : "chiusa"}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function Traceroute() {
  const [host, setHost] = useState("");
  const [resolveHostnames, setResolveHostnames] = useState(false);
  const [task, setTask] = useState<TaskInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const run = async () => {
    setBusy(true);
    setError(null);
    const r = await post<TaskInfo>("/api/net/traceroute", { host, resolveHostnames });
    setBusy(false);
    if (r.ok) setTask(r.data);
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
        <button disabled={busy || !host.trim()}>{busy ? "Avvio…" : "Traccia percorso"}</button>
      </form>
      <label className="checkbox">
        <input
          type="checkbox"
          checked={resolveHostnames}
          onChange={(e) => setResolveHostnames(e.target.checked)}
        />
        Risolvi hostname per ogni hop (più lento)
      </label>
      {error && <div className="banner banner-error">{error}</div>}
      {task && <TaskLog key={task.id} task={task} />}
    </div>
  );
}

function Scan({ autoRunSeq }: { autoRunSeq: number }) {
  // Store esterno all'albero React: i risultati restano visibili quando si
  // cambia tool/sezione e tornano finché non si lancia una nuova scansione.
  const hosts = useNetScanStore((s) => s.hosts);
  const setHosts = useNetScanStore((s) => s.setHosts);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async () => {
    setBusy(true);
    setError(null);
    const r = await post<{ hosts: LanHost[] }>("/api/net/scan", {});
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
  const [tool, setTool] = usePageTab(
    "net",
    TOOLS.map((t) => t.id),
    "listen",
  );
  // Auto-run dello scan quando si arriva sul tab "scan" da tray/palette.
  const page = useNavStore((s) => s.page);
  const reqTab = useNavStore((s) => s.tab);
  const seq = useNavStore((s) => s.seq);
  const scanSeq = page === "net" && reqTab === "scan" ? seq : 0;

  return (
    <div>
      <div className="section-header">
        <h2>Rete</h2>
        <Tabs tabs={TOOLS} active={tool} onChange={setTool} />
      </div>
      {tool === "listen" && <ListeningPorts />}
      {tool === "services" && <Services />}
      {tool === "ping" && <Ping />}
      {tool === "dns" && <Dns />}
      {tool === "portcheck" && <Ports />}
      {tool === "traceroute" && <Traceroute />}
      {tool === "scan" && <Scan autoRunSeq={scanSeq} />}
    </div>
  );
}
