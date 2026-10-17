import { type ReactNode, useEffect, useMemo, useState } from "react";
import { StackedTrendChart } from "../../../components/charts/StackedTrendChart";
import { spendSourceColor } from "../../../components/charts/chartPalette";
import { formatUsd } from "../../../lib/usageSpendSharing";
import {
  type TrendDrill,
  type TrendInterval,
  type TrendModel,
  type TrendSection,
  type TrendSlot,
  bucketAt,
  buildDailyTrendModel,
  buildHourlyTrendModel,
  dailyTicks,
  dayOrdinal,
  drillTarget,
  emptyState,
  focusedHourlyDay,
  hourText,
  hourlyDays,
  hourlyTicks,
  hourlyTimeZoneText,
  localeFirstWeekday,
  resolveIsolatedSource,
  slotAt,
  slotInterval,
  sourceLabel,
  toggleIsolatedSource,
  trendWindow,
} from "../../../lib/spendTrendModel";
import type { SpendTrend, SpendTrendSource } from "../../../types/bridge";
import type { LocaleKey } from "../../../i18n/keys";

type Translate = (key: LocaleKey) => string;

interface Props {
  trend: SpendTrend;
  t: Translate;
  /** Hook for a provider brand icon beside each legend and inspector entry. */
  renderSourceIcon?: (source: SpendTrendSource) => ReactNode;
  /** Sunday = 0; defaults to the user's locale. */
  firstWeekday?: number;
}

const DAY_MS = 86_400_000;
const CURRENCY = "USD";

