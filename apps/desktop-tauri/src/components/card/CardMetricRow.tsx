import { useEffect, useState } from "react";
import type { RateWindowSnapshot, WorkdayTickStyle } from "../../types/bridge";
import { useLocale } from "../../hooks/useLocale";
import {
  useFormattedResetTime,
  type ResetTimeFormatMode,
} from "../../hooks/useFormattedResetTime";
import { isMonthlyLimitBlockActive } from "../../lib/monthlyLimitBlock";
import { windowDetailText } from "../../lib/usageWindows";
import { localizeProviderText } from "../../lib/providerText";
import {
  metricResetText,
  metricRowPresentation,
  type MetricLane,
  type UsageThresholdSettings,
} from "../../lib/metricRowModel";
import UsageProgressBar from "../UsageProgressBar";

export interface MetricEntry {
  id: string;
  label: string;
  snap: RateWindowSnapshot;
  lane: MetricLane;
  resetFormatMode?: ResetTimeFormatMode;
}

export interface MetricRowDisplay {
  resetTimeRelative: boolean;
  showResetWhenExhausted?: boolean;
  showPace?: boolean;
  showAsUsed?: boolean;
  compactOverview?: boolean;
  monthlyLimitBlockNow?: number;
  usageThresholds?: UsageThresholdSettings | null;
  quotaWarningMarkersVisible?: boolean;
  weeklyProgressWorkDays?: number | null;
  workdayTickStyle?: WorkdayTickStyle;
}

/** Bar thickness in px: the tray card draws a thick, fully rounded bar. */
const BAR_HEIGHT = 8;

function useCountdownNow(ticking: boolean): number {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!ticking) return;
    const id = window.setInterval(() => setTick((tick) => tick + 1), 30_000);
    return () => window.clearInterval(id);
  }, [ticking]);
  return Date.now();
}

/**
 * One rate window: the title, the bar, the percent and the reset on one line
 * under the bar, then the pace meta line.
 * Budgets, the pace chart and the session forecast live in Settings.
 */
export default function CardMetricRow({
  title,
  snap,
  lane,
  providerId,
  display,
  resetFormatMode,
}: {
  title: string;
  snap: RateWindowSnapshot;
  lane: MetricLane;
  providerId: string;
  display: MetricRowDisplay;
  resetFormatMode?: ResetTimeFormatMode;
}) {
  const { t } = useLocale();
  const {
    resetTimeRelative,
    showResetWhenExhausted = false,
    showPace = true,
    showAsUsed = false,
    compactOverview = false,
    monthlyLimitBlockNow,
    usageThresholds = null,
    quotaWarningMarkersVisible = true,
    weeklyProgressWorkDays = null,
    workdayTickStyle = "subtle",
  } = display;
  // Upstream 0.69.0 #4091: a longer exhausted pool (Kimi's monthly membership)
  // blocks this window until the pool resets. Raw percentages stay untouched.
  const blocked = isMonthlyLimitBlockActive(
    snap.monthlyLimitBlock,
    monthlyLimitBlockNow ?? Date.now(),
  );
  const isInformational = snap.isInformational === true;
  const informationalReset = useFormattedResetTime(
    isInformational && !blocked ? snap.resetsAt : null,
    null,
    resetTimeRelative,
    resetFormatMode ?? "reset",
  );
  const now = useCountdownNow(!isInformational && !blocked && snap.resetsAt != null);
  const detailText = localizeProviderText(windowDetailText(snap), t) || null;
  if (blocked) {
    // Upstream `MetricRow` status layout: title plus one secondary status line.
    return (
      <div className="menu-metric menu-metric--blocked">
        <span className="menu-metric__title">{title}</span>
        <span className="menu-metric__status">{t("PanelBlockedByMonthlyLimit")}</span>
      </div>
    );
  }
  if (isInformational) {
    const infoPrimary =
      localizeProviderText(snap.resetDescription?.trim(), t) || informationalReset || "—";
    return (
      <div className="menu-metric">
        <span className="menu-metric__title">{title}</span>
        <div className="menu-metric__row">
          <span className="menu-metric__pct">{infoPrimary}</span>
          {!compactOverview &&
            snap.resetDescription?.trim() &&
            informationalReset &&
            informationalReset !== infoPrimary && (
              <span className="menu-metric__reset">{informationalReset}</span>
            )}
        </div>
        {!compactOverview && detailText && (
          <div className="menu-metric__detail">{detailText}</div>
        )}
      </div>
    );
  }
  const row = metricRowPresentation(
    {
      snap,
      lane,
      providerId,
      resetText: metricResetText(snap, resetTimeRelative, now, t),
      compact: compactOverview,
      showAsUsed,
      showResetWhenExhausted,
      paceEnabled: showPace,
      usageThresholds,
      quotaWarningMarkersVisible,
      weeklyProgressWorkDays,
      workdayTickStyle,
      now,
    },
    t,
  );
  return (
    <div className="menu-metric">
      <span className="menu-metric__title">{title}</span>
      <UsageProgressBar bar={row.bar} label={title} height={BAR_HEIGHT} />
      {(row.percentText || row.resetText) && (
        <div className="menu-metric__head">
          {row.percentText && <span className="menu-metric__percent">{row.percentText}</span>}
          {row.resetText && <span className="menu-metric__reset">{row.resetText}</span>}
        </div>
      )}
      {row.metaText && <div className="menu-metric__meta">{row.metaText}</div>}
      {!compactOverview && detailText && (
        <div className="menu-metric__detail">{detailText}</div>
      )}
      {!compactOverview && snap.isExhausted && (
        <div className="menu-metric__exhausted">{t("DetailWindowExhausted")}</div>
      )}
    </div>
  );
}
