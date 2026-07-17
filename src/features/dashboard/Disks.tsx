import { useState } from "react";
import { post } from "../../lib/api";
import { useDisksStore } from "../../stores/disksStore";
import { FormatDialog } from "./FormatDialog";
import type { ApiError, DiskInfo } from "../../lib/types";

function fmtGb(bytes: number) {
  return (bytes / 1024 ** 3).toFixed(bytes < 10 * 1024 ** 3 ? 1 : 0);
}

function DiskRow({ disk }: { disk: DiskInfo }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);
  const [formatting, setFormatting] = useState(false);

  const eject = async () => {
    setBusy(true);
    setError(null);
    const r = await post("/api/disks/eject", { mountPoint: disk.mountPoint });
    setBusy(false);
    if (!r.ok) setError(r.error);
  };

  const barColor =
    disk.usedPct > 90 ? "var(--error)" : disk.usedPct > 75 ? "var(--warn)" : "var(--accent)";

  return (
    <div className="disk-row">
      <div className="disk-head">
        <span className="disk-name">
          {disk.name}
          {disk.isRemovable && <span className="badge badge-app">rimovibile</span>}
          {disk.isSystem && <span className="badge">sistema</span>}
        </span>
        {disk.isRemovable && !disk.isSystem && (
          <span className="disk-actions">
            <button className="small" onClick={eject} disabled={busy}>
              Espelli
            </button>
            <button className="small danger" onClick={() => setFormatting(true)} disabled={busy}>
              Formatta
            </button>
          </span>
        )}
      </div>
      <div className="disk-bar">
        <div className="disk-bar-fill" style={{ width: `${disk.usedPct}%`, background: barColor }} />
      </div>
      <div className="disk-meta">
        {fmtGb(disk.availableBytes)} GB liberi di {fmtGb(disk.totalBytes)} GB · {disk.fileSystem}
      </div>
      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}
      {formatting && <FormatDialog disk={disk} onClose={() => setFormatting(false)} />}
    </div>
  );
}

export function Disks() {
  const disks = useDisksStore((s) => s.disks);

  return (
    <section className="disks">
      <h3>Dischi</h3>
      {!disks && <div className="empty">Lettura dischi…</div>}
      {disks && disks.length === 0 && <div className="empty">Nessun disco rilevato.</div>}
      {disks?.map((d) => (
        <DiskRow key={d.mountPoint} disk={d} />
      ))}
    </section>
  );
}
