import { useCallback, useEffect, useState } from "react";
import type {
  DailyCostPoint,
  PaceSnapshot,
  ProviderChartData,
  ProviderLocalUsageSummary,
  ProviderUsageSnapshot,
  RateWindowSnapshot,
} from "../types/bridge";
import { getProviderChartData } from "../lib/tauri";
import { useLocale } from "../hooks/useLocale";
import { useFormattedResetTime } from "../hooks/useFormattedResetTime";
import type { LocaleKey } from "../i18n/keys";
import { paceCategory } from "../surfaces/tray/paceCategory";
import { SimpleBarChart, StackedBarChart } from "./MiniBarChart";
import { DEMO_ENABLED } from "../lib/demoProviders";
import { providerSupportsChartData } from "../lib/providerCharts";
import { getPaceBudget } from "../lib/paceBudget";
import PaceDetailsChart from "./PaceDetailsChart";

/** Small copy-to-clipboard button matching macOS CopyIconButton (doc.on.doc → checkmark). */
function CopyIconButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(text).catch(() => {});
    setCopied(true);
    setTimeout(() => setCopied(false), 900);
  }, [text]);
  return (
    <button
      type="button"
      className="menu-card__copy-btn"
      onClick={handleCopy}
      aria-label={copied ? "Copied" : "Copy error"}
      title={copied ? "Copied" : "Copy error"}
    >
      {copied ? "✓" : (
        <svg width="12" height="12" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
          <rect x="5" y="5" width="9" height="9" rx="1.5" stroke="currentColor" strokeWidth="1.5"/>
          <path d="M11 3V2.5A1.5 1.5 0 009.5 1H2.5A1.5 1.5 0 001 2.5v7A1.5 1.5 0 002.5 11H3" stroke="currentColor" strokeWidth="1.5"/>
        </svg>
      )}
    </button>
  );
}

interface MenuCardProps {
  provider: ProviderUsageSnapshot;
  hideEmail: boolean;
  resetTimeRelative: boolean;
  showAsUsed?: boolean;
  compactMetrics?: boolean;
  onLayoutChange?: () => void;
}

function maskEmail(email: string): string {
  const at = email.indexOf("@");
  if (at <= 1) return "••••@••••";
  return email[0] + "•".repeat(at - 1) + email.slice(at);
}

function formatCurrency(amount: number, code: string): string {
  try {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: code,
    }).format(amount);
  } catch {
    return `${code} ${amount.toFixed(2)}`;
  }
}

const DEMO_COST_BARS = [
  0.58, 0.73, 0.66, 0.62, 0.26, 0.86, 0.17, 0.10, 0.21, 0.19,
  0.23, 0.38, 0.09, 0.34, 0.24, 1.0, 0.42, 0.51, 0.14, 0.08,
  0.20, 0.15, 0.22, 0.11, 0.18, 0.41, 0.55, 0.16, 0.44, 0.31,
];

const DEMO_LOCAL_USAGE: Record<string, ProviderLocalUsageSummary> = {
  codex: {
    todayCost: 75.24,
    thirtyDayCost: 3442.16,
    thirtyDayTokens: 4_700_000_000,
    latestTokens: 115_000_000,
    topModel: "gpt-5.5",
    estimateNote: "Estimated from local logs; may differ from your bill",
  },
  claude: {
    todayCost: null,
    thirtyDayCost: null,
    thirtyDayTokens: 584_000,
    latestTokens: 352_000,
    topModel: "glm-4.6",
    estimateNote:
      "Estimated from local Claude logs at API rates; token totals may differ from your bill",
  },
};

function formatCompactCount(value: number | null): string {
  if (value == null || value <= 0) return "—";
  return new Intl.NumberFormat("en-US", {
    notation: "compact",
    maximumFractionDigits: value >= 1_000_000 ? 1 : 0,
  }).format(value);
}

function localUsageForDemo(providerId: string): ProviderLocalUsageSummary | null {
  return DEMO_ENABLED ? DEMO_LOCAL_USAGE[providerId] ?? null : null;
}

function costBarsForDemo(): DailyCostPoint[] {
  return DEMO_COST_BARS.map((value, index) => ({
    date: String(index),
    value,
  }));
}

