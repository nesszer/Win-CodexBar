import { useCallback, useState } from "react";
import { Field, SettingSelect, SettingToggle, type SettingsControl } from "../components/FormControls";
import { useDraftNumber } from "../hooks/useDraftNumber";
import { useLocale } from "../hooks/useLocale";

/**
 * Settings UI block for the floating capacity bar. Rendered as one row
 * in the Display tab — kept in this module so the Display tab only
 * imports a single component.
 */
export default function FloatBarSettingsSection({ settings, saving, set }: SettingsControl) {
  const { t } = useLocale();
  const ctl = { settings, set, saving };
  const opacity = useDraftNumber(settings.floatBarOpacity);
  const scale = useDraftNumber(settings.floatBarScale);
  const commitOpacity = () => {
    opacity.commit(opacity.draft, (value) => set({ floatBarOpacity: value }));
  };
  const commitScale = () => {
    scale.commit(scale.draft, (value) => set({ floatBarScale: value }));
  };

  return (
    <section className="settings-section">
      <h3 className="settings-section__title">{t("FloatBarSectionTitle")}</h3>
      <div className="settings-section__group">
        <SettingToggle
          ctl={ctl}
          field="floatBarEnabled"
          label={t("FloatBarShowFloatingBar")}
          description={t("FloatBarShowFloatingBarHelper")}
        />
        <SettingSelect
          ctl={ctl}
          field="floatBarOrientation"
          label={t("FloatBarOrientation")}
          description={t("FloatBarOrientationHelper")}
          disabled={saving || !settings.floatBarEnabled}
          options={[
            { value: "horizontal", label: t("FloatBarOrientationHorizontal") },
            { value: "vertical", label: t("FloatBarOrientationVertical") },
          ]}
        />
        <SettingSelect
          ctl={ctl}
          field="floatBarStyle"
          label={t("FloatBarStyle")}
          description={t("FloatBarStyleHelper")}
          disabled={saving || !settings.floatBarEnabled}
          options={[
            { value: "floating", label: t("FloatBarStyleFloating") },
            { value: "taskbar", label: t("FloatBarStyleTaskbar") },
          ]}
        />
        <Field
          label={`${t("FloatBarOpacity")} (${opacity.draft}%)`}
          description={t("FloatBarOpacityHelper")}
        >
          <input
            type="range"
            min={30}
            max={100}
            step={5}
            value={opacity.draft}
            disabled={!settings.floatBarEnabled}
            onChange={(e) => opacity.setDraft(Number(e.target.value))}
            onPointerUp={commitOpacity}
            onTouchEnd={commitOpacity}
            onBlur={commitOpacity}
            onKeyUp={commitOpacity}
            aria-label={t("FloatBarOpacityAriaLabel")}
          />
        </Field>
        <Field
          label={`${t("FloatBarSize")} (${scale.draft}%)`}
          description={t("FloatBarSizeHelper")}
        >
          <input
            type="range"
            min={75}
            max={200}
            step={5}
            value={scale.draft}
            disabled={!settings.floatBarEnabled}
            onChange={(e) => scale.setDraft(Number(e.target.value))}
            onPointerUp={commitScale}
            onTouchEnd={commitScale}
            onBlur={commitScale}
            onKeyUp={commitScale}
            aria-label={t("FloatBarSizeAriaLabel")}
          />
        </Field>
        <SettingToggle
          ctl={ctl}
          field="floatBarShowCost"
          label={t("FloatBarShowCost")}
          description={t("FloatBarShowCostDescription")}
          disabled={saving || !settings.floatBarEnabled}
        />
        <SettingToggle
          ctl={ctl}
          field="floatBarShowResetInline"
          label={t("FloatBarShowResetInline")}
          description={t("FloatBarShowResetInlineHelper")}
          disabled={saving || !settings.floatBarEnabled}
        />
        <SettingToggle
          ctl={ctl}
          field="floatBarShowWeekly"
          label={t("FloatBarShowWeekly")}
          description={t("FloatBarShowWeeklyHelper")}
          disabled={saving || !settings.floatBarEnabled}
        />
        <SettingToggle
          ctl={ctl}
          field="floatBarDarkText"
          label={t("FloatBarInvertColors")}
          description={t("FloatBarInvertColorsHelper")}
          disabled={saving || !settings.floatBarEnabled}
        />
        <SettingToggle
          ctl={ctl}
          field="floatBarClickThrough"
          label={t("FloatBarClickThrough")}
          description={t("FloatBarClickThroughHelper")}
          disabled={saving || !settings.floatBarEnabled}
        />
      </div>
    </section>
  );
}
