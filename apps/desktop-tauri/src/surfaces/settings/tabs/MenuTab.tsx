import { useEffect, useState } from "react";
import { useDraftNumber } from "../../../hooks/useDraftNumber";
import { useLocale } from "../../../hooks/useLocale";
import { Field, Select, SettingsGroup, Toggle } from "../../../components/FormControls";
import type {
  CostSummaryDisplayStyle,
  OverviewLayout,
  WorkdayTickStyle,
} from "../../../types/bridge";
import type { TabProps } from "../settingsTabs";
import FloatBarSettingsSection from "../../../floatbar/SettingsSection";
import { normalizeCostPeriod } from "../../../lib/costPeriod";
import {
  clampTrayScalePercent,
  TRAY_SCALE_MAX,
  TRAY_SCALE_MIN,
  TRAY_SCALE_STEP,
} from "../../../lib/trayScale";
import CostPeriodControl from "./CostPeriodControl";

/** macOS "Work days" choices; Automatic (empty) saves null. */
const WORK_DAY_PRESETS = [4, 5, 7];

function parseSshHosts(value: string): string[] {
  return value
    .split(/[,\n]/)
    .map((host) => host.trim())
    .filter(Boolean);
}

/**
 * Settings > Menu, laid out like the macOS "Menu" pane: how usage rows read,
 * the Windows-only panel options, Overview content, agent sessions and the
 * local cost summary, then the float bar.
 */
