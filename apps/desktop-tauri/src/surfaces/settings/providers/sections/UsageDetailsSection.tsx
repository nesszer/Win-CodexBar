import { useEffect, useState } from "react";
import type {
  DeepSeekPricingStatus,
  PaceSnapshot,
  ProviderUsageSnapshot,
  RateWindowSnapshot,
  SessionEquivalentForecastSnapshot,
} from "../../../../types/bridge";
import type { LocaleKey } from "../../../../i18n/keys";
import { useLocale } from "../../../../hooks/useLocale";
import { useMonthlyLimitBlockNow } from "../../../../hooks/useMonthlyLimitBlockNow";
import { DEEPSEEK_PRICING_EVENT } from "../../../../hooks/useDeepSeekPricingStatus";
import { getDeepSeekPricingStatus } from "../../../../lib/tauri";
import { getPaceBudget, type PaceBudget } from "../../../../lib/paceBudget";
import { isMonthlyLimitBlockActive } from "../../../../lib/monthlyLimitBlock";
import { formatSignedPace } from "../../../../lib/paceSign";
import { providerAllowsPace } from "../../../../lib/providerPace";
import { localizeWindowLabel } from "../../../../lib/windowLabels";
import { paceCategory } from "../../../tray/paceCategory";
import PaceDetailsChart from "../../../../components/PaceDetailsChart";
import QuotaBurndownChart from "../../../../components/QuotaBurndownChart";

const WEEKLY_WINDOW_MINUTES = 7 * 24 * 60;

/** Upstream session-quota estimate: "Estimated: {n} session quota(s) left". */
export function formatSessionEquivalentEstimate(
  forecast: SessionEquivalentForecastSnapshot | null | undefined,
): string | null {
  if (!forecast) return null;
  const raw = forecast.estimatedWindowsToExhaustWeekly;
  if (!Number.isFinite(raw)) return null;
  const rounded = Math.round(Math.min(Math.max(raw, 0), 1_000_000) * 10) / 10;
  const display =
    Number.isInteger(rounded) || Math.abs(rounded - Math.round(rounded)) < 1e-9
      ? String(Math.round(rounded))
      : rounded.toFixed(1);
  const unit = rounded > 0 && rounded <= 1 ? "session quota" : "session quotas";
  return `Estimated: ${display} ${unit} left`;
}

export function formatBudget(value: number): string {
  return value < 10 ? value.toFixed(1).replace(/\.0$/, "") : Math.round(value).toString();
}

interface WeeklyBudget {
  id: string;
  label: string;
  snap: RateWindowSnapshot;
  budget: PaceBudget;
}

type T = (key: LocaleKey) => string;

/** Weekly (or longer) windows with an on-pace budget; blocked windows have none. */
export function weeklyBudgets(
  provider: ProviderUsageSnapshot,
  t: T,
  language: string,
  monthlyLimitBlockNow: number,
): WeeklyBudget[] {
  const windows: { id: string; label: string; snap: RateWindowSnapshot | null | undefined }[] = [
    {
      id: "primary",
      label:
        localizeWindowLabel(provider.primaryLabel, t, language, provider.primary.windowMinutes) ||
        t("DetailWindowPrimary"),
      snap: provider.providerId === "wayfinder" ? null : provider.primary,
    },
    {
      id: "secondary",
      label: localizeWindowLabel(provider.secondaryLabel, t) || t("DetailWindowSecondary"),
      snap: provider.secondary,
    },
    { id: "model-specific", label: t("DetailWindowModelSpecific"), snap: provider.modelSpecific },
    {
      id: "tertiary",
      label: localizeWindowLabel(provider.tertiaryLabel, t) || t("DetailWindowTertiary"),
      snap: provider.tertiary,
    },
    ...(provider.extraRateWindows ?? []).map((extra) => ({
      id: `extra-${extra.id}`,
      label:
        localizeWindowLabel(extra.title, t, language, extra.window.windowMinutes, extra.id) ||
        extra.title,
      snap: extra.window,
    })),
  ];
  const result: WeeklyBudget[] = [];
  for (const window of windows) {
    const snap = window.snap;
    if (!snap || snap.isExhausted || snap.isInformational === true) continue;
    if (snap.windowMinutes == null || snap.windowMinutes < WEEKLY_WINDOW_MINUTES) continue;
    if (isMonthlyLimitBlockActive(snap.monthlyLimitBlock, monthlyLimitBlockNow)) continue;
    const budget = getPaceBudget(snap);
    if (budget) result.push({ id: window.id, label: window.label, snap, budget });
  }
  return result;
}

function PaceBars({
  pace,
  reservePositive,
  t,
}: {
  pace: PaceSnapshot;
  reservePositive: boolean;
  t: T;
}) {
  const category = paceCategory(pace.stage);
  return (
    <div className="provider-usage-details__pace">
      <div className="provider-usage-details__pace-delta" data-pace={category}>
        {formatSignedPace(pace.deltaPercent, reservePositive)}
      </div>
      <div className="provider-usage-details__pace-row">
        <span>{t("PanelExpected")}</span>
        <div className="provider-usage-details__pace-track">
          <div
            className="provider-usage-details__pace-fill provider-usage-details__pace-fill--expected"
            style={{ width: `${pace.expectedUsedPercent.toFixed(1)}%` }}
          />
        </div>
      </div>
      <div className="provider-usage-details__pace-row">
        <span>{t("PanelActual")}</span>
        <div className="provider-usage-details__pace-track">
          <div
            className="provider-usage-details__pace-fill"
            data-pace={category}
            style={{ width: `${pace.actualUsedPercent.toFixed(1)}%` }}
          />
        </div>
      </div>
    </div>
  );
}

