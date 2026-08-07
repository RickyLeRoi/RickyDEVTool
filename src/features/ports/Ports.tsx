import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { ws } from "../../lib/ws";
import { API_BASE, post } from "../../lib/api";
import { usePortsStore } from "../../stores/portsStore";
import { KillDialog } from "./KillDialog";
import { isTauri } from "../../lib/appWindow";
import { KNOWN_APP_LABELS, LOOPBACK_HOST, STORAGE_KEYS } from "../../lib/constants";
import { DEFAULT_PORTS_GROUPING } from "../../lib/defaults";
import type { PortEntry, PortProcess, PortScan } from "../../lib/types";

type Grouping = "port" | "process";

function initialGrouping(): Grouping {
  return localStorage.getItem(STORAGE_KEYS.portsGrouping) === "process"
    ? "process"
    : DEFAULT_PORTS_GROUPING;
}

function openPortInBrowser(port: number) {
  const base = API_BASE || window.location.origin;
  const hostname = new URL(base).hostname;
  const host = hostname === LOOPBACK_HOST ? "localhost" : hostname;
  const url = `http://${host}:${port}`;
  if (isTauri) {
    post("/api/system/open-url", { url });
  } else {
    window.open(url, "_blank");
  }
}

function AppBadge({ app }: { app: string | null }) {
  if (!app) return null;
  return <span className="badge badge-app">{KNOWN_APP_LABELS[app] ?? app}</span>;
}

interface ProcessPorts {
  pid: number;
  proc: PortProcess;
  ports: PortEntry[];
  hasZombie: boolean;
}

function groupByProcess(ports: PortEntry[]): ProcessPorts[] {
  const map = new Map<number, ProcessPorts>();
  for (const entry of ports) {
    for (const p of entry.processes) {
      let g = map.get(p.pid);
      if (!g) {
        g = { pid: p.pid, proc: p, ports: [], hasZombie: false };
        map.set(p.pid, g);
      }
      g.ports.push(entry);
      if (p.zombie) g.hasZombie = true;
    }
  }
  return [...map.values()].sort(
    (a, b) => a.proc.name.localeCompare(b.proc.name) || a.pid - b.pid,
  );
}