function dateText(ordinal: number): string {
  return new Date(ordinal * DAY_MS).toLocaleDateString(undefined, {
    timeZone: "UTC",
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

function axisDateText(slot: TrendSlot, model: TrendModel): string {
  const date = new Date(slot.start * DAY_MS);
  return model.unit === "month"
    ? date.toLocaleDateString(undefined, { timeZone: "UTC", year: "2-digit", month: "short" })
    : date.toLocaleDateString(undefined, { timeZone: "UTC", month: "short", day: "numeric" });
}

const compactFormatters = new Map<string, Intl.NumberFormat>();
function compactCost(value: number): string {
  let formatter = compactFormatters.get(CURRENCY);
  if (!formatter) {
    formatter = new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: CURRENCY,
      notation: "compact",
      maximumFractionDigits: 2,
    });
    compactFormatters.set(CURRENCY, formatter);
  }
  return formatter.format(value);
}

const costText = (value: number | null) => formatUsd(value, CURRENCY);

function groupingText(model: TrendModel, t: Translate): string {
  if (model.unit === "hour") return t("UsageSpendTrendHourly");
  if (model.unit === "month") return t("UsageSpendTrendGroupMonthly");
  if (model.unit === "week") return t("UsageSpendTrendGroupWeekly");
  return t("UsageSpendTrendDaily");
}

function scopeGroupingText(model: TrendModel, t: Translate): string {
  if (model.unit === "month") return t("UsageSpendTrendGroupMonthly");
  if (model.unit === "week") return t("UsageSpendTrendGroupWeekly");
  return t("UsageSpendTrendGroupDaily");
}

function rangeText(interval: TrendInterval): string {
  return `${dateText(interval.start)} – ${dateText(interval.end - 1)}`;
}

/** Upstream `bucketText`: weeks and months show their dates, hours carry their offset. */
function bucketText(model: TrendModel, key: number): string {
  if (model.section === "hourly") {
    const slot = slotAt(model, key);
    const day = model.day ? dayOrdinal(model.day) : null;
    const date = day == null ? "" : dateText(day);
    return slot ? `${date} · ${hourText(slot)}` : date;
  }
  if (model.unit === "week" || model.unit === "month") {
    const interval = slotInterval(model, key);
    if (interval) return rangeText(interval);
  }
  return dateText(key);
}

/**
 * Usage & Spend trend explorer (upstream 0.73.0 `SpendDashboardTrendPanel`):
 * long ranges draw weeks or months, a bucket drills to its days and a day to
 * its hours, the legend isolates one source, and an inspector reads out the
 * hovered, pinned or highest bucket.
 */
export function UsageSpendTrendPanel({ trend, t, renderSourceIcon, firstWeekday: weekStart }: Props) {
  const [section, setSection] = useState<TrendSection>("daily");
  const [focusedDay, setFocusedDay] = useState<string | null>(null);
  const [isolated, setIsolated] = useState<string | null>(null);
  const [focusedInterval, setFocusedInterval] = useState<TrendInterval | null>(null);
  const firstWeekday = useMemo(() => weekStart ?? localeFirstWeekday(), [weekStart]);

  // A new reporting window invalidates a drill-down taken inside the old one.
  useEffect(() => {
    setFocusedInterval(null);
  }, [trend.startDay, trend.endDay]);

  const sourceId = resolveIsolatedSource(isolated, trend.sources);
  const days = useMemo(() => hourlyDays(trend), [trend]);
  const activeSection: TrendSection = days.length === 0 ? "daily" : section;
  const day = focusedHourlyDay(days, null, focusedDay);
  const dayIndex = day ? days.indexOf(day) : -1;
  const reportWindow = trendWindow(trend);
  const focus =
    focusedInterval &&
    reportWindow &&
    focusedInterval.start >= reportWindow.start &&
    focusedInterval.end <= reportWindow.end
      ? focusedInterval
      : null;

  const model =
    activeSection === "hourly" && day
      ? buildHourlyTrendModel(trend, day, sourceId)
      : buildDailyTrendModel(trend, { focus, sourceId, firstWeekday });
  if (!model) return null;

  const openDay = (next: string) => {
    setFocusedDay(next);
    setSection("hourly");
  };
  const onDrill = (drill: TrendDrill) => {
    if (drill.kind === "interval") setFocusedInterval(drill.interval);
    else openDay(drill.day);
  };
  const colorFor = (id: string) => {
    const source = trend.sources.find((entry) => entry.id === id);
    return source ? spendSourceColor(source, trend.sources) : "var(--chart-cost)";
  };
  const nameFor = (id: string) => {
    const source = trend.sources.find((entry) => entry.id === id);
    return source ? sourceLabel(source, trend.sources) : id;
  };
  const identity = [activeSection, day ?? "", sourceId ?? "all", trend.startDay, trend.endDay, focus?.start ?? ""].join(":");

  return (
    <div className="settings-section__group usage-spend__card spend-trend" data-testid="spend-trend">
      <div className="spend-trend__header">
        <h4>{activeSection === "hourly" ? t("UsageSpendHourlyEstimatedSpend") : t("UsageSpendEstimatedSpend")}</h4>
        {days.length > 0 && (
          <div className="provider-detail-segmented" role="group" aria-label={t("UsageSpendTitle")}>
            {(["daily", "hourly"] as const).map((value) => (
              <button
                key={value}
                type="button"
                className={`provider-detail-segmented__option${activeSection === value ? " is-active" : ""}`}
                aria-pressed={activeSection === value}
                onClick={() => setSection(value)}
              >
                {value === "daily" ? t("UsageSpendTrendOverview") : t("UsageSpendTrendHourly")}
              </button>
            ))}
          </div>
        )}
      </div>

      {activeSection === "hourly" && day ? (
        <div className="spend-trend__day-nav">
          <button
            type="button"
            className="credential-btn credential-btn--secondary"
            aria-label={t("UsageSpendTrendPreviousDay")}
            title={t("UsageSpendTrendPreviousDay")}
            disabled={dayIndex <= 0}
            onClick={() => setFocusedDay(days[Math.max(0, dayIndex - 1)])}
          >
            ‹
          </button>
          <select
            aria-label={t("UsageSpendTrendDaily")}
            value={day}
            onChange={(event) => setFocusedDay(event.target.value)}
            data-testid="spend-trend-day-picker"
          >
            {days.map((entry) => (
              <option key={entry} value={entry}>
                {dateText(dayOrdinal(entry) ?? 0)}
              </option>
            ))}
          </select>
          <button
            type="button"
            className="credential-btn credential-btn--secondary"
            aria-label={t("UsageSpendTrendNextDay")}
            title={t("UsageSpendTrendNextDay")}
            disabled={dayIndex < 0 || dayIndex >= days.length - 1}
            onClick={() => setFocusedDay(days[Math.min(days.length - 1, dayIndex + 1)])}
          >
            ›
          </button>
        </div>
      ) : (
        <>
          <div className="spend-trend__scope">
            <p className="settings-section__caption" data-testid="spend-trend-scope">
              {`${rangeText(model.scope)} · ${scopeGroupingText(model, t)}`}
            </p>
            <span className="spend-trend__spacer" />
            {focus && (
              <button
                type="button"
                className="credential-btn credential-btn--secondary"
                onClick={() => setFocusedInterval(null)}
              >
                {t("UsageSpendTrendOverview")}
              </button>
            )}
          </div>
          <p className="settings-section__caption" style={{ margin: 0 }}>
            {t("UsageSpendTrendExploreHint")}
          </p>
        </>
      )}

      <TrendChartView
        key={identity}
        trend={trend}
        model={model}
        sourceId={sourceId}
        daysWithHours={days}
        colorFor={colorFor}
        nameFor={nameFor}
        onDrill={onDrill}
        renderSourceIcon={renderSourceIcon}
        t={t}
      />

      <ul className="spend-trend__legend" aria-label={t("UsageSpendSources")}>
        {trend.sources.map((source) => {
          const selected = sourceId === source.id;
          return (
            <li key={source.id}>
              <button
                type="button"
                className={`spend-trend__legend-item${selected ? " is-selected" : ""}${
                  sourceId != null && !selected ? " is-dimmed" : ""
                }`}
                aria-pressed={selected}
                title={t("UsageSpendTrendIsolateHint")}
                data-testid={`spend-trend-source-${source.id}`}
                onClick={() => setIsolated(toggleIsolatedSource(sourceId, source.id))}
              >
                <span className="spend-trend__dot" style={{ background: colorFor(source.id) }} aria-hidden="true" />
                {renderSourceIcon?.(source)}
                {sourceLabel(source, trend.sources)}
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

interface ChartViewProps {
  trend: SpendTrend;
  model: TrendModel;
  sourceId: string | null;
  daysWithHours: readonly string[];
  colorFor: (id: string) => string;
  nameFor: (id: string) => string;
  onDrill: (drill: TrendDrill) => void;
  renderSourceIcon?: (source: SpendTrendSource) => ReactNode;
  t: Translate;
}

/** Keyed by chart identity, so hover and pin reset with the view like upstream `.id(chartIdentity)`. */
function TrendChartView({
  trend,
  model,
  sourceId,
  daysWithHours,
  colorFor,
  nameFor,
  onDrill,
  renderSourceIcon,
  t,
}: ChartViewProps) {
  const [hoverKey, setHoverKey] = useState<number | null>(null);
  const [pinnedKey, setPinnedKey] = useState<number | null>(null);
  const zone = model.section === "hourly" ? hourlyTimeZoneText(model) : null;

  const zoneLine = zone && (
    <p
      className="spend-trend__zone"
      title={trend.timeZone}
      aria-label={`${t("UsageSpendTrendTimeZone")}: ${zone}`}
      data-testid="spend-trend-time-zone"
    >
      {zone}
    </p>
  );

  if (model.buckets.length === 0) {
    const state = emptyState(trend, model, sourceId);
    return (
      <>
        {zoneLine}
        <div className="spend-trend__empty" role="status" data-testid="spend-trend-empty" data-state={state}>
          <span aria-hidden="true">▁▃▂</span>
          {state === "noUsage" ? t("UsageSpendTrendNoUsage") : t("UsageSpendTrendUnavailable")}
        </div>
      </>
    );
  }

  const activeKey = hoverKey ?? pinnedKey;
  const inspected = activeKey == null ? model.peak : bucketAt(model, activeKey);
  const drillFor = (key: number) => drillTarget(model, key, daysWithHours);
  const activate = (key: number) => {
    setPinnedKey(key);
    setHoverKey(null);
    const drill = drillFor(key);
    if (drill) onDrill(drill);
  };
  const ticks =
    model.section === "hourly" ? hourlyTicks(model) : dailyTicks(model, (slot) => axisDateText(slot, model));
  const services = new Set(model.buckets.flatMap((bucket) => bucket.segments.map((segment) => segment.sourceId)));
  const description =
    model.section === "hourly"
      ? t("UsageSpendTrendHourlySummary")
          .replace("{}", model.buckets.length.toLocaleString())
          .replace("{}", services.size.toLocaleString())
      : `${groupingText(model, t)} · ${rangeText(model.scope)}`;
  const inspectedDrill = inspected ? drillFor(inspected.key) : null;
  const sourceById = new Map(trend.sources.map((source) => [source.id, source]));

  return (
    <>
      {zoneLine}
      <div className="spend-trend__summary" data-testid="spend-trend-summary">
        <span>
          <span className="spend-trend__summary-label">
            {model.section === "hourly" ? t("UsageSpendTrendRecordedHourly") : t("UsageSpendTrendRecorded")}
          </span>
          <span className="spend-trend__summary-total">{costText(model.total)}</span>
        </span>
        {model.peak && (
          <span>
            <span className="spend-trend__summary-label">{t("UsageSpendTrendHighest")}</span>
            {`${bucketText(model, model.peak.key)} · ${costText(model.peak.total)}`}
          </span>
        )}
      </div>

      <StackedTrendChart
        model={model}
        ariaLabel={model.section === "hourly" ? t("UsageSpendHourlyEstimatedSpend") : t("UsageSpendDailySpend")}
        ariaDescription={description}
        ticks={ticks}
        colorFor={colorFor}
        sourceName={nameFor}
        bucketLabel={(key) => bucketText(model, key)}
        formatCost={(value) => costText(value)}
        formatAxisCost={compactCost}
        activeKey={activeKey}
        pinnedKey={pinnedKey}
        onHover={setHoverKey}
        onPin={setPinnedKey}
        onActivate={activate}
      />

      <div className="spend-trend__inspector" data-testid="spend-trend-inspection">
        <div className="spend-trend__inspector-head">
          <select
            aria-label={groupingText(model, t)}
            value={inspected ? String(inspected.key) : ""}
            onChange={(event) => {
              setHoverKey(null);
              if (event.target.value) setPinnedKey(Number(event.target.value));
            }}
            data-testid="spend-trend-inspection-picker"
          >
            {!inspected && <option value="">{bucketText(model, activeKey ?? model.domain.start)}</option>}
            {model.buckets.map((bucket) => (
              <option key={bucket.key} value={String(bucket.key)}>
                {bucketText(model, bucket.key)}
              </option>
            ))}
          </select>
          {inspected && inspectedDrill && (
            <button
              type="button"
              className="credential-btn credential-btn--secondary"
              data-testid="spend-trend-inspection-details"
              onClick={() => onDrill(inspectedDrill)}
            >
              {inspectedDrill.kind === "day" ? t("UsageSpendTrendHourly") : t("UsageSpendTrendShowDetails")}
            </button>
          )}
          <span className="spend-trend__inspector-total">
            {inspected ? costText(inspected.total) : t("UsageSpendTrendNoData")}
          </span>
        </div>
        <ul className="spend-trend__inspector-rows">
          {(inspected?.segments ?? []).map((segment) => {
            const source = sourceById.get(segment.sourceId);
            return (
              <li key={segment.sourceId} className="spend-trend__source">
                <span className="spend-trend__dot" style={{ background: colorFor(segment.sourceId) }} aria-hidden="true" />
                {source && renderSourceIcon?.(source)}
                <span>{nameFor(segment.sourceId)}</span>
                <span>{costText(segment.cost)}</span>
              </li>
            );
          })}
        </ul>
      </div>
    </>
  );
}
