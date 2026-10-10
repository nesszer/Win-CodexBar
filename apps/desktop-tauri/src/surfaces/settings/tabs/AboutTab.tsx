import { useEffect, useState } from "react";
import { useLocale } from "../../../hooks/useLocale";
import { useUpdateState } from "../../../hooks/useUpdateState";
import { getAppInfo, openExternalUrl } from "../../../lib/tauri";
import { SettingSelect, SettingToggle } from "../../../components/FormControls";
import type { AppInfoBridge } from "../../../types/bridge";
import type { LocaleKey } from "../../../i18n/keys";
import type { TabProps } from "../settingsTabs";
import codexbarIcon from "../../../assets/codexbar-icon.png";

const REPO_URL = "https://github.com/nesszer/Win-CodexBar";
const SUBMIT_ISSUE_URL = `${REPO_URL}/issues/new?labels=bug&template=bug_report.yml`;

const ABOUT_LINKS: ReadonlyArray<{ labelKey: LocaleKey; url: string }> = [
  {
    labelKey: "AboutLinkGitHub",
    url: REPO_URL,
  },
  {
    labelKey: "AboutLinkWebsite",
    url: "https://codexbar.app",
  },
  {
    labelKey: "AboutLinkOriginalProject",
    url: "https://github.com/steipete/CodexBar",
  },
];

export default function AboutTab({ settings, set, saving }: TabProps) {
  const { t } = useLocale();
  const ctl = { settings, set, saving };
  const [appInfo, setAppInfo] = useState<AppInfoBridge | null>(null);
  const { updateState, checkNow, download, apply, openRelease } =
    useUpdateState();
  const [hasChecked, setHasChecked] = useState(false);
  const [linkError, setLinkError] = useState<string | null>(null);

  useEffect(() => {
    void getAppInfo().then(setAppInfo);
  }, []);

  const handleCheck = () => {
    setHasChecked(true);
    checkNow();
  };

  const openAboutLink = (url: string) => {
    setLinkError(null);
    openExternalUrl(url).catch((error) => {
      setLinkError(String(error));
    });
  };

  if (!appInfo) {
    return (
      <section className="settings-section">
        <p className="settings-section__hint">{t("AboutLoading")}</p>
      </section>
    );
  }

  const isBusy =
    updateState.status === "checking" ||
    updateState.status === "downloading";

  // Copyright is split into two keys so the brand link can render inline
  // between them, avoiding any Fluent placeholder syntax.
  const copyrightBefore = t("AboutCopyrightBefore");
  const copyrightAfter = t("AboutCopyrightAfter");

  return (
    <section className="settings-section about-section">
      <div className="about-header">
        <img className="about-icon" src={codexbarIcon} alt={t("AppName")} />
        <div className="about-title-block">
          <h2 className="about-title">{appInfo.name}</h2>
          <p className="about-version">
            {t("Version")} {appInfo.version}
            {appInfo.buildNumber !== "dev" && ` (${appInfo.buildNumber})`}
          </p>
          <p className="about-tagline">{appInfo.tagline}</p>
        </div>
      </div>

      <div className="about-links">
        {ABOUT_LINKS.map((link) => (
          <button
            key={link.url}
            type="button"
            className="about-link"
            onClick={() => openAboutLink(link.url)}
          >
            {t(link.labelKey)}
          </button>
        ))}
      </div>
      <button
        type="button"
        className="about-link"
        onClick={() => openAboutLink(SUBMIT_ISSUE_URL)}
      >
        {t("SubmitIssue")}
      </button>
      {linkError && (
        <p className="about-update-msg">
          {t("ErrorPrefix")} {linkError}
        </p>
      )}

      <div className="about-divider" />

      <div className="about-update-controls">
        <SettingToggle
          ctl={ctl}
          field="autoDownloadUpdates"
          label={t("AutoDownloadUpdates")}
          description={t("AutoDownloadUpdatesHelper")}
        />

        <div className="about-channel-row">
          <SettingSelect
            ctl={ctl}
            field="updateChannel"
            label={t("UpdateChannelChoice")}
            options={[
              { value: "stable", label: t("UpdateChannelStableOption") },
              { value: "beta", label: t("UpdateChannelBetaOption") },
            ]}
          />
          <p className="about-channel-description">
            {t("UpdateChannelChoiceHelper")}
          </p>
        </div>
      </div>

      <div className="about-actions">
        <button
          type="button"
          className="credential-btn credential-btn--primary"
          disabled={isBusy}
          onClick={handleCheck}
        >
          {updateState.status === "checking"
            ? t("AboutChecking")
            : t("AboutCheckForUpdates")}
        </button>

        {updateState.status === "available" && (
          <div className="about-update-row">
            <span className="about-update-msg">
              {t("UpdateAvailableMessage").replace(
                "{}",
                updateState.version ?? "",
              )}
            </span>
            {updateState.canDownload ? (
              <button
                type="button"
                className="credential-btn credential-btn--primary"
                onClick={download}
              >
                {t("BannerDownloadButton")}
              </button>
            ) : (
              <button type="button" className="credential-btn" onClick={openRelease}>
                {t("BannerViewRelease")}
              </button>
            )}
          </div>
        )}

        {updateState.status === "downloading" && (
          <span className="about-update-msg">
            {t("UpdateDownloading")}
            {updateState.progress != null &&
              ` ${Math.round(updateState.progress * 100)}%`}
          </span>
        )}

        {updateState.status === "ready" && (
          <div className="about-update-row">
            <span className="about-update-msg">{t("UpdateReady")}</span>
            {updateState.canApply ? (
              <button
                type="button"
                className="credential-btn credential-btn--primary"
                onClick={apply}
              >
                {t("BannerInstallRestart")}
              </button>
            ) : (
              <button type="button" className="credential-btn" onClick={openRelease}>
                {t("BannerViewRelease")}
              </button>
            )}
          </div>
        )}

        {updateState.status === "error" && (
          <span className="about-update-msg">
            {t("ErrorPrefix")} {updateState.error}
          </span>
        )}

        {updateState.status === "idle" && hasChecked && (
          <span className="about-update-msg">{t("AboutUpToDate")}</span>
        )}
      </div>

      <p className="about-copyright">
        {copyrightBefore}{" "}
        <button
          type="button"
          className="about-link about-link--inline"
          onClick={() => openAboutLink("https://github.com/steipete/CodexBar")}
        >
          {t("AppName")}
        </button>
        {" "}{copyrightAfter}
      </p>
    </section>
  );
}