function PortRow({ entry }: { entry: PortEntry }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [killing, setKilling] = useState<PortProcess | null>(null);

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
              {p.zombie && (
                <span className="badge badge-warn" title={t("ports.zombieTitle")}>
                  {t("ports.zombie")}
                </span>
              )}
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
                {t("ports.copyLocalhost", { port: entry.port })}
              </button>
              <button onClick={() => openPortInBrowser(entry.port)}>
                {t("common.openInBrowser")}
              </button>
            </div>
            <table className="proc-table inner">
              <tbody>
                {entry.processes.map((p) => (
                  <tr key={p.pid} title={p.exePath ?? undefined}>
                    <td>
                      {p.name}
                      <AppBadge app={p.knownApp} />
                      {p.killProtection === "typed-confirm" && (
                        <span className="badge badge-warn">{t("ports.protected")}</span>
                      )}
                      {p.zombie && (
                        <span className="badge badge-warn" title={t("ports.zombieTitleDetail")}>
                          {t("ports.zombie")}
                        </span>
                      )}
                    </td>
                    <td className="num">PID {p.pid}</td>
                    <td>{p.user ?? t("common.none")}</td>
                    <td className="num">
                      <button className="danger small" onClick={() => setKilling(p)}>
                        {t("ports.kill")}
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

function ProcessRow({ group }: { group: ProcessPorts }) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(false);
  const [killing, setKilling] = useState<PortProcess | null>(null);
  const p = group.proc;

  return (
    <>
      <tr className="port-row" onClick={() => setExpanded(!expanded)} title={p.exePath ?? undefined}>
        <td>
          {p.name}
          <AppBadge app={p.knownApp} />
          {p.killProtection === "typed-confirm" && (
            <span className="badge badge-warn">{t("ports.protected")}</span>
          )}
          {group.hasZombie && (
            <span className="badge badge-warn" title={t("ports.zombieTitleDetail")}>
              {t("ports.zombie")}
            </span>
          )}
        </td>
        <td className="num">PID {p.pid}</td>
        <td>{p.user ?? t("common.none")}</td>
        <td>
          {group.ports.map((entry) => (
            <span key={`${entry.protocol}-${entry.port}`} className="proc-chip">
              {entry.port}
            </span>
          ))}
        </td>
        <td className="num dim">{expanded ? "▾" : "▸"}</td>
      </tr>
      {expanded && (
        <tr className="port-detail">
          <td colSpan={5}>
            <table className="proc-table inner">
              <tbody>
                {group.ports.map((entry) => (
                  <tr key={`${entry.protocol}-${entry.port}`}>
                    <td className="num port-number">{entry.port}</td>
                    <td>{entry.protocol}</td>
                    <td className="dim">{entry.addresses.join(", ")}</td>
                    <td className="num">
                      <div className="port-actions">
                        <button
                          onClick={() =>
                            navigator.clipboard.writeText(`localhost:${entry.port}`)
                          }
                        >
                          {t("ports.copyLocalhost", { port: entry.port })}
                        </button>
                        <button onClick={() => openPortInBrowser(entry.port)}>
                          {t("common.openInBrowser")}
                        </button>
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <div className="port-actions">
              <button className="danger small" onClick={() => setKilling(p)}>
                {t("ports.kill")}
              </button>
            </div>
          </td>
        </tr>
      )}
      {killing && <KillDialog process={killing} onClose={() => setKilling(null)} />}
    </>
  );
}

export function Ports() {
  const { t } = useTranslation();
  const { scan, error, setScan, setError } = usePortsStore();
  const [grouping, setGrouping] = useState<Grouping>(initialGrouping);

  const chooseGrouping = (g: Grouping) => {
    localStorage.setItem(STORAGE_KEYS.portsGrouping, g);
    setGrouping(g);
  };

  useEffect(() => {
    return ws.subscribe("ports", (event) => {
      if (event.topic === "ports") setScan(event.payload as PortScan);
      else if (event.topic === "ports:error")
        setError((event.payload as { message: string }).message);
    });
  }, [setScan, setError]);

  const processes = useMemo(
    () => (scan ? groupByProcess(scan.ports) : []),
    [scan],
  );

  return (
    <div>
      <div className="section-header">
        <h2>
          {t("ports.title")}
          <span className="live-dot" title={t("ports.live")} />
        </h2>
        {scan && (
          <span className="dim">
            {t("ports.summary", { count: scan.ports.length, hidden: scan.hiddenSystem })}
          </span>
        )}
      </div>

      <div className="ports-toolbar">
        <span className="dim">{t("ports.groupBy")}</span>
        <div className="segmented">
          <button
            className={grouping === "port" ? "active" : ""}
            onClick={() => chooseGrouping("port")}
          >
            {t("ports.groupByPort")}
          </button>
          <button
            className={grouping === "process" ? "active" : ""}
            onClick={() => chooseGrouping("process")}
          >
            {t("ports.groupByProcess")}
          </button>
        </div>
      </div>

      {error && <div className="banner banner-error">{t("ports.scanError", { message: error })}</div>}
      {!scan && !error && <div className="empty">{t("ports.scanning")}</div>}
      {scan && scan.ports.length === 0 && (
        <div className="empty">{t("ports.emptyNonSystem")}</div>
      )}

      {scan && scan.ports.length > 0 && grouping === "port" && (
        <table className="proc-table">
          <thead>
            <tr>
              <th className="num">{t("ports.colPort")}</th>
              <th>{t("ports.colProto")}</th>
              <th>{t("ports.colBind")}</th>
              <th>{t("ports.colProcesses")}</th>
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

      {scan && scan.ports.length > 0 && grouping === "process" && (
        <table className="proc-table">
          <thead>
            <tr>
              <th>{t("ports.colProcess")}</th>
              <th className="num">PID</th>
              <th>{t("ports.colUser")}</th>
              <th>{t("ports.colPorts")}</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {processes.map((group) => (
              <ProcessRow key={group.pid} group={group} />
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
