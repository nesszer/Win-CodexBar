import { useEffect, useState } from "react";
import { useLocale } from "../../../hooks/useLocale";
import { SettingSelect, SettingToggle } from "../../../components/FormControls";
import type { ProviderCatalogEntry, TrayVisibilityStatusDto } from "../../../types/bridge";
import type { TabProps } from "../settingsTabs";
import FloatBarSettingsSection from "../../../floatbar/SettingsSection";
import SwitcherShortcutsSection from "../SwitcherShortcutsSection";
import { getTrayVisibilityStatus } from "../../../lib/tauri";

export default function DisplayTab({
  mode = "menu",
  settings,
  set,
  saving,
  providers = [],
}: TabProps & {
  mode?: "menuBar" | "menu";
  providers?: ProviderCatalogEntry[];
}) {
  const { t } = useLocale();
  const ctl = { settings, set, saving };
  const [trayVisibility, setTrayVisibility] = useState<TrayVisibilityStatusDto | null>(null);

  useEffect(() => {
    getTrayVisibilityStatus()
      .then(setTrayVisibility)
      .catch(() => setTrayVisibility(null));
  }, []);

  const providerName = new Map(
    providers.map((provider) => [provider.id, provider.displayName]),
  );
  const stackedProviderOptions = settings.enabledProviders.map((providerId) => ({
    value: providerId,
    label: providerName.get(providerId) ?? providerId,
  }));
  return (
    <>
      {/* ── Menu bar ─────────────────────────────────────────────── */}
      {mode === "menuBar" && <section className="settings-section">
        <h3 className="settings-section__title">{t("MenuBar")}</h3>
        <div className="settings-section__group">
          <SettingSelect
            ctl={ctl}
            field="trayIconMode"
            label={t("TrayIconModeLabel")}
            description={t("TrayIconModeHelper")}
            options={[
              { value: "single", label: t("TrayIconModeSingle") },
              { value: "perProvider", label: t("TrayIconModePerProvider") },
              { value: "stacked", label: t("TrayIconModeStacked") },
            ]}
          />
          {settings.trayIconMode === "stacked" && (
            <>
              <SettingSelect
                ctl={ctl}
                field="stackedTrayTopProvider"
                label={t("StackedTrayTopProvider")}
                options={[
                  { value: "", label: t("Automatic") },
                  ...stackedProviderOptions.filter(
                    (provider) =>
                      provider.value !== settings.stackedTrayBottomProvider,
                  ),
                ]}
              />
              <SettingSelect
                ctl={ctl}
                field="stackedTrayBottomProvider"
                label={t("StackedTrayBottomProvider")}
                options={[
                  { value: "", label: t("Automatic") },
                  ...stackedProviderOptions.filter(
                    (provider) =>
                      provider.value !== settings.stackedTrayTopProvider,
                  ),
                ]}
              />
            </>
          )}
          <SettingToggle
            ctl={ctl}
            field="switcherShowsIcons"
            label={t("ShowProviderIcons")}
            description={t("ShowProviderIconsHelper")}
          />
          <SettingToggle
            ctl={ctl}
            field="menuBarShowsHighestUsage"
            label={t("PreferHighestUsage")}
            description={t("PreferHighestUsageHelper")}
            disabled={saving || settings.trayIconMode === "stacked"}
          />
          <SettingToggle
            ctl={ctl}
            field="menuBarShowsPercent"
            label={t("ShowPercentInTray")}
            description={t("ShowPercentInTrayHelper")}
            disabled={saving || settings.trayIconMode === "stacked"}
          />
          <SettingToggle
            ctl={ctl}
            field="menuBarColorPace"
            label={t("ColorPaceInTray")}
            description={t("ColorPaceInTrayHelper")}
            ariaLabel={t("ColorPaceInTray")}
          />
          <SettingSelect
            ctl={ctl}
            field="menuBarDisplayMode"
            label={t("DisplayModeLabel")}
            description={t("DisplayModeHelper")}
            options={[
              { value: "detailed", label: t("DisplayModeDetailed") },
              { value: "compact", label: t("DisplayModeCompact") },
              { value: "minimal", label: t("DisplayModeMinimal") },
            ]}
          />
          <SettingToggle
            ctl={ctl}
            field="promoteTrayIcon"
            label={t("PromoteTrayIconLabel")}
            description={trayVisibility?.support === "supported"
                ? t("PromoteTrayIconHelper")
                : t("PromoteTrayIconUnsupportedHint")}
            disabled={saving || trayVisibility?.support !== "supported"}
          />
        </div>
      </section>}

      {/* ── Menu content ─────────────────────────────────────────── */}
      {mode === "menu" && <section className="settings-section">
        <h3 className="settings-section__title">{t("TabMenu")}</h3>
        <div className="settings-section__group">
          <SettingToggle
            ctl={ctl}
            field="trayPanelAlwaysOnTop"
            label={t("TrayPanelAlwaysOnTopLabel")}
            description={t("TrayPanelAlwaysOnTopHelper")}
            ariaLabel={t("TrayPanelAlwaysOnTopLabel")}
          />
          <SettingToggle
            ctl={ctl}
            field="showAsUsed"
            label={t("ShowAsUsedLabel")}
            description={t("ShowAsUsedHelper")}
          />
          <SettingSelect
            ctl={ctl}
            field="overviewLayout"
            label={t("OverviewLayoutLabel")}
            description={t("OverviewLayoutHelper")}
            options={[
              { value: "detailed", label: t("OverviewLayoutDetailed") },
              { value: "compact", label: t("OverviewLayoutCompact") },
            ]}
          />
          <SettingToggle
            ctl={ctl}
            field="showAllTokenAccountsInMenu"
            label={t("ShowAllTokenAccountsLabel")}
            description={t("ShowAllTokenAccountsHelper")}
          />
          <SettingToggle
            ctl={ctl}
            field="resetTimeRelative"
            label={t("ResetTimeRelative")}
            description={t("ResetTimeRelativeHelper")}
          />
          <SettingToggle
            ctl={ctl}
            field="showResetWhenExhausted"
            label={t("ShowResetWhenExhausted")}
            description={t("ShowResetWhenExhaustedHelper")}
            ariaLabel={t("ShowResetWhenExhausted")}
          />
          <SettingToggle
            ctl={ctl}
            field="showPace"
            checked={settings.showPace ?? true}
            label={t("ShowPace")}
            description={t("ShowPaceHelper")}
            ariaLabel={t("ShowPace")}
          />
        </div>
      </section>}

      {mode === "menu" && (
        <SwitcherShortcutsSection settings={settings} saving={saving} set={set} />
      )}

      {mode === "menu" && (
        <FloatBarSettingsSection settings={settings} saving={saving} set={set} />
      )}
    </>
  );
}
