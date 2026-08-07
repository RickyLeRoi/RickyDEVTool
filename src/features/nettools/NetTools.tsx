import { useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { post } from "../../lib/api";
import { useNavStore } from "../../stores/navStore";
import { useNetScanStore } from "../../stores/netScanStore";
import { Tabs, usePageTab, type TabDef } from "../../components/Tabs";
import { Services } from "../services/Services";
import { Ports as ListeningPorts } from "../ports/Ports";
import { Docker } from "../docker/Docker";
import { TaskLog } from "../../components/TaskLog";
import {
  ALL_PORTS,
  NET_TAB_IDS,
  PING_ATTEMPTS,
  PORT_MAX,
  PORT_MIN,
  PORT_SCAN_BATCH_SIZE,
  PORT_SCAN_HISTORY_LABEL_CHARS,
  PORT_SCAN_HISTORY_MAX,
  PORT_SCAN_SHOW_CLOSED_MAX,
  PORT_SERVICE_NAMES,
  STORAGE_KEYS,
  WELL_KNOWN_PORTS,
} from "../../lib/constants";
import { DEFAULT_NET_TAB, DEFAULT_PING_HOST, DEFAULT_PORT_SCAN_PORTS } from "../../lib/defaults";
import type { DnsRecordSet, LanHost, PingResult, PortCheckResult, TaskInfo } from "../../lib/types";

function ProgressBar({ done, total }: { done: number; total: number }) {
  const pct = total > 0 ? Math.min(100, (done / total) * 100) : 0;
  return (
    <div className="progress-bar">
      <div className="progress-bar-fill" style={{ width: `${pct}%` }} />
    </div>
  );
}

function Ping() {
  const { t } = useTranslation();
  const [host, setHost] = useState(DEFAULT_PING_HOST);
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
        <input
          value={host}
          onChange={(e) => setHost(e.target.value)}
          placeholder={t("net.hostPlaceholder")}
        />
        <button disabled={busy}>
          {busy
            ? t("net.pingBusy", { done: attempts.length, total: PING_ATTEMPTS })
            : t("net.pingBtn")}
        </button>
      </form>
      {busy && <ProgressBar done={attempts.length} total={PING_ATTEMPTS} />}
      {error && <div className="banner banner-error">{error}</div>}
      {attempts.length > 0 && (
        <div className="net-result">
          <div>
            {t("net.responses", { received, total: attempts.length })}
            {avg != null && t("net.avg", { avg: avg.toFixed(1) })}
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
  const { t } = useTranslation();
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
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder={t("net.dnsPlaceholder")}
        />
        <button disabled={busy || !name.trim()}>
          {busy ? t("net.resolving") : t("net.resolve")}
        </button>
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
                <td className="dim">{t("net.noRecords")}</td>
              </tr>
            )}
          </tbody>
        </table>
      )}
    </div>
  );
}

interface PortScanHistoryEntry {
  host: string;
  portsStr: string;
}

function loadPortHistory(): PortScanHistoryEntry[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(STORAGE_KEYS.portScanHistory) ?? "[]");
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function savePortHistory(entry: PortScanHistoryEntry): PortScanHistoryEntry[] {
  const prev = loadPortHistory().filter(
    (h) => !(h.host === entry.host && h.portsStr === entry.portsStr),
  );
  const next = [entry, ...prev].slice(0, PORT_SCAN_HISTORY_MAX);
  localStorage.setItem(STORAGE_KEYS.portScanHistory, JSON.stringify(next));
  return next;
}

function parsePortsInput(input: string): number[] {
  const set = new Set<number>();
  for (const token of input.split(/[\s,]+/)) {
    const n = parseInt(token, 10);
    if (n >= PORT_MIN && n <= PORT_MAX) set.add(n);
  }
  return [...set].sort((a, b) => a - b);
}