function LocalUsageBlock({
  providerId,
  summary,
  costHistory,
}: {
  providerId: string;
  summary: ProviderLocalUsageSummary;
  costHistory: DailyCostPoint[];
}) {
  const isCodex = providerId === "codex";
  const visibleHistory = costHistory
    .slice(-30)
    .filter((point) => point.value > 0);
  const maxCost = Math.max(...visibleHistory.map((point) => point.value), 0);

  return (
    <section className="menu-card__group menu-card__local-usage">
      <div className="menu-card__local-grid">
        <div>
          <span className="menu-card__local-label">Today</span>
          <strong>
            {summary.todayCost != null
              ? formatCurrency(summary.todayCost, "USD")
              : "—"}
          </strong>
        </div>
        <div>
          <span className="menu-card__local-label">30d cost</span>
          <strong>
            {summary.thirtyDayCost != null
              ? formatCurrency(summary.thirtyDayCost, "USD")
              : "—"}
          </strong>
        </div>
        <div>
          <span className="menu-card__local-label">30d tokens</span>
          <strong>{formatCompactCount(summary.thirtyDayTokens)}</strong>
        </div>
        <div>
          <span className="menu-card__local-label">Latest tokens</span>
          <strong>{formatCompactCount(summary.latestTokens)}</strong>
        </div>
      </div>

      {isCodex && visibleHistory.length > 0 && (
        <div className="menu-card__local-chart" aria-label="30 day cost histogram">
          {visibleHistory.map((point, index) => (
            <span
              key={`${point.date}-${index}`}
              style={{
                height: `${Math.max(4, Math.round((point.value / maxCost) * 64))}px`,
              }}
              title={`${point.date}: ${formatCurrency(point.value, "USD")}`}
            />
          ))}
        </div>
      )}

      <div className="menu-card__local-note">
        {summary.topModel && <strong>Top model: {summary.topModel}</strong>}
        <span>{summary.estimateNote}</span>
      </div>
    </section>
  );
}

/**
 * Format a backend `updatedAt` timestamp as a short relative string
 * ("just now", "2m ago", "3h ago", "5d ago"). If the value isn't a parseable
 * ISO datetime, return it unchanged so manual / preformatted strings still
 * render verbatim.
 */
