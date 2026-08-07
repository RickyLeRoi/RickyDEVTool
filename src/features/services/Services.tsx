import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { ws } from "../../lib/ws";
import type { ServiceDef, ServiceState, ServiceStatus } from "../../lib/types";

function StateDot({ state }: { state: ServiceState }) {
  return <span className={`state-dot ${state}`} />;
}

function CertBadge({ daysLeft }: { daysLeft: number | null }) {
  const { t } = useTranslation();
  if (daysLeft === null) return null;
  if (daysLeft > 21) return null;
  if (daysLeft < 0) {
    return (
      <span className="badge badge-cert-expired" title={t("services.certExpiredTitle")}>
        {t("services.certExpired")}
      </span>
    );
  }
  return (
    <span className="badge badge-warn" title={t("services.certExpiringTitle")}>
      {t("services.certExpiring", { days: daysLeft })}
    </span>
  );
}

function HistoryBar({ history }: { history: ServiceState[] }) {
  const { t } = useTranslation();
  return (
    <span className="history-bar" title={t("services.lastChecks")}>
      {history.map((s, i) => (
        <span key={i} className={`history-cell ${s}`} />
      ))}
    </span>
  );
}

export function Services() {
  const { t } = useTranslation();
  const [statuses, setStatuses] = useState<ServiceStatus[] | null>(null);
  const [defs, setDefs] = useState<ServiceDef[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [showConfig, setShowConfig] = useState(false);
  const [form, setForm] = useState({ label: "", target: "", kind: "http" as "http" | "tcp" });
  const [editingId, setEditingId] = useState<string | null>(null);

  const loadDefs = async () => {
    const r = await api<{ services: ServiceDef[] }>("/api/services");
    if (r.ok) setDefs(r.data.services);
  };

  useEffect(() => {
    loadDefs();
    return ws.subscribe("services", (event) => {
      if (event.topic === "services")
        setStatuses((event.payload as { statuses: ServiceStatus[] }).statuses);
      else if (event.topic === "services:error")
        setError((event.payload as { message: string }).message);
    });
  }, []);

  const toggle = async (id: string) => {
    const r = await post<{ services: ServiceDef[] }>(`/api/services/${id}/toggle`, {});
    if (r.ok) setDefs(r.data.services);
  };

  const remove = async (id: string) => {
    const r = await api<{ services: ServiceDef[] }>(`/api/services/${id}`, { method: "DELETE" });
    if (r.ok) setDefs(r.data.services);
    if (id === editingId) cancelEdit();
  };

  const startEdit = (d: ServiceDef) => {
    if (d.builtin) return;
    setEditingId(d.id);
    setForm({ label: d.label, target: d.target, kind: d.kind });
  };

  const cancelEdit = () => {
    setEditingId(null);
    setForm({ label: "", target: "", kind: "http" });
  };

  const submitForm = async (e: React.FormEvent) => {
    e.preventDefault();
    const existing = editingId ? defs.find((d) => d.id === editingId) : undefined;
    const id = editingId ?? form.label.toLowerCase().replace(/[^a-z0-9]+/g, "-");
    const r = await post<{ services: ServiceDef[] }>("/api/services", {
      id,
      label: form.label,
      kind: form.kind,
      target: form.target,
      timeoutMs: existing?.timeoutMs ?? 4000,
      builtin: false,
      enabled: existing?.enabled ?? true,
    });
    if (r.ok) {
      setDefs(r.data.services);
      setEditingId(null);
      setForm({ label: "", target: "", kind: "http" });
    }
  };

  const statusById = new Map((statuses ?? []).map((s) => [s.id, s]));

  return (
    <div>
      <div className="section-header">
        <h2>
          {t("services.title")}
          <span className="live-dot" title={t("services.live")} />
        </h2>
        <button onClick={() => setShowConfig(!showConfig)}>
          {showConfig ? t("common.close") : t("services.configure")}
        </button>
      </div>

      {error && (
        <div className="banner banner-error">{t("services.checkError", { message: error })}</div>
      )}
      {!statuses && !error && <div className="empty">{t("services.firstCheck")}</div>}

      {statuses && (
        <table className="proc-table">
          <tbody>
            {defs.filter((d) => d.enabled).map((def) => {
              const s = statusById.get(def.id);
              return (
                <tr key={def.id} title={def.target}>
                  <td>
                    <StateDot state={s?.state ?? "down"} /> {def.label}
                    <CertBadge daysLeft={s?.certDaysLeft ?? null} />
                  </td>
                  <td className="num dim">
                    {s?.latencyMs != null ? `${s.latencyMs} ms` : s?.error ?? t("common.none")}
                  </td>
                  <td className="num">
                    <HistoryBar history={s?.history ?? []} />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}

      {showConfig && (
        <div className="services-config">
          <h3>{t("services.configuration")}</h3>
          <table className="proc-table">
            <tbody>
              {defs.map((d) => (
                <tr
                  key={d.id}
                  title={d.builtin ? d.target : t("services.editTitle", { target: d.target })}
                  className={d.id === editingId ? "row-editing" : d.builtin ? undefined : "row-clickable"}
                  onClick={() => startEdit(d)}
                >
                  <td>
                    {d.label}
                    {d.builtin && <span className="badge">{t("services.preset")}</span>}
                    <span className="dim"> · {d.kind}</span>
                  </td>
                  <td className="num">
                    <button
                      className="small"
                      onClick={(e) => {
                        e.stopPropagation();
                        toggle(d.id);
                      }}
                    >
                      {d.enabled ? t("services.deactivate") : t("services.activate")}
                    </button>{" "}
                    {!d.builtin && (
                      <button
                        className="small danger"
                        onClick={(e) => {
                          e.stopPropagation();
                          remove(d.id);
                        }}
                      >
                        {t("common.delete")}
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <form className="service-form" onSubmit={submitForm}>
            <input
              placeholder={t("services.namePlaceholder")}
              value={form.label}
              onChange={(e) => setForm({ ...form, label: e.target.value })}
              required
            />
            <select
              value={form.kind}
              onChange={(e) => setForm({ ...form, kind: e.target.value as "http" | "tcp" })}
            >
              <option value="http">HTTP</option>
              <option value="tcp">TCP</option>
            </select>
            <input
              placeholder={
                form.kind === "http"
                  ? t("services.httpPlaceholder")
                  : t("services.tcpPlaceholder")
              }
              value={form.target}
              onChange={(e) => setForm({ ...form, target: e.target.value })}
              required
            />
            <button type="submit">{editingId ? t("common.save") : t("common.add")}</button>
            {editingId && (
              <button type="button" className="small" onClick={cancelEdit}>
                {t("common.cancel")}
              </button>
            )}
          </form>
          <p className="hint">{t("services.cloudflaredHint")}</p>
        </div>
      )}
    </div>
  );
}
