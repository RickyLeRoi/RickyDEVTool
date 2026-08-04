import { useEffect, type ReactNode } from "react";
import type { ApiError } from "../lib/types";

interface ModalProps {
  title: ReactNode;
  onCancel: () => void;
  children: ReactNode;
  error?: ApiError | null;
  busy?: boolean;
  confirm?: {
    label: string;
    onClick: () => void;
    danger?: boolean;
    disabled?: boolean;
  };
  cancelLabel?: string;
  className?: string;
}

export function Modal({
  title,
  onCancel,
  children,
  error,
  busy = false,
  confirm,
  cancelLabel = confirm ? "Annulla" : "Chiudi",
  className,
}: ModalProps) {
  useEffect(() => {
    if (busy) return;
    const onKey = (e: KeyboardEvent) => {
      // 20260704 RG Esc non chiude mentre un'azione è in volo: l'utente resterebbe senza
      // sapere se l'operazione è andata a buon fine.
      if (e.key === "Escape") onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [busy, onCancel]);

  return (
    <div className="overlay" onClick={() => !busy && onCancel()}>
      <div
        className={className ? `dialog ${className}` : "dialog"}
        onClick={(e) => e.stopPropagation()}
      >
        <h3>{title}</h3>
        {children}
        {error && (
          <div className="banner banner-error">
            {error.message}
            {error.osHint && <div className="hint">{error.osHint}</div>}
          </div>
        )}
        <div className="dialog-actions">
          <button onClick={onCancel} disabled={busy}>
            {cancelLabel}
          </button>
          {confirm && (
            <button
              className={confirm.danger ? "danger" : undefined}
              onClick={confirm.onClick}
              disabled={busy || confirm.disabled}
            >
              {confirm.label}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
