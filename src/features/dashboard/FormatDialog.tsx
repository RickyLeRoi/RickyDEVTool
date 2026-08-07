import { useState } from "react";
import { Trans, useTranslation } from "react-i18next";
import { Modal } from "../../components/Modal";
import { post } from "../../lib/api";
import { useSubmit } from "../../lib/useSubmit";
import { DEFAULT_FORMAT_FILESYSTEM } from "../../lib/defaults";
import type { DiskInfo } from "../../lib/types";

const FILESYSTEMS = [
  { id: "exfat", labelKey: "dashboard.format.fsExfat" },
  { id: "fat32", labelKey: "dashboard.format.fsFat32" },
  { id: "apfs", labelKey: "dashboard.format.fsApfs" },
  { id: "hfs+", labelKey: "dashboard.format.fsHfs" },
  { id: "ntfs", labelKey: "dashboard.format.fsNtfs" },
] as const;

export function FormatDialog({
  disk,
  onClose,
}: {
  disk: DiskInfo;
  onClose: (done: boolean) => void;
}) {
  const { t } = useTranslation();
  const [filesystem, setFilesystem] = useState(DEFAULT_FORMAT_FILESYSTEM);
  const [label, setLabel] = useState(disk.name);
  const [wholeDisk, setWholeDisk] = useState(false);
  const [confirmName, setConfirmName] = useState("");
  const { busy, error, run } = useSubmit();

  const confirmOk = confirmName.trim() === disk.name;

  const submit = () =>
    run(
      () =>
        post("/api/disks/format", {
          mountPoint: disk.mountPoint,
          filesystem,
          label,
          wholeDisk,
          confirmName: confirmName.trim(),
        }),
      () => onClose(true),
    );

  return (
    <Modal
      title={t("dashboard.format.title", { name: disk.name })}
      onCancel={() => onClose(false)}
      error={error}
      busy={busy}
      confirm={{
        label: busy ? t("dashboard.format.formatting") : t("dashboard.format.submit"),
        onClick: submit,
        danger: true,
        disabled: !confirmOk,
      }}
    >
      <div className="banner banner-error">{t("dashboard.format.irreversible")}</div>

      <label className="form-row">
        <span>{t("dashboard.format.filesystem")}</span>
        <select value={filesystem} onChange={(e) => setFilesystem(e.target.value)}>
          {FILESYSTEMS.map((fs) => (
            <option key={fs.id} value={fs.id}>
              {t(fs.labelKey)}
            </option>
          ))}
        </select>
      </label>

      <label className="form-row">
        <span>{t("dashboard.format.volumeName")}</span>
        <input value={label} onChange={(e) => setLabel(e.target.value)} />
      </label>

      <label className="checkbox">
        <input
          type="checkbox"
          checked={wholeDisk}
          onChange={(e) => setWholeDisk(e.target.checked)}
        />
        {t("dashboard.format.lowLevel")}
      </label>

      <label className="form-row">
        <span>
          <Trans i18nKey="dashboard.format.confirmType" values={{ name: disk.name }} components={{ code: <code /> }} />
        </span>
        <input
          value={confirmName}
          onChange={(e) => setConfirmName(e.target.value)}
          placeholder={disk.name}
          autoFocus
        />
      </label>
    </Modal>
  );
}
