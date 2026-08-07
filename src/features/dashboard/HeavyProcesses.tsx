import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../../lib/api";
import { KNOWN_APP_LABELS } from "../../lib/constants";
import type { ApiError, HeavyProcessesResult, ProcessGroup } from "../../lib/types";

function fmtMem(bytes: number) {
  const mb = bytes / 1024 ** 2;
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb.toFixed(0)} MB`;
}

function GroupRow({ group }: { group: ProcessGroup }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const multi = group.count > 1;

  return (
    <>
      <tr
        className={multi ? "port-row" : undefined}
        onClick={multi ? () => setOpen(!open) : undefined}
        title={!multi ? group.members[0]?.exePath ?? undefined : undefined}
      >
        <td>
          {group.name}
          {group.knownApp && (
            <span className="badge badge-app">{KNOWN_APP_LABELS[group.knownApp] ?? group.knownApp}</span>
          )}
          {group.isSystem && <span className="badge">{t("dashboard.system")}</span>}
          {multi && <span className="dim"> · {t("dashboard.heavy.procCount", { count: group.count })}</span>}
        </td>
        <td className="num">{multi ? t("common.none") : group.members[0]?.pid}</td>
        <td>{multi ? t("common.none") : group.members[0]?.user ?? t("common.none")}</td>
        <td className="num">{group.cpuPct.toFixed(1)}%</td>
        <td className="num">{group.memPct.toFixed(1)}%</td>
        <td className="num">
          {fmtMem(group.memBytes)}
          {multi && <span className="dim"> {open ? "▾" : "▸"}</span>}
        </td>
      </tr>
      {multi && open && (
        <tr className="port-detail">
          <td colSpan={6}>
            <table className="proc-table inner">
              <tbody>
                {group.members.map((p) => (
                  <tr key={p.pid} title={p.exePath ?? undefined}>
                    <td className="dim">PID {p.pid}</td>
                    <td>{p.user ?? t("common.none")}</td>
                    <td className="num">{p.cpuPct.toFixed(1)}%</td>
                    <td className="num">{p.memPct.toFixed(1)}%</td>
                    <td className="num">{fmtMem(p.memBytes)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </td>
        </tr>
      )}
    </>
  );
}

export function HeavyProcesses() {
  const { t } = useTranslation();
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
        <h3>{t("dashboard.heavy.title")}</h3>
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
            {loading
              ? t("dashboard.heavy.sampling")
              : result
                ? t("common.refresh")
                : t("dashboard.heavy.analyze")}
          </button>
        </div>
      </div>

      <p className="hint">{t("dashboard.heavy.multiProcHint")}</p>

      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}

      {result && result.groups.length === 0 && (
        <div className="empty">
          {t("dashboard.heavy.noneAboveThreshold", {
            cpu: result.cpuMinPct,
            mem: result.memMinPct,
          })}
        </div>
      )}

      {result && result.groups.length > 0 && (
        <table className="proc-table">
          <thead>
            <tr>
              <th>{t("dashboard.heavy.colProcess")}</th>
              <th>PID</th>
              <th>{t("dashboard.heavy.colUser")}</th>
              <th className="num">CPU</th>
              <th className="num">RAM</th>
              <th className="num">Mem</th>
            </tr>
          </thead>
          <tbody>
            {result.groups.map((g) => (
              <GroupRow key={g.name} group={g} />
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}
