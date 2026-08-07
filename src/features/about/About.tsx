import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { api, post } from "../../lib/api";
import { useUpdateStore } from "../../stores/updateStore";

const isTauri = "__TAURI_INTERNALS__" in window;

const OWNER = "Riccardo Giordano";
const GITHUB_USER = "RickyLeRoi";
const REPO_URL = "https://github.com/RickyLeRoi/RickyDEVTool";

function openExternal(url: string) {
  if (isTauri) post("/api/system/open-url", { url });
  else window.open(url, "_blank");
}

function UpdateFeedback() {
  const { t } = useTranslation();
  const phase = useUpdateStore((s) => s.phase);
  const error = useUpdateStore((s) => s.error);
  if (phase === "checking") return <span className="dim"> {t("about.checking")}</span>;
  if (phase === "uptodate")
    return <span className="badge badge-ok">{t("about.upToDate")}</span>;
  if (phase === "available")
    return <span className="badge badge-branch">{t("about.updateAvailable")}</span>;
  if (phase === "downloading") return <span className="dim"> {t("about.downloading")}</span>;
  if (phase === "error")
    return <span className="banner-error-text">{t("about.errorLabel", { message: error })}</span>;
  return null;
}

export function About() {
  const { t } = useTranslation();
  const [version, setVersion] = useState<string | null>(null);
  const check = useUpdateStore((s) => s.check);

  useEffect(() => {
    api<{ version: string }>("/api/health").then((r) => {
      if (r.ok) setVersion(r.data.version);
    });
  }, []);

  return (
    <div className="settings about">
      <h2>{t("nav.about")}</h2>

      <section>
        <h3>RickyDEVTool</h3>
        <div className="about-version-row">
          <div>
            <span className="dim">{t("about.currentVersion")}</span>{" "}
            <span className="about-version">{version ?? t("common.none")}</span>
          </div>
          {isTauri && (
            <button className="small" onClick={() => check(true)}>
              {t("about.checkUpdates")}
            </button>
          )}
        </div>
        <div className="about-feedback">
          <UpdateFeedback />
        </div>
      </section>

      <section>
        <h3>{t("about.author")}</h3>
        <div className="about-line">
          <span className="dim">{t("about.owner")}</span>
          <span>{OWNER}</span>
        </div>
        <div className="about-line">
          <span className="dim">GitHub</span>
          <button
            className="linklike"
            onClick={() => openExternal(`https://github.com/${GITHUB_USER}`)}
          >
            @{GITHUB_USER}
          </button>
        </div>
        <div className="about-line">
          <span className="dim">{t("about.repository")}</span>
          <button className="linklike" onClick={() => openExternal(REPO_URL)}>
            {REPO_URL}
          </button>
        </div>
      </section>
    </div>
  );
}
