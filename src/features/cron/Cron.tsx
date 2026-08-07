import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api } from "../../lib/api";
import { getLang } from "../../lib/i18n";
import { cronNextRun } from "./cronParse";
import type { SchedEntry, SchedListing } from "../../lib/types";

function CronRow({ entry }: { entry: SchedEntry }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [lines, setLines] = useState<string[] | null>(null);
  const [loading, setLoading] = useState(false);

  const isCron = entry.source === "crontab";
  const next = isCron ? cronNextRun(entry.schedule) : null;

  const toggle = async () => {
    const willOpen = !open;
    setOpen(willOpen);
    if (willOpen && !isCron && lines === null) {
      setLoading(true);
      const r = await api<{ lines: string[] }>(
        `/api/scheduler/detail?source=${encodeURIComponent(entry.source)}&id=${encodeURIComponent(entry.command)}`,
      );
      setLoading(false);
      if (r.ok) setLines(r.data.lines ?? []);
      else setLines([]);
    }
  };

  return (
    <>
      <tr className="cron-row" onClick={toggle}>
        <td className="cron-schedule">{entry.schedule}</td>
        <td>
          <code className="cron-cmd">{entry.command}</code>
          {entry.detail && <div className="dim">{entry.detail}</div>}
        </td>
        <td className="dim cron-source">
          {entry.source}
          <span className="cron-expand">{open ? "▾" : "▸"}</span>
        </td>
      </tr>
      {open && (
        <tr className="cron-detail">
          <td colSpan={3}>
            {isCron ? (
              next ? (
                <div>
                  {t("cron.nextRun")} <strong>{next.toLocaleString(getLang())}</strong>
                </div>
              ) : (
                <div className="dim">{t("cron.notFixed")}</div>
              )
            ) : loading ? (
              <div className="dim">{t("cron.loadingDetails")}</div>
            ) : lines && lines.length > 0 ? (
              <ul className="cron-detail-list">
                {lines.map((l, i) => (
                  <li key={i}>{l}</li>
                ))}
              </ul>
            ) : (
              <div className="dim">{t("cron.noDetails")}</div>
            )}
          </td>
        </tr>
      )}
    </>
  );
}

export function Cron() {
  const { t } = useTranslation();
  const [listing, setListing] = useState<SchedListing | null>(null);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    const r = await api<SchedListing>("/api/scheduler");
    setLoading(false);
    if (r.ok) {
      setListing({
        supported: !!r.data.supported,
        entries: r.data.entries ?? [],
        note: r.data.note ?? null,
      });
    }
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <div className="cron">
      <div className="section-header">
        <h2>{t("cron.title")}</h2>
        <button className="small" onClick={load} disabled={loading}>
          {loading ? "…" : t("common.refresh")}
        </button>
      </div>
      <p className="hint">{t("cron.intro")}</p>

      {listing && listing.entries.length === 0 && (
        <div className="empty">{listing.note ?? t("cron.noEntries")}</div>
      )}

      {listing && listing.entries.length > 0 && (
        <div className="table-scroll">
          <table className="proc-table cron-table">
            <thead>
              <tr>
                <th>{t("cron.colSchedule")}</th>
                <th>{t("cron.colCommand")}</th>
                <th>{t("cron.colSource")}</th>
              </tr>
            </thead>
            <tbody>
              {listing.entries.map((e, i) => (
                <CronRow key={`${e.source}-${i}`} entry={e} />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
