import { useCallback, useEffect, useState } from "react";
import { useLocale } from "../../../hooks/useLocale";
import { Field, Select, Toggle } from "../../../components/FormControls";
import type { MenuBarDisplayMode, TrayIconMode } from "../../../types/bridge";
import type { TabProps } from "../../Settings";
import { FloatBarSettingsSection } from "../../../floatbar";

function clampWindowScalePercent(value: number): number {
  return Math.min(250, Math.max(125, Number.isFinite(value) ? value : 125));
}

export default function DisplayTab({ settings, set, saving }: TabProps) {
  const { t } = useLocale();
  const [windowScaleDraft, setWindowScaleDraft] = useState(() =>
    clampWindowScalePercent(settings.windowScalePercent),
  );

  useEffect(() => {
    setWindowScaleDraft(clampWindowScalePercent(settings.windowScalePercent));
  }, [settings.windowScalePercent]);

  const commitWindowScale = useCallback(() => {
    const next = clampWindowScalePercent(windowScaleDraft);
    if (next !== settings.windowScalePercent) {
      set({ windowScalePercent: next });
    }
  }, [set, settings.windowScalePercent, windowScaleDraft]);
  return (
    <>
      {/* ── Menu bar ─────────────────────────────────────────────── */}
      <section className="settings-section">
        <h3 className="settings-section__title">{t("MenuBar")}</h3>
        <div className="settings-section__group">
          <Field
            label={t("TrayIconModeLabel")}
            description={t("TrayIconModeHelper")}
          >
            <Select
              value={settings.trayIconMode}
              disabled={saving}
              options={[
                { value: "single", label: t("TrayIconModeSingle") },
                { value: "perProvider", label: t("TrayIconModePerProvider") },
              ]}
              onChange={(v) => set({ trayIconMode: v as TrayIconMode })}
            />
          </Field>
          <Field
            label={t("ShowProviderIcons")}
            description={t("ShowProviderIconsHelper")}
            leading
          >
            <Toggle
              checked={settings.switcherShowsIcons}
              disabled={saving}
              onChange={(v) => set({ switcherShowsIcons: v })}
            />
          </Field>
          <Field
            label={t("PreferHighestUsage")}
            description={t("PreferHighestUsageHelper")}
            leading
          >
            <Toggle
              checked={settings.menuBarShowsHighestUsage}
              disabled={saving}
              onChange={(v) => set({ menuBarShowsHighestUsage: v })}
            />
          </Field>
          <Field
            label={t("ShowPercentInTray")}
            description={t("ShowPercentInTrayHelper")}
            leading
          >
            <Toggle
              checked={settings.menuBarShowsPercent}
              disabled={saving}
              onChange={(v) => set({ menuBarShowsPercent: v })}
            />
          </Field>
          <Field
            label={t("DisplayModeLabel")}
            description={t("DisplayModeHelper")}
          >
            <Select
              value={settings.menuBarDisplayMode}
              disabled={saving}
              options={[
                { value: "detailed", label: t("DisplayModeDetailed") },
                { value: "compact", label: t("DisplayModeCompact") },
                { value: "minimal", label: t("DisplayModeMinimal") },
              ]}
              onChange={(v) =>
                set({ menuBarDisplayMode: v as MenuBarDisplayMode })
              }
            />
          </Field>
        </div>
      </section>

      {/* ── Menu content ─────────────────────────────────────────── */}
      <section className="settings-section">
        <h3 className="settings-section__title">Menu Content</h3>
        <div className="settings-section__group">
          <Field
            label={`Window scale (${windowScaleDraft}%)`}
            description="Pop-out content size."
          >
            <input
              type="range"
              min={125}
              max={250}
              step={5}
              value={windowScaleDraft}
              disabled={saving}
              onChange={(e) =>
                setWindowScaleDraft(
                  clampWindowScalePercent(Number(e.target.value)),
                )
              }
              onPointerUp={commitWindowScale}
              onTouchEnd={commitWindowScale}
              onBlur={commitWindowScale}
              onKeyUp={commitWindowScale}
              aria-label="Window scale"
            />
          </Field>
          <Field
            label={t("ShowAsUsedLabel")}
            description={t("ShowAsUsedHelper")}
            leading
          >
            <Toggle
              checked={settings.showAsUsed}
              disabled={saving}
              onChange={(v) => set({ showAsUsed: v })}
            />
          </Field>
          <Field
            label={t("ShowAllTokenAccountsLabel")}
            description={t("ShowAllTokenAccountsHelper")}
            leading
          >
            <Toggle
              checked={settings.showAllTokenAccountsInMenu}
              disabled={saving}
              onChange={(v) => set({ showAllTokenAccountsInMenu: v })}
            />
          </Field>
          <Field
            label={t("ResetTimeRelative")}
            description={t("ResetTimeRelativeHelper")}
            leading
          >
            <Toggle
              checked={settings.resetTimeRelative}
              disabled={saving}
              onChange={(v) => set({ resetTimeRelative: v })}
            />
          </Field>
        </div>
      </section>

      <FloatBarSettingsSection settings={settings} saving={saving} set={set} />
    </>
  );
}