function Ports() {
  const { t } = useTranslation();
  const [host, setHost] = useState("");
  const [portsStr, setPortsStr] = useState(DEFAULT_PORT_SCAN_PORTS);
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
    setResults([]);
    setProgress({ done: 0, total: ports.length });

    const collected: PortCheckResult[] = [];
    for (let i = 0; i < ports.length; i += PORT_SCAN_BATCH_SIZE) {
      if (cancelRef.current) return;
      const batch = ports.slice(i, i + PORT_SCAN_BATCH_SIZE);
      const r = await post<{ results: PortCheckResult[] }>("/api/net/portcheck", { host, ports: batch });
      if (cancelRef.current) return;
      if (!r.ok) {
        setError(r.error.message);
        break;
      }
      collected.push(...r.data.results);
      setResults([...collected]);
      setProgress({ done: Math.min(i + PORT_SCAN_BATCH_SIZE, ports.length), total: ports.length });
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
    scan(WELL_KNOWN_PORTS, t("net.historyKnownPorts"));
  };
  const runAll = () => {
    setPortsStr(t("net.historyAllPorts"));
    scan(ALL_PORTS, t("net.historyAllPorts"));
  };
  const stop = () => {
    cancelRef.current = true;
    setBusy(false);
  };
  const useHistoryEntry = (h: PortScanHistoryEntry) => {
    setHost(h.host);
    setPortsStr(h.portsStr);
  };

  const showClosed = (results?.length ?? 0) <= PORT_SCAN_SHOW_CLOSED_MAX;
  const displayResults = showClosed ? results : results?.filter((r) => r.open);
  const openCount = results?.filter((r) => r.open).length ?? 0;

  return (
    <div className="net-tool">
      <form className="net-form" onSubmit={runManual}>
        <input
          value={host}
          onChange={(e) => setHost(e.target.value)}
          placeholder={t("net.hostPlaceholder")}
        />
        <input
          value={portsStr}
          onChange={(e) => setPortsStr(e.target.value)}
          placeholder={t("net.portsPlaceholder")}
        />
        <button disabled={busy || !host.trim()}>{busy ? t("net.checking") : t("net.check")}</button>
        <button type="button" className="small" disabled={busy || !host.trim()} onClick={runKnown}>
          {t("net.knownPorts")}
        </button>
        <button type="button" className="small" disabled={busy || !host.trim()} onClick={runAll}>
          {t("net.allPorts")}
        </button>
        {busy && (
          <button type="button" className="small danger" onClick={stop}>
            {t("net.stop")}
          </button>
        )}
      </form>

      {history.length > 0 && (
        <div className="net-history">
          {history.map((h, i) => (
            <button key={i} type="button" className="small ghost" onClick={() => useHistoryEntry(h)}>
              {h.host} ·{" "}
              {h.portsStr.length > PORT_SCAN_HISTORY_LABEL_CHARS
                ? `${h.portsStr.slice(0, PORT_SCAN_HISTORY_LABEL_CHARS)}…`
                : h.portsStr}
            </button>
          ))}
        </div>
      )}

      {busy && (
        <>
          <ProgressBar done={progress.done} total={progress.total} />
          <div className="dim">
            {t("net.portsTestedProgress", { done: progress.done, total: progress.total })}
          </div>
        </>
      )}
      {error && <div className="banner banner-error">{error}</div>}
      {results && !busy && (
        <div className="dim">
          {t("net.openOfTested", { open: openCount, total: results.length })}
        </div>
      )}
      {displayResults && displayResults.length > 0 && (
        <div className="net-result port-chips">
          {displayResults.map((r) => (
            <span key={r.port} className={`port-pill ${r.open ? "open" : "closed"}`}>
              {r.port}
              {r.open && PORT_SERVICE_NAMES[r.port] && <> · {PORT_SERVICE_NAMES[r.port]}</>}{" "}
              {r.open ? `· ${r.latencyMs}ms` : t("net.closed")}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function Traceroute() {
  const { t } = useTranslation();
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
        <input
          value={host}
          onChange={(e) => setHost(e.target.value)}
          placeholder={t("net.hostPlaceholder")}
        />
        <button disabled={busy || !host.trim()}>
          {busy ? t("net.starting") : t("net.traceBtn")}
        </button>
      </form>
      <label className="checkbox">
        <input
          type="checkbox"
          checked={resolveHostnames}
          onChange={(e) => setResolveHostnames(e.target.checked)}
        />
        {t("net.resolveHops")}
      </label>
      {error && <div className="banner banner-error">{error}</div>}
      {task && <TaskLog key={task.id} task={task} />}
    </div>
  );
}

export function commonDomain(hostnames: (string | null)[]): string | null {
  const domains = hostnames
    .filter((h): h is string => !!h)
    .map((h) => {
      const dot = h.indexOf(".");
      return dot > 0 ? h.slice(dot + 1) : null;
    });
  if (domains.length < 2 || domains.some((d) => !d)) return null;
  const first = domains[0]!.toLowerCase();
  return domains.every((d) => d!.toLowerCase() === first) ? domains[0] : null;
}

function shortHostname(hostname: string | null, domain: string | null): string {
  if (!hostname) return "—";
  if (!domain) return hostname;
  const suffix = `.${domain}`;
  return hostname.toLowerCase().endsWith(suffix.toLowerCase())
    ? hostname.slice(0, hostname.length - suffix.length)
    : hostname;
}

function Scan({ autoRunSeq }: { autoRunSeq: number }) {
  const { t } = useTranslation();
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

  useEffect(() => {
    if (autoRunSeq === 0) return;
    run();
  }, [autoRunSeq]);

  const domain = useMemo(() => commonDomain((hosts ?? []).map((h) => h.hostname)), [hosts]);

  return (
    <div className="net-tool">
      <button disabled={busy} onClick={run}>
        {busy ? t("net.scanning") : t("net.scanBtn")}
      </button>
      {error && <div className="banner banner-error">{error}</div>}
      {hosts && (
        <table className="proc-table net-result">
          <thead>
            <tr>
              <th>IP</th>
              <th>
                {t("net.colHostname")}
                {domain && <span className="dim th-note"> · .{domain}</span>}
              </th>
              <th>MAC</th>
              <th className="num">{t("net.colPing")}</th>
            </tr>
          </thead>
          <tbody>
            {hosts.map((h) => (
              <tr key={h.ip}>
                <td>
                  {h.ip}
                  {h.isSelf && <span className="badge">{t("net.thisPc")}</span>}
                </td>
                <td className="dim" title={h.hostname ?? undefined}>
                  {shortHostname(h.hostname, domain)}
                </td>
                <td className="dim">{h.mac ?? t("common.none")}</td>
                <td className="num dim">{h.latencyMs != null ? `${h.latencyMs.toFixed(0)}ms` : t("common.none")}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}

const TAB_LABEL_KEYS: Record<(typeof NET_TAB_IDS)[number], string> = {
  listen: "net.tabListen",
  services: "net.tabServices",
  ping: "net.tabPing",
  dns: "net.tabDns",
  portcheck: "net.tabPortcheck",
  traceroute: "net.tabTraceroute",
  scan: "net.tabScan",
  docker: "net.tabDocker",
};

export function NetTools() {
  const { t } = useTranslation();
  const [tool, setTool] = usePageTab("net", [...NET_TAB_IDS], DEFAULT_NET_TAB);
  const page = useNavStore((s) => s.page);
  const reqTab = useNavStore((s) => s.tab);
  const seq = useNavStore((s) => s.seq);
  const scanSeq = page === "net" && reqTab === "scan" ? seq : 0;

  const tabs: TabDef[] = NET_TAB_IDS.map((id) => ({
    id,
    label: t(TAB_LABEL_KEYS[id] as "net.tabListen"),
  }));

  return (
    <div>
      <div className="section-header">
        <h2>{t("net.title")}</h2>
        <Tabs tabs={tabs} active={tool} onChange={setTool} />
      </div>
      {tool === "listen" && <ListeningPorts />}
      {tool === "services" && <Services />}
      {tool === "ping" && <Ping />}
      {tool === "dns" && <Dns />}
      {tool === "portcheck" && <Ports />}
      {tool === "traceroute" && <Traceroute />}
      {tool === "scan" && <Scan autoRunSeq={scanSeq} />}
      {tool === "docker" && <Docker />}
    </div>
  );
}
