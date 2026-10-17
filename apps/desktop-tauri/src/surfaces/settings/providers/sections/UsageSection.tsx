import type { CSSProperties } from "react";
import type { ProviderDetail, RateWindowSnapshot } from "../../../../types/bridge";
import { InventoryItemRow } from "../../../../components/InventoryRows";
import {
  groupProviderDisplayDetails,
  ProviderDisplayRow,
} from "../../../../components/ProviderDisplayRow";
import CardMetricRow, { type MetricRowDisplay } from "../../../../components/card/CardMetricRow";
import type { LocaleKey } from "../../../../i18n/keys";
import type { ResetTimeFormatMode } from "../../../../hooks/useFormattedResetTime";
import { useMonthlyLimitBlockNow } from "../../../../hooks/useMonthlyLimitBlockNow";
import type { MetricLane } from "../../../../lib/metricRowModel";
import {
  claudePlanDateNote,
  isUsageItemVisible,
  projectClaudeSections,
} from "../../../../lib/usageItemVisibility";
import { localizeProviderLabel, localizeWindowLabel } from "../../../../lib/windowLabels";

interface Props {
  provider: ProviderDetail;
  resetTimeRelative: boolean;
  /**
   * The card's row settings (% left or used, pace, quota-warning markers,
   * workday ticks). `resetTimeRelative` and the monthly block clock are
   * filled in here.
   */
  display?: Omit<MetricRowDisplay, "resetTimeRelative" | "monthlyLimitBlockNow">;
  /** Bar fill color (the provider accent); falls back to the theme bar color. */
  accentColor?: string;
  t: (key: LocaleKey) => string;
}

interface BarSpec {
  key: string;
  label: string;
  rate: RateWindowSnapshot;
  lane: MetricLane;
  resetFormatMode?: ResetTimeFormatMode;
}

/**
 * Usage rows (session / weekly / model-specific / tertiary / extras), drawn
 * with the tray card's metric row as in macOS `PreferencesProviderDetailView`:
 * bars follow "Show usage as used", carry the quota-warning markers, and the
 * pace becomes the meta line under the bar.
 */
export function UsageSection({
  provider: reportedProvider,
  resetTimeRelative,
  display,
  accentColor,
  t,
}: Props) {
  const provider = projectClaudeSections(reportedProvider.id, reportedProvider);
  const bars: BarSpec[] = [];
  if (provider.session && isUsageItemVisible(provider.hiddenUsageItemIds, "primary")) {
    bars.push({
      key: "session",
      lane: "primary",
      label: localizeWindowLabel(provider.primaryLabel ?? undefined, t) || t("ProviderSessionLabel"),
      rate: provider.session,
    });
  }
  if (provider.weekly && isUsageItemVisible(provider.hiddenUsageItemIds, "secondary")) {
    bars.push({
      key: "weekly",
      lane: "secondary",
      label: localizeWindowLabel(provider.secondaryLabel ?? undefined, t) || t("ProviderWeeklyLabel"),
      rate: provider.weekly,
    });
  }
  if (provider.modelSpecific && isUsageItemVisible(provider.hiddenUsageItemIds, "model-specific")) {
    bars.push({
      key: "modelSpecific",
      lane: "tertiary",
      label: t("DetailWindowModelSpecific"),
      rate: provider.modelSpecific,
    });
  }
  if (provider.tertiary && isUsageItemVisible(provider.hiddenUsageItemIds, "tertiary")) {
    bars.push({
      key: "tertiary",
      lane: "tertiary",
      label: t("DetailWindowTertiary"),
      rate: provider.tertiary,
    });
  }
  for (const extra of provider.extraRateWindows ?? []) {
    if (!isUsageItemVisible(provider.hiddenUsageItemIds, `extra-${extra.id}`)) {
      continue;
    }
    bars.push({
      key: extra.id,
      label: extra.title,
      rate: extra.window,
      lane: "extra",
      resetFormatMode: extra.id === "reset-credits" ? "expires" : "reset",
    });
  }

  // Card-hidden windows only feed the tray icon and float bar, as on MenuCard.
  const shownBars = bars.filter((bar) => !bar.rate.hiddenOnCard);

  // Cached snapshots outlive the pool reset; re-check blocks on the clock.
  const monthlyLimitBlockNow = useMonthlyLimitBlockNow(
    shownBars.map((bar) => bar.rate.monthlyLimitBlock),
  );
  const inventory = provider.inventory ?? [];
  const displayDetails = provider.displayDetails ?? [];
  const displayDetailGroups = groupProviderDisplayDetails(displayDetails);
  const planDateNote = claudePlanDateNote(provider.id, provider.subscription, t);
  if (
    shownBars.length === 0 &&
    !planDateNote &&
    inventory.length === 0 &&
    displayDetails.length === 0
  ) {
    return null;
  }

  return (
    <section
      className="provider-detail-section provider-usage-main"
      style={accentColor ? ({ "--provider-accent": accentColor } as CSSProperties) : undefined}
    >
      <h4>{t("ProviderUsage")}</h4>
      {shownBars.map((b) => (
        <CardMetricRow
          key={b.key}
          title={b.label}
          snap={b.rate}
          lane={b.lane}
          providerId={provider.id}
          resetFormatMode={b.resetFormatMode}
          display={{ ...display, resetTimeRelative, monthlyLimitBlockNow }}
        />
      ))}
      {planDateNote && <div className="provider-usage-inventory">{planDateNote}</div>}
      {inventory.map((item) => (
        <InventoryItemRow
          key={item.id}
          item={item}
          resetTimeRelative={resetTimeRelative}
          lineClassName="provider-usage-inventory"
        />
      ))}
      {displayDetailGroups.map((group) => (
        <div key={group.id}>
          {group.title && (
            <div
              className="provider-detail-field__label"
              role="heading"
              aria-level={5}
            >
              {localizeProviderLabel(group.title, t)}
            </div>
          )}
          {group.rows.map((detail) => (
            <ProviderDisplayRow
              key={detail.id}
              detail={detail}
              lineClassName="provider-usage-inventory"
              trackClassName="provider-usage-bar__track"
              fillClassName="provider-usage-bar__fill"
            />
          ))}
        </div>
      ))}
    </section>
  );
}
