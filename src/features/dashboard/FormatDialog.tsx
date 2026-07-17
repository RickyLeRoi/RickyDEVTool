import { useState } from "react";
import { post } from "../../lib/api";
import type { ApiError, DiskInfo } from "../../lib/types";

const FILESYSTEMS = [
  { id: "exfat", label: "ExFAT (universale)" },
  { id: "fat32", label: "FAT32 (max 4GB/file)" },
  { id: "apfs", label: "APFS (solo Mac)" },
  { id: "hfs+", label: "Mac OS Extended (HFS+)" },
];

export function FormatDialog({
  disk,
  onClose,
}: {
  disk: DiskInfo;
  onClose: (done: boolean) => void;
}) {
  const [filesystem, setFilesystem] = useState("exfat");
  const [label, setLabel] = useState(disk.name);
  const [wholeDisk, setWholeDisk] = useState(false);
  const [confirmName, setConfirmName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);

  const confirmOk = confirmName.trim() === disk.name;

  const submit = async () => {
    setBusy(true);
    setError(null);
    const r = await post("/api/disks/format", {
      mountPoint: disk.mountPoint,
      filesystem,
      label,
      wholeDisk,
      confirmName: confirmName.trim(),
    });
    setBusy(false);
    if (r.ok) onClose(true);
    else setError(r.error);
  };

  return (
    <div className="overlay" onClick={() => onClose(false)}>
      <div className="dialog" onClick={(e) => e.stopPropagation()}>
        <h3>Formatta «{disk.name}»</h3>
        <div className="banner banner-error">
          ⚠ Operazione irreversibile: tutti i dati su questo volume verranno cancellati.
        </div>

        <label className="form-row">
          <span>File system</span>
          <select value={filesystem} onChange={(e) => setFilesystem(e.target.value)}>
            {FILESYSTEMS.map((fs) => (
              <option key={fs.id} value={fs.id}>
                {fs.label}
              </option>
            ))}
          </select>
        </label>

        <label className="form-row">
          <span>Nome volume</span>
          <input value={label} onChange={(e) => setLabel(e.target.value)} />
        </label>

        <label className="checkbox">
          <input
            type="checkbox"
            checked={wholeDisk}
            onChange={(e) => setWholeDisk(e.target.checked)}
          />
          Formattazione a basso livello (ripartiziona l'intero disco fisico, più lenta)
        </label>

        <label className="form-row">
          <span>
            Digita <code>{disk.name}</code> per confermare
          </span>
          <input
            value={confirmName}
            onChange={(e) => setConfirmName(e.target.value)}
            placeholder={disk.name}
            autoFocus
          />
        </label>

        {error && (
          <div className="banner banner-error">
            {error.message}
            {error.osHint && <div className="hint">{error.osHint}</div>}
          </div>
        )}

        <div className="dialog-actions">
          <button onClick={() => onClose(false)} disabled={busy}>
            Annulla
          </button>
          <button className="danger" onClick={submit} disabled={busy || !confirmOk}>
            {busy ? "Formatto…" : "Formatta"}
          </button>
        </div>
      </div>
    </div>
  );
}
