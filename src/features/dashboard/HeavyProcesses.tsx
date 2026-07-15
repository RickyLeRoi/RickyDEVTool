import { useState } from "react";
import { api } from "../../lib/api";
import type { ApiError, HeavyProcessesResult } from "../../lib/types";

// Etichette compatte per le app note (icone SVG dedicate in v1).
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

function fmtMem(bytes: number) {
  const mb = bytes / 1024 ** 2;
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb.toFixed(0)} MB`;
}

export function HeavyProcesses() {
  const [cpuMin, setCpuMin] = useState(20);
  const [memMin, setMemMin] = useState(10);
  const [result, setResult] = useState<HeavyProcessesResult | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);

  const load = async () => {
    setLoading(true);
    setError(null);
    const r = await api<HeavyProcessesResult>(
      `/api/processes/heavy?cpuMin=${cpuMin}&memMin=${memMin}`,
    );
    setLoading(false);
    if (r.ok) setResult(r.data);
    else setError(r.error);
  };

  return (
    <section className="heavy">
      <div className="heavy-header">
        <h3>Processi pesanti</h3>
        <div className="heavy-controls">
          <label>
            CPU &gt;
            <input
              type="number"
              min={0}
              max={100}
              value={cpuMin}
              onChange={(e) => setCpuMin(Number(e.target.value))}
            />
            %
          </label>
          <label>
            RAM &gt;
            <input
              type="number"
              min={0}
              max={100}
              value={memMin}
              onChange={(e) => setMemMin(Number(e.target.value))}
            />
            %
          </label>
          <button onClick={load} disabled={loading}>
            {loading ? "Campiono…" : result ? "Aggiorna" : "Analizza"}
          </button>
        </div>
      </div>

      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}

      {result && result.processes.length === 0 && (
        <div className="empty">
          Nessun processo sopra soglia ({result.cpuMinPct}% CPU / {result.memMinPct}% RAM).
        </div>
      )}

      {result && result.processes.length > 0 && (
        <table className="proc-table">
          <thead>
            <tr>
              <th>Processo</th>
              <th>PID</th>
              <th>Utente</th>
              <th className="num">CPU</th>
              <th className="num">RAM</th>
              <th className="num">Mem</th>
            </tr>
          </thead>
          <tbody>
            {result.processes.map((p) => (
              <tr key={p.pid} title={p.exePath ?? undefined}>
                <td>
                  {p.name}
                  {p.knownApp && (
                    <span className="badge badge-app">
                      {KNOWN_LABELS[p.knownApp] ?? p.knownApp}
                    </span>
                  )}
                  {p.isSystem && <span className="badge">sistema</span>}
                </td>
                <td className="num">{p.pid}</td>
                <td>{p.user ?? "—"}</td>
                <td className="num">{p.cpuPct.toFixed(1)}%</td>
                <td className="num">{p.memPct.toFixed(1)}%</td>
                <td className="num">{fmtMem(p.memBytes)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