export default function MenuTab({ settings, set, saving }: TabProps) {
  const { t } = useLocale();
  const panelScale = useDraftNumber(clampTrayScalePercent(settings.trayScalePercent));
  const commitPanelScale = () => {
    panelScale.commit(panelScale.draft, (value) => set({ trayScalePercent: value }));
  };
  const [sshHostsDraft, setSshHostsDraft] = useState(() =>
    (settings.agentSessionSshHosts ?? []).join(", "),
  );
  useEffect(() => {
    if (!saving) setSshHostsDraft((settings.agentSessionSshHosts ?? []).join(", "));
  }, [saving, settings.agentSessionSshHosts]);

  const workDays = settings.weeklyProgressWorkDays ?? null;
  const workDayOptions = [
    { value: "", label: t("Automatic") },
    ...[...new Set([...WORK_DAY_PRESETS, ...(workDays === null ? [] : [workDays])])]
      .sort((a, b) => a - b)
      .map((days) => ({ value: String(days), label: t("WorkDaysCount").replace("{}", String(days)) })),
  ];

  return (
    <>
      <SettingsGroup title={t("MenuUsageSectionTitle")}>
        <Field label={t("UsageBarsFillLabel")}>
          <Select
            variant="menu"
            value={settings.showAsUsed ? "used" : "remaining"}
            disabled={saving}
            options={[
              { value: "remaining", label: t("UsageBarsFillRemaining") },
              { value: "used", label: t("UsageBarsFillUsed") },
            ]}
            onChange={(v) => set({ showAsUsed: v === "used" })}
          />
        </Field>
        <Field label={t("QuotaWarningMarkersLabel")} description={t("QuotaWarningMarkersHelper")}>
          <Toggle
            variant="switch"
            checked={settings.quotaWarningMarkersVisible ?? true}
            disabled={saving}
            onChange={(v) => set({ quotaWarningMarkersVisible: v })}
          />
        </Field>
        <Field label={t("ShowPace")} description={t("ShowPaceHelper")}>
          <Toggle
            variant="switch"
            checked={settings.showPace ?? true}
            disabled={saving}
            onChange={(v) => set({ showPace: v })}
          />
        </Field>
        <Field label={t("WorkDaysLabel")}>
          <Select
            variant="menu"
            value={workDays === null ? "" : String(workDays)}
            disabled={saving}
            options={workDayOptions}
            onChange={(v) => set({ weeklyProgressWorkDays: v === "" ? null : Number(v) })}
          />
        </Field>
        <Field
          label={t("WorkdayTickAppearanceLabel")}
          description={t("WorkdayTickAppearanceHelper")}
        >
          <Select
            variant="menu"
            value={settings.workdayTickStyle ?? "subtle"}
            disabled={saving || workDays === null}
            options={[
              { value: "hidden", label: t("WorkdayTickHidden") },
              { value: "subtle", label: t("WorkdayTickSubtle") },
              { value: "high_contrast", label: t("WorkdayTickHighContrast") },
            ]}
            onChange={(v) => set({ workdayTickStyle: v as WorkdayTickStyle })}
          />
        </Field>
        <Field label={t("ResetTimesLabel")}>
          <Select
            variant="menu"
            value={settings.resetTimeRelative ? "countdown" : "clock"}
            disabled={saving}
            options={[
              { value: "countdown", label: t("ResetTimesCountdown") },
              { value: "clock", label: t("ResetTimesClockTime") },
            ]}
            onChange={(v) => set({ resetTimeRelative: v === "countdown" })}
          />
        </Field>
      </SettingsGroup>

      <SettingsGroup title={t("MenuWindowsSectionTitle")}>
        <Field label={t("TrayPanelAlwaysOnTopLabel")} description={t("TrayPanelAlwaysOnTopHelper")}>
          <Toggle
            variant="switch"
            checked={settings.trayPanelAlwaysOnTop}
            disabled={saving}
            onChange={(v) => set({ trayPanelAlwaysOnTop: v })}
          />
        </Field>
        <Field
          label={t("ShowResetWhenExhausted")}
          description={t("ShowResetWhenExhaustedHelper")}
        >
          <Toggle
            variant="switch"
            checked={settings.showResetWhenExhausted}
            disabled={saving}
            onChange={(v) => set({ showResetWhenExhausted: v })}
          />
        </Field>
        <Field
          label={`${t("PanelScaleLabel")} (${panelScale.draft}%)`}
          description={t("PanelScaleHelper")}
        >
          <input
            type="range"
            min={TRAY_SCALE_MIN}
            max={TRAY_SCALE_MAX}
            step={TRAY_SCALE_STEP}
            value={panelScale.draft}
            disabled={saving}
            onChange={(e) => panelScale.setDraft(Number(e.target.value))}
            onPointerUp={commitPanelScale}
            onTouchEnd={commitPanelScale}
            onBlur={commitPanelScale}
            onKeyUp={commitPanelScale}
            aria-label={t("PanelScaleLabel")}
          />
        </Field>
      </SettingsGroup>

      <SettingsGroup title={t("MenuContentSectionTitle")}>
        <Field label={t("HighlightMenuCardsOnHoverLabel")}>
          <Toggle
            variant="switch"
            checked={settings.highlightMenuCardsOnHover ?? true}
            disabled={saving}
            onChange={(v) => set({ highlightMenuCardsOnHover: v })}
          />
        </Field>
        <Field label={t("OverviewLayoutLabel")} description={t("OverviewLayoutHelper")}>
          <Select
            variant="menu"
            value={settings.overviewLayout}
            disabled={saving}
            options={[
              { value: "detailed", label: t("OverviewLayoutDetailed") },
              { value: "compact", label: t("OverviewLayoutCompact") },
            ]}
            onChange={(v) => set({ overviewLayout: v as OverviewLayout })}
          />
        </Field>
        <Field label={t("ShowAllTokenAccountsLabel")} description={t("ShowAllTokenAccountsHelper")}>
          <Toggle
            variant="switch"
            checked={settings.showAllTokenAccountsInMenu}
            disabled={saving}
            onChange={(v) => set({ showAllTokenAccountsInMenu: v })}
          />
        </Field>
      </SettingsGroup>

      <SettingsGroup title={t("AgentSessionsTitle")}>
        <Field
          label={t("AgentSessionsStayAwakeLabel")}
          description={t("AgentSessionsStayAwakeHelper")}
        >
          <Toggle
            variant="switch"
            checked={settings.stayAwakeEnabled ?? false}
            disabled={saving}
            onChange={(v) => set({ stayAwakeEnabled: v })}
          />
        </Field>
        <Field
          label={t("AgentSessionsEnableLabel")}
          description={t("AgentSessionsEnableHelper")}
        >
          <Toggle
            variant="switch"
            checked={settings.agentSessionsEnabled ?? false}
            disabled={saving}
            onChange={(v) => set({ agentSessionsEnabled: v })}
          />
        </Field>
        <Field
          label={t("AgentSessionsSshHostsLabel")}
          description={t("AgentSessionsSshHostsHelper")}
        >
          <input
            type="text"
            className="text-input"
            value={sshHostsDraft}
            aria-label={t("AgentSessionsSshHostsLabel")}
            placeholder="user@host, user@host"
            disabled={saving || !settings.agentSessionsEnabled}
            onChange={(event) => setSshHostsDraft(event.target.value)}
            onBlur={() => set({ agentSessionSshHosts: parseSshHosts(sshHostsDraft) })}
            onKeyDown={(event) => {
              if (event.key === "Enter") event.currentTarget.blur();
            }}
          />
        </Field>
      </SettingsGroup>

      <SettingsGroup title={t("MenuCostSummarySectionTitle")}>
        <Field
          label={t("CostSummaryDisplayStyle")}
          description={t("CostSummaryDisplayStyleHelper")}
        >
          <Select
            variant="menu"
            value={settings.costSummaryDisplayStyle}
            disabled={saving}
            options={[
              { value: "compact", label: t("CostSummaryStyleCompact") },
              { value: "detailed", label: t("CostSummaryStyleDetailed") },
              { value: "hidden", label: t("CostSummaryStyleHidden") },
            ]}
            onChange={(v) => set({ costSummaryDisplayStyle: v as CostSummaryDisplayStyle })}
          />
        </Field>
        <Field
          label={t("CostPeriodHistoryWindow")}
          description={t("CostPeriodHistoryWindowHelper")}
        >
          <CostPeriodControl
            value={normalizeCostPeriod(settings.costReportingPeriod)}
            disabled={saving}
            onChange={(raw) => set({ costReportingPeriod: raw })}
            t={t}
          />
        </Field>
      </SettingsGroup>

      <FloatBarSettingsSection settings={settings} saving={saving} set={set} />
    </>
  );
}
