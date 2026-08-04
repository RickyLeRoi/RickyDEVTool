import { useCallback, useEffect, useState } from "react";
import { api, post } from "../../lib/api";
import { fmtBytes } from "../../lib/format";
import type { EnvContent, EnvFile } from "../../lib/types";

function EnvValues({ path, file }: { path: string; file: string }) {
  const [content, setContent] = useState<EnvContent | null>(null);
  const [reveal, setReveal] = useState(false);

  useEffect(() => {
    setContent(null);
    setReveal(false);
    api<EnvContent>(`/api/env/read?path=${encodeURIComponent(path)}&file=${encodeURIComponent(file)}`).then(
      (r) => {
        if (r.ok) setContent(r.data);
      },
    );
  }, [path, file]);

  if (!content) return <div className="dim">Lettura…</div>;

  return (
    <div className="env-values">
      <button className="small" onClick={() => setReveal(!reveal)}>
        {reveal ? "Nascondi valori" : "Mostra valori"}
      </button>
      <table className="proc-table">
        <tbody>
          {content.entries.map((e, i) =>
            e.raw != null ? (
              <tr key={i}>
                <td colSpan={2} className="dim env-raw">
                  {e.raw}
                </td>
              </tr>
            ) : (
              <tr key={i}>
                <td className="env-key">{e.key}</td>
                <td className="env-val">{reveal ? e.value : "•".repeat(Math.min(e.value.length, 12))}</td>
              </tr>
            ),
          )}
        </tbody>
      </table>
    </div>
  );
}

export function EnvPanel({ path }: { path: string }) {
  const [files, setFiles] = useState<EnvFile[] | null>(null);
  const [open, setOpen] = useState<string | null>(null);

  const load = useCallback(async () => {
    const r = await api<{ files: EnvFile[] }>(`/api/env/files?path=${encodeURIComponent(path)}`);
    if (r.ok) setFiles(r.data.files);
  }, [path]);

  useEffect(() => {
    load();
  }, [load]);

  const activate = async (file: string) => {
    const r = await post<{ files: EnvFile[] }>("/api/env/activate", { path, file });
    if (r.ok) setFiles(r.data.files);
  };

  if (!files || files.length === 0) return null;

  return (
    <div className="env-panel">
      <h3>File .env</h3>
      <table className="proc-table">
        <tbody>
          {files.map((f) => (
            <tr key={f.name}>
              <td>
                <button className="link-btn" onClick={() => setOpen(open === f.name ? null : f.name)}>
                  {f.name}
                </button>
                {f.isActive && <span className="badge badge-ok">attivo</span>}
              </td>
              <td className="num dim">{fmtBytes(f.sizeBytes)}</td>
              <td className="num">
                {!f.isActive && f.name !== ".env" && (
                  <button className="small" onClick={() => activate(f.name)} title="Copia su .env (con backup)">
                    Attiva
                  </button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {open && <EnvValues path={path} file={open} />}
    </div>
  );
}
