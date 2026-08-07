import { useState } from "react";
import { useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { useDisksStore } from "../../stores/disksStore";
import { FormatDialog } from "./FormatDialog";
import type { ApiError, DiskInfo } from "../../lib/types";

function fmtGb(bytes: number) {
  return (bytes / 1024 ** 3).toFixed(bytes < 10 * 1024 ** 3 ? 1 : 0);
}

async function refreshDisksNow() {
  const r = await api<{ disks: DiskInfo[] }>("/api/disks");
  if (r.ok) useDisksStore.getState().setDisks(r.data.disks);
}

function DiskRow({ disk }: { disk: DiskInfo }) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);
  const [formatting, setFormatting] = useState(false);

  const eject = async () => {
    setBusy(true);
    setError(null);
    const r = await post("/api/disks/eject", { mountPoint: disk.mountPoint });
    setBusy(false);
    if (!r.ok) setError(r.error);
    else await refreshDisksNow();
  };

  const barColor =
    disk.usedPct > 90 ? "var(--error)" : disk.usedPct > 75 ? "var(--warn)" : "var(--accent)";

  return (
    <div className="disk-row">
      <div className="disk-head">
        <span className="disk-name">
          {disk.name}
          {disk.isRemovable && <span className="badge badge-app">{t("dashboard.disks.removable")}</span>}
          {disk.isSystem && <span className="badge">{t("dashboard.system")}</span>}
        </span>
        {disk.isRemovable && !disk.isSystem && (
          <span className="disk-actions">
            <button className="small" onClick={eject} disabled={busy}>
              {t("dashboard.disks.eject")}
            </button>
            <button className="small danger" onClick={() => setFormatting(true)} disabled={busy}>
              {t("dashboard.disks.format")}
            </button>
          </span>
        )}
      </div>
      <div className="disk-bar">
        <div className="disk-bar-fill" style={{ width: `${disk.usedPct}%`, background: barColor }} />
      </div>
      <div className="disk-meta">
        {t("dashboard.disks.freeOfTotal", {
          free: fmtGb(disk.availableBytes),
          total: fmtGb(disk.totalBytes),
          fs: disk.fileSystem,
        })}
      </div>
      {error && (
        <div className="banner banner-error">
          {error.message}
          {error.osHint && <div className="hint">{error.osHint}</div>}
        </div>
      )}
      {formatting && (
        <FormatDialog
          disk={disk}
          onClose={(done) => {
            setFormatting(false);
            if (done) refreshDisksNow();
          }}
        />
      )}
    </div>
  );
}

export function Disks() {
  const { t } = useTranslation();
  const disks = useDisksStore((s) => s.disks);

  return (
    <section className="disks">
      <h3>{t("dashboard.disks.title")}</h3>
      {!disks && <div className="empty">{t("dashboard.disks.reading")}</div>}
      {disks && disks.length === 0 && <div className="empty">{t("dashboard.disks.none")}</div>}
      {disks?.map((d) => (
        <DiskRow key={d.mountPoint} disk={d} />
      ))}
    </section>
  );
}
