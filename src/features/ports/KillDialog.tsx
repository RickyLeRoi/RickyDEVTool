import { useState } from "react";
import { Modal } from "../../components/Modal";
import { post } from "../../lib/api";
import { useSubmit } from "../../lib/useSubmit";
import type { KillOutcome, PortProcess } from "../../lib/types";

interface KillDialogProps {
  process: PortProcess;
  onClose: (killed: boolean) => void;
}

export function KillDialog({ process, onClose }: KillDialogProps) {
  const needsTyped = process.killProtection === "typed-confirm";
  const [typed, setTyped] = useState("");
  const [force, setForce] = useState(false);
  const { busy, error, run } = useSubmit();

  const confirm = () =>
    run(
      () =>
        post<KillOutcome>("/api/processes/kill", {
          pid: process.pid,
          expectedName: process.name,
          expectedStartedAt: process.startedAt,
          force,
          confirmName: needsTyped ? typed.trim() : undefined,
        }),
      () => onClose(true),
    );

  const typedOk = !needsTyped || typed.trim().toLowerCase() === process.name.toLowerCase();

  return (
    <Modal
      title="Termina processo"
      onCancel={() => onClose(false)}
      error={error}
      busy={busy}
      confirm={{
        label: busy ? "Termino…" : force ? "Force kill" : "Termina",
        onClick: confirm,
        danger: true,
        disabled: !typedOk,
      }}
    >
      <p>
        <strong>{process.name}</strong> (PID {process.pid}
        {process.user ? `, utente ${process.user}` : ""})
      </p>
      {needsTyped && (
        <>
          <p className="hint">
            Processo protetto: digita <code>{process.name}</code> per confermare.
          </p>
          <input
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            placeholder={process.name}
            autoFocus
          />
        </>
      )}
      <label className="checkbox">
        <input type="checkbox" checked={force} onChange={(e) => setForce(e.target.checked)} />
        Force kill (immediato, senza chiusura pulita)
      </label>
    </Modal>
  );
}
