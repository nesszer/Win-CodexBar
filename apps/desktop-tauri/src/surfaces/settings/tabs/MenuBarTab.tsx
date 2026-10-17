import { useEffect, useMemo, useState } from "react";
import { useLocale } from "../../../hooks/useLocale";
import { Field, Select, SettingsGroup, Toggle } from "../../../components/FormControls";
import type {
  MenuBarIconStyle,
  ProviderCatalogEntry,
  TrayVisibilityStatusDto,
} from "../../../types/bridge";
import type { TabProps } from "../settingsTabs";
import { getTrayVisibilityStatus } from "../../../lib/tauri";
import { orderProviderSnapshots } from "../../../lib/providerOrder";
import {
  MERGED_OVERVIEW_PROVIDER_LIMIT,
  effectiveOverviewSelection,
  toggleOverviewProvider,
} from "../../../lib/overviewProviders";
import SettingsSheet from "../SettingsSheet";

type CombinedIconStyle = "switcher" | "stacked";

/**
 * Settings > Menu Bar, laid out like the macOS "Menu Bar" pane: the tray icon
 * look ("Icon") and how several providers share the one icon ("Combined
 * icon"). Windows always merges providers into one tray icon, so the Mac
 * "Merge icons" switch is not shown.
 */
export default function MenuBarTab({
  settings,
  set,
  saving,
  providers = [],
}: TabProps & { providers?: ProviderCatalogEntry[] }) {
  const { t } = useLocale();
  const [trayVisibility, setTrayVisibility] = useState<TrayVisibilityStatusDto | null>(null);
  const [configuringOverview, setConfiguringOverview] = useState(false);

  useEffect(() => {
    getTrayVisibilityStatus()
      .then(setTrayVisibility)
      .catch(() => setTrayVisibility(null));
  }, []);

  const providerName = useMemo(
    () => new Map(providers.map((provider) => [provider.id, provider.displayName])),
    [providers],
  );
  const enabledInOrder = useMemo(
    () =>
      orderProviderSnapshots(
        settings.enabledProviders.map((providerId) => ({
          providerId,
          displayName: providerName.get(providerId) ?? providerId,
        })),
        providers,
        settings.enabledProviders,
        settings.providerOrder,
      ),
    [providerName, providers, settings.enabledProviders, settings.providerOrder],
  );
  const enabledIds = enabledInOrder.map((provider) => provider.providerId);
  const savedSelection = settings.mergedOverviewSelectedProviders ?? [];
  const overviewIds = effectiveOverviewSelection(enabledIds, savedSelection);
  const overviewIsDefault = !enabledIds.some((id) => savedSelection.includes(id));
  const overviewSummary = overviewIsDefault
    ? t("OverviewProvidersDefaultSummary").replace("{}", String(MERGED_OVERVIEW_PROVIDER_LIMIT))
    : overviewIds.map((id) => providerName.get(id) ?? id).join(", ");

  const stacked = settings.trayIconMode === "stacked";
  const combinedStyle: CombinedIconStyle = stacked ? "stacked" : "switcher";
  const stackedProviderOptions = enabledInOrder.map((provider) => ({
    value: provider.providerId,
    label: provider.displayName,
  }));
  const pinSupported = trayVisibility?.support === "supported";

  return (
    <>
      <SettingsGroup title={t("MenuBarIconSectionTitle")}>
        <Field label={t("MenuBarIconStyleLabel")} description={t("MenuBarIconStyleHelper")}>
          <Select
            variant="menu"
            value={settings.menuBarIconStyle ?? (settings.menuBarShowsPercent ? "percent" : "bars")}
            disabled={saving || stacked}
            options={[
              { value: "bars", label: t("MenuBarIconStyleBars") },
              { value: "percent", label: t("MenuBarIconStylePercent") },
              { value: "stackedBars", label: t("MenuBarIconStyleStackedBars") },
            ]}
            onChange={(v) => set({ menuBarIconStyle: v as MenuBarIconStyle })}
          />
        </Field>
        <Field label={t("ColorPaceInTray")} description={t("ColorPaceInTrayHelper")}>
          <Toggle
            variant="switch"
            checked={settings.menuBarColorPace ?? false}
            disabled={saving}
            onChange={(v) => set({ menuBarColorPace: v })}
          />
        </Field>
        <Field label={t("ColorByProviderInTray")} description={t("ColorByProviderInTrayHelper")}>
          <Toggle
            variant="switch"
            checked={settings.menuBarColorByProvider ?? false}
            disabled={saving}
            onChange={(v) => set({ menuBarColorByProvider: v })}
          />
        </Field>
        <Field label={t("PaceSignLabel")} description={t("PaceSignHelper")}>
          <Select
            variant="menu"
            value={settings.menuBarPaceReservePositive ? "reserve" : "ahead"}
            disabled={saving}
            options={[
              { value: "ahead", label: t("PaceSignAheadPositive") },
              { value: "reserve", label: t("PaceSignReservePositive") },
            ]}
            onChange={(v) => set({ menuBarPaceReservePositive: v === "reserve" })}
          />
        </Field>
        <Field
          label={t("PromoteTrayIconLabel")}
          description={pinSupported ? t("PromoteTrayIconHelper") : t("PromoteTrayIconUnsupportedHint")}
        >
          <Toggle
            variant="switch"
            checked={settings.promoteTrayIcon ?? false}
            disabled={saving || !pinSupported}
            onChange={(v) => set({ promoteTrayIcon: v })}
          />
        </Field>
      </SettingsGroup>

      <SettingsGroup title={t("CombinedIconSectionTitle")}>
        <Field label={t("CombinedIconStyleLabel")}>
          <Select
            variant="menu"
            value={combinedStyle}
            disabled={saving}
            options={[
              { value: "switcher", label: t("CombinedIconStyleSwitcher") },
              { value: "stacked", label: t("CombinedIconStyleStacked") },
            ]}
            onChange={(v) => {
              if (v === combinedStyle) return;
              set({ trayIconMode: v === "stacked" ? "stacked" : "single" });
            }}
          />
        </Field>
        {stacked && (
          <>
            <Field label={t("StackedTrayTopProvider")}>
              <Select
                variant="menu"
                value={settings.stackedTrayTopProvider ?? ""}
                disabled={saving}
                options={[
                  { value: "", label: t("Automatic") },
                  ...stackedProviderOptions.filter(
                    (provider) => provider.value !== settings.stackedTrayBottomProvider,
                  ),
                ]}
                onChange={(provider) => set({ stackedTrayTopProvider: provider })}
              />
            </Field>
            <Field label={t("StackedTrayBottomProvider")}>
              <Select
                variant="menu"
                value={settings.stackedTrayBottomProvider ?? ""}
                disabled={saving}
                options={[
                  { value: "", label: t("Automatic") },
                  ...stackedProviderOptions.filter(
                    (provider) => provider.value !== settings.stackedTrayTopProvider,
                  ),
                ]}
                onChange={(provider) => set({ stackedTrayBottomProvider: provider })}
              />
            </Field>
          </>
        )}
        <Field
          label={t("ShowEveryProviderInTrayMenuLabel")}
          description={t("ShowEveryProviderInTrayMenuHelper")}
        >
          <Toggle
            variant="switch"
            checked={settings.trayIconMode === "perProvider"}
            disabled={saving || stacked}
            onChange={(v) => set({ trayIconMode: v ? "perProvider" : "single" })}
          />
        </Field>
        <Field label={t("SwitcherRowsLabel")}>
          <Select
            variant="menu"
            value={settings.switcherShowsIcons ? "icons" : "weekly"}
            disabled={saving}
            options={[
              { value: "icons", label: t("SwitcherRowsProviderIcons") },
              { value: "weekly", label: t("SwitcherRowsWeeklyProgress") },
            ]}
            onChange={(v) => set({ switcherShowsIcons: v === "icons" })}
          />
        </Field>
        <Field label={t("PreferHighestUsage")} description={t("PreferHighestUsageHelper")}>
          <Toggle
            variant="switch"
            checked={settings.menuBarShowsHighestUsage}
            disabled={saving || stacked}
            onChange={(v) => set({ menuBarShowsHighestUsage: v })}
          />
        </Field>
        <Field label={t("OverviewProvidersLabel")} description={overviewSummary}>
          <button
            type="button"
            className="credential-btn"
            disabled={saving || enabledIds.length === 0}
            onClick={() => setConfiguringOverview(true)}
          >
            {t("OverviewProvidersConfigure")}
          </button>
        </Field>
      </SettingsGroup>

      {configuringOverview && (
        <SettingsSheet
          title={t("OverviewProvidersChooseUpTo").replace(
            "{}",
            String(MERGED_OVERVIEW_PROVIDER_LIMIT),
          )}
          onClose={() => setConfiguringOverview(false)}
        >
          <div className="overview-providers-list" role="group">
            {enabledInOrder.map((provider) => {
              const checked = overviewIds.includes(provider.providerId);
              const full = overviewIds.length >= MERGED_OVERVIEW_PROVIDER_LIMIT;
              return (
                <Toggle
                  key={provider.providerId}
                  label={provider.displayName}
                  ariaLabel={provider.displayName}
                  checked={checked}
                  disabled={saving || (!checked && full)}
                  onChange={(next) =>
                    set({
                      mergedOverviewSelectedProviders: toggleOverviewProvider(
                        enabledIds,
                        savedSelection,
                        provider.providerId,
                        next,
                      ),
                    })
                  }
                />
              );
            })}
          </div>
        </SettingsSheet>
      )}
    </>
  );
}
