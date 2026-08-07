import { useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { Modal } from "../../components/Modal";
import { post } from "../../lib/api";
import { useSubmit } from "../../lib/useSubmit";
import type { KillOutcome, PortProcess } from "../../lib/types";

interface KillDialogProps {
  process: PortProcess;
  onClose: (killed: boolean) => void;
}

export function KillDialog({ process, onClose }: KillDialogProps) {
  const { t } = useTranslation();
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
      title={t("kill.title")}
      onCancel={() => onClose(false)}
      error={error}
      busy={busy}
      confirm={{
        label: busy ? t("kill.submitting") : force ? t("kill.submitForce") : t("kill.submit"),
        onClick: confirm,
        danger: true,
        disabled: !typedOk,
      }}
    >
      <p>
        <strong>{process.name}</strong>{" "}
        {process.user
          ? t("kill.pidUser", { pid: process.pid, user: process.user })
          : t("kill.pid", { pid: process.pid })}
      </p>
      {needsTyped && (
        <>
          <p className="hint">
            <Trans i18nKey="kill.protectedHint" values={{ name: process.name }} components={{ code: <code /> }} />
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
        {t("kill.forceLabel")}
      </label>
    </Modal>
  );
}