function formatRelative(updatedAt: string): string {
  const ts = Date.parse(updatedAt);
  if (Number.isNaN(ts)) return updatedAt;
  const diffSec = Math.max(0, Math.round((Date.now() - ts) / 1000));
  if (diffSec < 60) return "just now";
  const diffMin = Math.round(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.round(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDay = Math.round(diffHr / 24);
  return `${diffDay}d ago`;
}

function displayPlanName(planName: string | null): string | null {
  if (!planName) return null;
  const normalized = planName.trim().toLowerCase();
  if (normalized === "default_claude_ai") return "Claude AI";
  return planName;
}

function paceStageKey(stage: PaceSnapshot["stage"]): LocaleKey {
  switch (stage) {
    case "on_track":
      return "DetailPaceOnTrack";
    case "slightly_ahead":
      return "DetailPaceSlightlyAhead";
    case "ahead":
      return "DetailPaceAhead";
    case "far_ahead":
      return "DetailPaceFarAhead";
    case "slightly_behind":
      return "DetailPaceSlightlyBehind";
    case "behind":
      return "DetailPaceBehind";
    case "far_behind":
      return "DetailPaceFarBehind";
    default:
      return "DetailPaceOnTrack";
  }
}

type UsageLevel = "normal" | "high" | "critical" | "exhausted";
const WEEKLY_WINDOW_MINUTES = 7 * 24 * 60;

function levelOf(remainPct: number, exhausted: boolean): UsageLevel {
  if (exhausted) return "exhausted";
  if (remainPct <= 5) return "critical";
  if (remainPct <= 25) return "high";
  return "normal";
}

interface MetricEntry {
  id: string;
  label: string;
  snap: RateWindowSnapshot;
}

type MetricPaceView =
  | { kind: "budget"; budget: NonNullable<ReturnType<typeof getPaceBudget>> }
  | { kind: "reserve"; percent: number; description: string | null }
  | { kind: "none" };

function getMetricPaceView(snap: RateWindowSnapshot): MetricPaceView {
  if (snap.isExhausted) return { kind: "none" };

  const isWeeklyWindow =
    snap.windowMinutes != null && snap.windowMinutes >= WEEKLY_WINDOW_MINUTES;
  const budget = isWeeklyWindow ? getPaceBudget(snap) : null;
  if (budget) return { kind: "budget", budget };

  if (snap.reservePercent != null) {
    return {
      kind: "reserve",
      percent: snap.reservePercent,
      description: snap.reserveDescription,
    };
  }

  return { kind: "none" };
}

/**
 * Single metric row inside the card — mirrors upstream `MetricRow`:
 *   • title (body / medium)
 *   • UsageProgressBar (capsule, 6pt)
 *   • HStack: "N% used"  ··  reset countdown (right-aligned, secondary)
 */
function MetricRow({
  title,
  snap,
  exhaustedLabel,
  resetTimeRelative,
  showAsUsed,
  expanded,
  onToggleExpanded,
}: {
  title: string;
  snap: RateWindowSnapshot;
  exhaustedLabel: string;
  resetTimeRelative: boolean;
  showAsUsed: boolean;
  expanded: boolean;
  onToggleExpanded: () => void;
}) {
  const usedPct = Number.isFinite(snap.usedPercent) ? Math.max(0, snap.usedPercent) : 0;
  const barPct = Math.min(100, usedPct);
  const remain = 100 - usedPct;
  const displayPct = showAsUsed ? usedPct : Math.max(0, remain);
  const barDisplayPct = showAsUsed ? barPct : Math.max(0, Math.min(100, remain));
  const displayLabel = showAsUsed ? "used" : "left";
  const level = levelOf(remain, snap.isExhausted);
  const resetText = useFormattedResetTime(
    snap.resetsAt,
    snap.resetDescription,
    resetTimeRelative,
  );
  const paceView = getMetricPaceView(snap);
  const formatBudget = (value: number) =>
    value < 10 ? value.toFixed(1).replace(/\.0$/, "") : Math.round(value).toString();
  return (
    <div className="menu-metric">
      <span className="menu-metric__title">{title}</span>
      <div className="menu-metric__bar">
        <div className="menu-metric__bar-fill" data-level={level} style={{ width: `${barDisplayPct}%` }} />
      </div>
      <div className="menu-metric__row">
        <span className="menu-metric__pct">{Math.round(displayPct)}% {displayLabel}</span>
        {resetText && (
          <span className="menu-metric__reset">{resetText}</span>
        )}
      </div>
      {snap.isExhausted && (
        <div className="menu-metric__exhausted">{exhaustedLabel}</div>
      )}
      {paceView.kind === "budget" && (
        <div className="menu-metric__budget">
          <button
            type="button"
            className="menu-metric__budget-header"
            onClick={onToggleExpanded}
            aria-expanded={expanded}
          >
            <span>On-pace budget</span>
            {snap.reserveDescription && <span>{snap.reserveDescription}</span>}
          </button>
          <div className="menu-metric__budget-pills">
            {[
              ["now", paceView.budget.now],
              ["1h", paceView.budget.nextHour],
              ["5h", paceView.budget.nextFiveHours],
              ["today", paceView.budget.today],
            ].map(([label, value]) => (
              <span className="menu-metric__budget-pill" key={String(label)}>
                {label} {formatBudget(Number(value))}%
              </span>
            ))}
          </div>
          {expanded && <PaceDetailsChart snap={snap} />}
        </div>
      )}
      {paceView.kind === "reserve" && (
        <div className="menu-metric__row menu-metric__reserve">
          <span className="menu-metric__pct">{Math.round(paceView.percent)}% in reserve</span>
          {paceView.description && (
            <span className="menu-metric__reset">{paceView.description}</span>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * Provider card — direct mirror of SwiftUI `UsageMenuCardView`.
 *
 * Layout (top to bottom):
 *   1. Header VStack(spacing: 3)
 *        – HStack: providerName (headline/semibold)  ··  email (subheadline/secondary, right)
 *        – HStack: subtitle "source · updated"        ··  plan (footnote/secondary, right)
 *   2. Divider (1pt)
 *   3. VStack(spacing: 12)
 *        – Metrics group VStack(spacing: 12) of MetricRow
 *        – (Divider) Cost group: title (body/medium) + session line + month line (footnote)
 *        – (Divider) Pace group (Tauri-only addition; placed last)
 *        – (Divider) Charts group (Tauri-only addition; placed last)
 *
 * Padding: upstream v0.32.2 uses wider horizontal card padding and slightly
 * taller header/content vertical padding so account/plan rows can breathe.
 */
export default function MenuCard({
  provider,
  hideEmail,
  resetTimeRelative,
  showAsUsed = false,
  compactMetrics = false,
  onLayoutChange,
}: MenuCardProps) {
  const { t } = useLocale();
  const [chartData, setChartData] = useState<ProviderChartData | null>(null);
  const [expandedPaceWindow, setExpandedPaceWindow] = useState<string | null>(null);
  const formattedCostReset = useFormattedResetTime(
    provider.cost?.resetsAt ?? null,
    null,
    resetTimeRelative,
  );

  useEffect(() => {
    if (DEMO_ENABLED || !providerSupportsChartData(provider.providerId)) {
      setChartData(null);
      return;
    }
    let cancelled = false;
    setChartData(null);
    getProviderChartData(
      provider.providerId,
      provider.accountEmail ?? undefined,
    )
      .then((data) => {
        if (!cancelled) {
          setChartData(data);
          requestAnimationFrame(() => onLayoutChange?.());
        }
      })
      .catch(() => {
        /* chart data is best-effort */
      });
    return () => {
      cancelled = true;
    };
  }, [provider.providerId, provider.accountEmail, onLayoutChange]);

  const email = provider.accountEmail
    ? hideEmail
      ? maskEmail(provider.accountEmail)
      : provider.accountEmail
    : null;
  const planName = displayPlanName(provider.planName);

  const metrics: MetricEntry[] = [
    {
      id: "primary",
      label: provider.primaryLabel ?? t("DetailWindowPrimary"),
      snap: provider.primary,
    },
  ];
  if (provider.secondary)
    metrics.push({
      id: "secondary",
      label: provider.secondaryLabel ?? t("DetailWindowSecondary"),
      snap: provider.secondary,
    });
  if (provider.modelSpecific)
    metrics.push({
      id: "model-specific",
      label: t("DetailWindowModelSpecific"),
      snap: provider.modelSpecific,
    });
  if (provider.tertiary)
    metrics.push({
      id: "tertiary",
      label: t("DetailWindowTertiary"),
      snap: provider.tertiary,
    });
  for (const extra of provider.extraRateWindows ?? []) {
    metrics.push({
      id: `extra-${extra.id}`,
      label: extra.title,
      snap: extra.window,
    });
  }
  const visibleMetrics = compactMetrics ? metrics.slice(0, 2) : metrics;

  const hasCostHistory =
    chartData !== null && chartData.costHistory.some((point) => point.value > 0);
  const hasCreditsHistory =
    chartData !== null && chartData.creditsHistory.length > 0;
  const hasUsageBreakdown =
    chartData !== null && chartData.usageBreakdown.length > 0;
  const hasCharts = hasCostHistory || hasCreditsHistory || hasUsageBreakdown;
  const demoLocalUsage = localUsageForDemo(provider.providerId);
  const localUsage = provider.error ? null : chartData?.localUsage ?? demoLocalUsage;
  const localCostHistory = DEMO_ENABLED
    ? costBarsForDemo()
    : chartData?.costHistory ?? [];
  const hasMetrics = visibleMetrics.length > 0;
  const hasCost = !!provider.cost;
  const hasPace = !!provider.pace;
  const hasDetails =
    !provider.error && (hasMetrics || hasCost || hasPace || hasCharts || !!localUsage);
  const cardClassName = [
    "menu-card",
    provider.error ? "menu-card--error" : null,
    hasDetails ? "menu-card--with-details" : "menu-card--header-only",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <article className={cardClassName}>
      <header className="menu-card__header">
        <div className="menu-card__title-row">
          <div className="menu-card__name-group">
            <span className="menu-card__name">{provider.displayName}</span>
            {!provider.error && email && <span className="menu-card__email">{email}</span>}
          </div>
        </div>
        {provider.error ? (
          <div className="menu-card__error-block">
            <div className="menu-card__error-text">{provider.error}</div>
            <CopyIconButton text={provider.error} />
          </div>
        ) : (
          <div className="menu-card__subtitle-row">
            <span className="menu-card__subtitle">
              {t("DetailUpdatedPrefix")} {formatRelative(provider.updatedAt)}
            </span>
            {planName && (
              <span className="menu-card__plan-badge">{planName}</span>
            )}
          </div>
        )}
      </header>

      {hasDetails && <div className="menu-card__divider" />}

      {hasDetails && (
        <div className="menu-card__content">
          {!provider.error && hasMetrics && (
            <section className="menu-card__group menu-card__metrics">
              {visibleMetrics.map((m) => (
                <MetricRow
                  key={m.id}
                  title={m.label}
                  snap={m.snap}
                  exhaustedLabel={t("DetailWindowExhausted")}
                  resetTimeRelative={resetTimeRelative}
                  showAsUsed={showAsUsed}
                  expanded={expandedPaceWindow === m.id}
                  onToggleExpanded={() => {
                    setExpandedPaceWindow((current) =>
                      current === m.id ? null : m.id,
                    );
                    requestAnimationFrame(() => onLayoutChange?.());
                  }}
                />
              ))}
            </section>
          )}

          {localUsage && (
            <LocalUsageBlock
              providerId={provider.providerId}
              summary={localUsage}
              costHistory={localCostHistory}
            />
          )}

          {hasMetrics && hasCost && <div className="menu-card__divider" />}

          {provider.cost && (
            <section className="menu-card__group menu-card__cost">
              <div className="menu-card__group-title">
                {t("DetailCostTitle")} — {provider.cost.period}
              </div>
              <div className="menu-card__cost-line">
                {t("DetailCostUsed")}:{" "}
                {provider.cost.formattedUsed ||
                  formatCurrency(provider.cost.used, provider.cost.currencyCode)}
                {provider.cost.limit != null && (
                  <>
                    {" / "}
                    {provider.cost.formattedLimit ||
                      formatCurrency(provider.cost.limit, provider.cost.currencyCode)}
                  </>
                )}
              </div>
              {provider.cost.remaining != null && (
                <div className="menu-card__cost-line menu-card__cost-line--muted">
                  {t("DetailCostRemaining")}:{" "}
                  {formatCurrency(provider.cost.remaining, provider.cost.currencyCode)}
                </div>
              )}
              {formattedCostReset && (
                <div className="menu-card__cost-line menu-card__cost-line--muted">
                  {t("DetailCostResets")}: {formattedCostReset}
                </div>
              )}
            </section>
          )}

          {(hasMetrics || hasCost) && hasPace && <div className="menu-card__divider" />}

          {provider.pace && (
            <section className="menu-card__group menu-card__pace">
              <div className="menu-card__pace-header">
                <span className="menu-card__group-title">{t("DetailPaceTitle")}</span>
                <span
                  className="menu-card__pace-label"
                  data-pace={paceCategory(provider.pace.stage)}
                >
                  {t(paceStageKey(provider.pace.stage))} (
                  {provider.pace.deltaPercent >= 0 ? "+" : ""}
                  {provider.pace.deltaPercent.toFixed(1)}%)
                </span>
              </div>
              <div className="menu-card__pace-bars">
                <div className="menu-card__pace-track" title="Expected">
                  <div
                    className="menu-card__pace-fill menu-card__pace-fill--expected"
                    style={{ width: `${provider.pace.expectedUsedPercent.toFixed(1)}%` }}
                  />
                </div>
                <div className="menu-card__pace-track" title="Actual">
                  <div
                    className="menu-card__pace-fill"
                    data-pace={paceCategory(provider.pace.stage)}
                    style={{ width: `${provider.pace.actualUsedPercent.toFixed(1)}%` }}
                  />
                </div>
              </div>
              {provider.pace.etaSeconds != null && !provider.pace.willLastToReset && (
                <div className="menu-card__pace-eta">
                  ⚠{" "}
                  {t("DetailPaceRunsOutIn").replace(
                    "{}",
                    String(Math.round(provider.pace.etaSeconds / 3600)),
                  )}
                </div>
              )}
              {provider.pace.willLastToReset && (
                <div className="menu-card__pace-ok">
                  ✓ {t("DetailPaceWillLastToReset")}
                </div>
              )}
            </section>
          )}

          {(hasMetrics || hasCost || hasPace) && hasCharts && (
            <div className="menu-card__divider" />
          )}

          {hasCharts && (
            <section className="menu-card__group menu-card__charts">
              {hasCostHistory && (
                <SimpleBarChart
                  points={chartData!.costHistory}
                  label={t("DetailChartCost")}
                  color="var(--accent)"
                  formatValue={(v) => `$${v.toFixed(2)}`}
                />
              )}
              {hasCreditsHistory && (
                <SimpleBarChart
                  points={chartData!.creditsHistory}
                  label={t("DetailChartCredits")}
                  color="var(--provider-status-ok)"
                  formatValue={(v) => v.toFixed(1)}
                />
              )}
              {hasUsageBreakdown && (
                <StackedBarChart
                  points={chartData!.usageBreakdown}
                  label={t("DetailChartUsageBreakdown")}
                  height={56}
                />
              )}
            </section>
          )}
        </div>
      )}
    </article>
  );
}