function DeepSeekPricing({ t }: { t: T }) {
  const [status, setStatus] = useState<DeepSeekPricingStatus | null>(null);
  useEffect(() => {
    let cancelled = false;
    const onPricing = (event: Event) =>
      setStatus((event as CustomEvent<DeepSeekPricingStatus>).detail);
    window.addEventListener(DEEPSEEK_PRICING_EVENT, onPricing);
    void getDeepSeekPricingStatus()
      .then((next) => {
        if (!cancelled) setStatus(next);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      window.removeEventListener(DEEPSEEK_PRICING_EVENT, onPricing);
    };
  }, []);
  if (!status) return null;
  const period =
    status.period === "peak"
      ? "DeepSeekPricingPeak"
      : status.period === "offPeak"
        ? "DeepSeekPricingOffPeak"
        : "DeepSeekPricingStandard";
  return (
    <div className="provider-usage-details__pricing" aria-label={t("DeepSeekPricingTitle")}>
      <strong>
        {t("DeepSeekPricingTitle")}: {t(period)}
      </strong>
      <span>
        {t("DeepSeekPricingCurrent")} {status.currentLocalTime}
      </span>
      <span>
        {t("DeepSeekPricingNext")} {status.nextTransitionLocalTime ?? "—"}
      </span>
      <span>
        {t("DeepSeekPricingEffective")} {status.effectiveLocalTime}
      </span>
      <small>{t("DeepSeekPricingAdvice")}</small>
    </div>
  );
}

/**
 * Settings > Providers "Usage details": the planning detail that used to sit
 * inside the tray card (on-pace budgets with the projection chart, the session
 * forecast, expected-versus-actual pace bars, the quota burndown and DeepSeek
 * pricing). The tray card keeps the upstream card anatomy only.
 */
export function UsageDetailsSection({
  provider,
  paceReservePositive = false,
}: {
  provider: ProviderUsageSnapshot | null;
  paceReservePositive?: boolean;
}) {
  const { t, language } = useLocale();
  const monthlyLimitBlockNow = useMonthlyLimitBlockNow([
    provider?.primary.monthlyLimitBlock,
    provider?.secondary?.monthlyLimitBlock,
    provider?.modelSpecific?.monthlyLimitBlock,
    provider?.tertiary?.monthlyLimitBlock,
    ...(provider?.extraRateWindows ?? []).map((extra) => extra.window.monthlyLimitBlock),
    provider?.pace?.monthlyLimitBlock,
  ]);
  if (!provider || provider.error) {
    return provider?.providerId === "deepseek" ? (
      <section className="provider-detail-section provider-usage-details">
        <h4>{t("PanelUsageDetails")}</h4>
        <DeepSeekPricing t={t} />
      </section>
    ) : null;
  }

  const paceAllowed = providerAllowsPace(provider.providerId, provider.sourceLabel);
  const budgets = paceAllowed ? weeklyBudgets(provider, t, language, monthlyLimitBlockNow) : [];
  const forecast = paceAllowed
    ? formatSessionEquivalentEstimate(provider.sessionEquivalentForecast)
    : null;
  const pace =
    paceAllowed &&
    provider.pace &&
    !isMonthlyLimitBlockActive(provider.pace.monthlyLimitBlock, monthlyLimitBlockNow)
      ? provider.pace
      : null;
  const burndown =
    provider.quotaBurndown && provider.quotaBurndown.samples.length > 0
      ? provider.quotaBurndown
      : null;
  const isDeepSeek = provider.providerId === "deepseek";

  if (budgets.length === 0 && !forecast && !pace && !burndown && !isDeepSeek) return null;

  return (
    <section className="provider-detail-section provider-usage-details">
      <h4>{t("PanelUsageDetails")}</h4>
      {pace && <PaceBars pace={pace} reservePositive={paceReservePositive} t={t} />}
      {forecast && <div className="provider-usage-details__forecast">{forecast}</div>}
      {budgets.map(({ id, label, snap, budget }) => (
        <div className="provider-usage-details__budget" key={id}>
          <div className="provider-usage-details__budget-title">
            {label} · {t("PanelOnPaceBudget")}
          </div>
          <div className="provider-usage-details__budget-values">
            {(
              [
                [t("PanelNow"), budget.now],
                [t("PanelOneHour"), budget.nextHour],
                [t("PanelFiveHours"), budget.nextFiveHours],
                [t("PanelTodayBudget"), budget.today],
              ] as const
            ).map(([name, value]) => (
              <span className="provider-usage-details__budget-value" key={name}>
                {name} {formatBudget(value)}%
              </span>
            ))}
          </div>
          <PaceDetailsChart snap={snap} t={t} />
        </div>
      ))}
      {burndown && <QuotaBurndownChart burndown={burndown} t={t} />}
      {isDeepSeek && <DeepSeekPricing t={t} />}
    </section>
  );
}
