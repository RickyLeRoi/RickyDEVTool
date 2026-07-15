import { useStatsStore } from "../stores/statsStore";
import { Sparkline } from "../components/Sparkline";

function fmtGb(bytes: number) {
  return (bytes / 1024 ** 3).toFixed(1);
}

/** Pannello destro sempre visibile: CPU/RAM in miniatura. */
export function VitalsPanel() {
  const { latest, cpuHistory, memHistory } = useStatsStore();

  return (
    <aside className="vitals">
      <div className="vitals-block">
        <div className="vitals-row">
          <span className="vitals-label">CPU</span>
          <span className="vitals-value">
            {latest ? `${latest.cpuTotalPct.toFixed(0)}%` : "—"}
          </span>
        </div>
        <Sparkline values={cpuHistory} width={180} height={24} />
        {latest && (
          <div className="vitals-sub">{latest.cores.length} core</div>
        )}
      </div>
      <div className="vitals-block">
        <div className="vitals-row">
          <span className="vitals-label">RAM</span>
          <span className="vitals-value">
            {latest ? `${latest.mem.usedPct.toFixed(0)}%` : "—"}
          </span>
        </div>
        <Sparkline values={memHistory} width={180} height={24} stroke="var(--accent2)" />
        {latest && (
          <div className="vitals-sub">
            {fmtGb(latest.mem.usedBytes)} / {fmtGb(latest.mem.totalBytes)} GB
          </div>
        )}
      </div>
      <div className="vitals-spacer" />
      <div className="vitals-alerts">
        <div className="vitals-label">Alert</div>
        <div className="vitals-sub">Nessun alert (v1)</div>
      </div>
    </aside>
  );
}
