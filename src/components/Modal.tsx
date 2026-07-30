import { useEffect, type ReactNode } from "react";
import type { ApiError } from "../lib/types";

interface ModalProps {
  title: ReactNode;
  /** Chiusura senza conferma: click fuori, Esc, pulsante di annullamento. */
  onCancel: () => void;
  children: ReactNode;
  /** Errore dell'ultima azione: reso in fondo, sopra ai pulsanti. */
  error?: ApiError | null;
  /** Azione in corso: blocca i pulsanti per evitare doppi invii. */
  busy?: boolean;
  /** Azione principale. Assente = dialog puramente informativo (solo "Chiudi"). */
  confirm?: {
    label: string;
    onClick: () => void;
    /** Rende il pulsante rosso: per le operazioni distruttive. */
    danger?: boolean;
    disabled?: boolean;
  };
  cancelLabel?: string;
  /** Classe extra sul riquadro, per i dialog con layout proprio (es. il QR). */
  className?: string;
}

/// Guscio comune dei dialog: overlay, riquadro, titolo, banner d'errore e barra
/// dei pulsanti. Prima ogni dialog si riscriveva questa impalcatura, e le
/// differenze erano tutte involontarie — uno chiudeva con Esc, gli altri no.
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
  // Esc chiude, ma non mentre un'azione è in volo: annullare a metà lascerebbe
  // l'utente senza sapere se l'operazione è andata a buon fine.
  useEffect(() => {
    if (busy) return;
    const onKey = (e: KeyboardEvent) => {
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
