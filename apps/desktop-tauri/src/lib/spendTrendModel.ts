/**
 * Presentation model for the Usage & Spend trend explorer (upstream 0.73.0
 * `SpendTrendChartModel`). Pure functions only: the backend publishes recorded
 * per-source points, and this module buckets them into days, weeks or months,
 * resolves drill-down windows, the hourly view of one local day, and which
 * empty state applies.
 *
 * Daily math runs on day ordinals (days since 1970-01-01 of a `YYYY-MM-DD`
 * key), never on 24-hour multiples, so daylight-saving days need no cases.
 * Hourly math uses the real hour slots the backend publishes for each day.
 */
import type { SpendTrend, SpendTrendSource } from "../types/bridge";

export type TrendSection = "daily" | "hourly";
export type TrendUnit = "day" | "week" | "month" | "hour";
export type TrendEmptyState = "noUsage" | "unavailable";

/** Half-open range: day ordinals for daily models, epoch milliseconds for hourly ones. */
export interface TrendInterval {
  start: number;
  end: number;
}

export interface TrendSegment {
  sourceId: string;
  cost: number;
  /** Stack offsets within the bucket. */
  start: number;
  end: number;
}

export interface TrendSlot extends TrendInterval {
  key: number;
  /** Zone offset during an hourly slot; 0 for daily slots. */
  offsetMinutes: number;
}

export interface TrendBucket {
  key: number;
  segments: TrendSegment[];
  total: number;
}

export interface TrendModel {
  section: TrendSection;
  unit: TrendUnit;
  /** What the chart reports; a drill-down narrows it inside the window. */
  scope: TrendInterval;
  /** Scope widened to whole units, so partial weeks and months keep their width. */
  domain: TrendInterval;
  /** Every unit slot of the domain, including those without data. */
  slots: TrendSlot[];
  /** Slots with drawable spend, oldest first. */
  buckets: TrendBucket[];
  total: number | null;
  peak: TrendBucket | null;
  yMax: number;
  /** Local day key of an hourly model. */
  day: string | null;
}

const DAY_MS = 86_400_000;
/** Upstream `overviewUnit`: more than 45 days draws weeks, more than 180 draws months. */
export const WEEKLY_AFTER_DAYS = 45;
export const MONTHLY_AFTER_DAYS = 180;
/** A daily chart longer than this scrolls, showing the newest 31 days first. */
export const SCROLL_AFTER_DAYS = 32;
export const VISIBLE_DAYS = 31;

export function dayOrdinal(key: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(key);
  if (!match) return null;
  const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
  const ms = Date.UTC(year, month - 1, day);
  const parsed = new Date(ms);
  if (parsed.getUTCFullYear() !== year || parsed.getUTCMonth() !== month - 1 || parsed.getUTCDate() !== day) {
    return null;
  }
  return Math.round(ms / DAY_MS);
}

export function dayKey(ordinal: number): string {
  return new Date(ordinal * DAY_MS).toISOString().slice(0, 10);
}

export function overviewUnit(dayCount: number): TrendUnit {
  if (dayCount > MONTHLY_AFTER_DAYS) return "month";
  if (dayCount > WEEKLY_AFTER_DAYS) return "week";
  return "day";
}

/** Sunday = 0. 1970-01-01 was a Thursday. */
function weekday(ordinal: number): number {
  return (((ordinal + 4) % 7) + 7) % 7;
}

/** First day ordinal of the unit containing `ordinal`. `firstWeekday` uses Sunday = 0. */
export function unitStart(ordinal: number, unit: TrendUnit, firstWeekday = 1): number {
  if (unit === "week") return ordinal - ((weekday(ordinal) - firstWeekday + 7) % 7);
  if (unit === "month") {
    const date = new Date(ordinal * DAY_MS);
    return Math.round(Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), 1) / DAY_MS);
  }
  return ordinal;
}

export function unitEnd(ordinal: number, unit: TrendUnit, firstWeekday = 1): number {
  const start = unitStart(ordinal, unit, firstWeekday);
  if (unit === "week") return start + 7;
  if (unit === "month") {
    const date = new Date(start * DAY_MS);
    return Math.round(Date.UTC(date.getUTCFullYear(), date.getUTCMonth() + 1, 1) / DAY_MS);
  }
  return start + 1;
}

/** The reporting window as day ordinals; null when the keys do not parse. */
export function trendWindow(trend: SpendTrend): TrendInterval | null {
  const start = dayOrdinal(trend.startDay);
  const end = dayOrdinal(trend.endDay);
  if (start == null || end == null || end < start) return null;
  return { start, end: end + 1 };
}

function stack(grouped: Map<number, Map<string, number>>): TrendBucket[] {
  return [...grouped.keys()]
    .sort((a, b) => a - b)
    .map((key) => {
      const bySource = grouped.get(key) ?? new Map<string, number>();
      let cursor = 0;
      const segments = [...bySource.keys()].sort().map((sourceId) => {
        const cost = bySource.get(sourceId) ?? 0;
        const segment = { sourceId, cost, start: cursor, end: cursor + cost };
        cursor += cost;
        return segment;
      });
      return { key, segments, total: cursor };
    })
    .filter((bucket) => Number.isFinite(bucket.total));
}

function drawable(cost: number | null): cost is number {
  return cost != null && Number.isFinite(cost) && cost > 0;
}

function finishModel(
  base: Omit<TrendModel, "buckets" | "total" | "peak" | "yMax">,
  grouped: Map<number, Map<string, number>>,
): TrendModel {
  const buckets = stack(grouped);
  let peak: TrendBucket | null = null;
  for (const bucket of buckets) if (!peak || bucket.total > peak.total) peak = bucket;
  const total = buckets.length === 0 ? null : buckets.reduce((sum, bucket) => sum + bucket.total, 0);
  const padded = (peak?.total ?? 0) * 1.15;
  return {
    ...base,
    buckets,
    total: total != null && Number.isFinite(total) ? total : null,
    peak,
    yMax: Math.max(0.01, Number.isFinite(padded) ? padded : (peak?.total ?? 0)),
  };
}

export interface DailyModelOptions {
  /** Drill-down window inside the reporting window; null shows the overview. */
  focus?: TrendInterval | null;
  /** Legend isolation; null shows every source. */
  sourceId?: string | null;
  firstWeekday?: number;
}

export function buildDailyTrendModel(trend: SpendTrend, options: DailyModelOptions = {}): TrendModel | null {
  const window = trendWindow(trend);
  if (!window) return null;
  const firstWeekday = options.firstWeekday ?? 1;
  const scope = options.focus ?? window;
  const unit = overviewUnit(scope.end - scope.start);
  const domain = {
    start: unitStart(scope.start, unit, firstWeekday),
    end: unitEnd(scope.end - 1, unit, firstWeekday),
  };
  const slots: TrendSlot[] = [];
  for (let cursor = domain.start; cursor < domain.end; cursor = unitEnd(cursor, unit, firstWeekday)) {
    slots.push({ key: cursor, start: cursor, end: unitEnd(cursor, unit, firstWeekday), offsetMinutes: 0 });
  }
  const grouped = new Map<number, Map<string, number>>();
  for (const point of trend.daily) {
    if (options.sourceId != null && point.sourceId !== options.sourceId) continue;
    const ordinal = dayOrdinal(point.day);
    if (ordinal == null || ordinal < scope.start || ordinal >= scope.end || !drawable(point.costUsd)) continue;
    const key = unitStart(ordinal, unit, firstWeekday);
    const bySource = grouped.get(key) ?? new Map<string, number>();
    bySource.set(point.sourceId, (bySource.get(point.sourceId) ?? 0) + point.costUsd);
    grouped.set(key, bySource);
  }
  return finishModel({ section: "daily", unit, scope, domain, slots, day: null }, grouped);
}

function instant(value: string): number | null {
  const ms = Date.parse(value);
  return Number.isFinite(ms) ? ms : null;
}

/** The hourly view of one local day, over the day's real hour slots. */
export function buildHourlyTrendModel(
  trend: SpendTrend,
  day: string,
  sourceId: string | null = null,
): TrendModel | null {
  const frame = trend.hourlyDays.find((entry) => entry.day === day);
  const start = frame && instant(frame.start);
  const end = frame && instant(frame.end);
  if (!frame || start == null || end == null || end <= start) return null;
  const starts = frame.hours.map((slot) => instant(slot.start));
  const slots: TrendSlot[] = [];
  frame.hours.forEach((slot, index) => {
    const slotStart = starts[index];
    if (slotStart == null) return;
    const next = starts.slice(index + 1).find((value): value is number => value != null) ?? end;
    slots.push({ key: slotStart, start: slotStart, end: next, offsetMinutes: slot.offsetMinutes });
  });
  const grouped = new Map<number, Map<string, number>>();
  for (const point of trend.hourly) {
    if (sourceId != null && point.sourceId !== sourceId) continue;
    const hour = instant(point.hour);
    if (hour == null || hour < start || hour >= end || !drawable(point.costUsd)) continue;
    const bySource = grouped.get(hour) ?? new Map<string, number>();
    bySource.set(point.sourceId, (bySource.get(point.sourceId) ?? 0) + point.costUsd);
    grouped.set(hour, bySource);
  }
  const scope = { start, end };
  return finishModel({ section: "hourly", unit: "hour", scope, domain: scope, slots, day }, grouped);
}

/** Local days that have recorded hourly spend, oldest first. */
export function hourlyDays(trend: SpendTrend): string[] {
  return trend.hourlyDays
    .filter((frame) => {
      const start = instant(frame.start);
      const end = instant(frame.end);
      if (start == null || end == null) return false;
      return trend.hourly.some((point) => {
        const hour = instant(point.hour);
        return hour != null && hour >= start && hour < end && point.costUsd != null;
      });
    })
    .map((frame) => frame.day);
}

/** Upstream `focusedDay`: a dashboard-wide selected day wins, then the user's pick, then the newest day. */
export function focusedHourlyDay(days: readonly string[], selectedDay: string | null, focused: string | null): string | null {
  if (selectedDay && days.includes(selectedDay)) return selectedDay;
  if (focused && days.includes(focused)) return focused;
  return days.length > 0 ? days[days.length - 1] : null;
}

export function domainDayCount(model: TrendModel): number {
  return model.section === "hourly" ? 1 : model.domain.end - model.domain.start;
}

export function needsScrolling(model: TrendModel): boolean {
  return model.unit === "day" && domainDayCount(model) > SCROLL_AFTER_DAYS;
}

export function visibleDayCount(model: TrendModel): number {
  const days = domainDayCount(model);
  return model.unit === "day" ? Math.min(days, VISIBLE_DAYS) : days;
}

/** Every nth slot carries an axis label (upstream `tickStride`). */
export function tickStride(model: TrendModel): number {
  const days = visibleDayCount(model);
  if (model.unit === "month") return Math.max(1, Math.ceil(days / 30 / 4));
  if (model.unit === "week") return Math.max(1, Math.ceil(days / 7 / 4));
  return days > 14 ? 7 : days > 7 ? 3 : 1;
}

export function slotAt(model: TrendModel, key: number): TrendSlot | null {
  return model.slots.find((slot) => slot.key === key) ?? null;
}

/** No nearest-slot fallback: a slot without spend has no bucket. */
export function bucketAt(model: TrendModel, key: number | null): TrendBucket | null {
  if (key == null) return null;
  return model.buckets.find((bucket) => bucket.key === key) ?? null;
}

/** The slot clipped to the model's scope, so a partial week or month keeps its real dates. */
export function slotInterval(model: TrendModel, key: number): TrendInterval | null {
  const slot = slotAt(model, key);
  if (!slot) return null;
  const start = Math.max(slot.start, model.scope.start);
  const end = Math.min(slot.end, model.scope.end);
  return end > start ? { start, end } : null;
}

/** A week or month opens its days; a day opens its hours when it has any. */
export type TrendDrill = { kind: "interval"; interval: TrendInterval } | { kind: "day"; day: string };

export function drillTarget(model: TrendModel, key: number, daysWithHours: readonly string[]): TrendDrill | null {
  if (model.section !== "daily") return null;
  if (model.unit !== "day") {
    const interval = slotInterval(model, key);
    return interval ? { kind: "interval", interval } : null;
  }
  const day = dayKey(key);
  return daysWithHours.includes(day) ? { kind: "day", day } : null;
}

/**
 * Upstream `emptyStateTitle`: "No usage yet" only when every shown source is a
 * covered zero over the whole window, or recorded a priced zero for every day
 * in scope. Anything unscanned, unpriced or hourly reads "Spend unavailable".
 */
export function emptyState(trend: SpendTrend, model: TrendModel, sourceId: string | null): TrendEmptyState {
  if (model.section !== "daily") return "unavailable";
  const sources = trend.sources.filter((source) => sourceId == null || source.id === sourceId);
  const days = model.scope.end - model.scope.start;
  if (sources.length === 0 || days <= 0) return "unavailable";
  if (sources.every((source) => source.knownZero)) return "noUsage";
  const zeros = new Set<string>();
  for (const point of trend.daily) {
    if (point.costUsd === 0) zeros.add(`${point.sourceId}|${point.day}`);
  }
  for (let ordinal = model.scope.start; ordinal < model.scope.end; ordinal += 1) {
    const key = dayKey(ordinal);
    if (!sources.every((source) => zeros.has(`${source.id}|${key}`))) return "unavailable";
  }
  return "noUsage";
}

/** Clicking the isolated source again shows every source. */
export function toggleIsolatedSource(current: string | null, sourceId: string): string | null {
  return current === sourceId ? null : sourceId;
}

/** A source that left the dashboard (hidden, period change) drops its isolation. */
export function resolveIsolatedSource(current: string | null, sources: readonly SpendTrendSource[]): string | null {
  return current != null && sources.some((source) => source.id === current) ? current : null;
}

/** Same-name sources get a stable ordinal suffix (upstream `SpendChartPalette.label`). */
export function sourceLabel(source: SpendTrendSource, sources: readonly SpendTrendSource[]): string {
  const duplicates = sources.filter((entry) => entry.displayName === source.displayName).sort((a, b) => a.id.localeCompare(b.id));
  if (duplicates.length < 2) return source.displayName;
  return `${source.displayName} · ${(duplicates.findIndex((entry) => entry.id === source.id) + 1).toLocaleString()}`;
}

/** "UTC", or "UTC+05:30" / "UTC-07:00". */
export function utcOffsetText(offsetMinutes: number): string {
  if (offsetMinutes === 0) return "UTC";
  const minutes = Math.abs(offsetMinutes);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `UTC${offsetMinutes < 0 ? "-" : "+"}${pad(Math.floor(minutes / 60))}:${pad(minutes % 60)}`;
}

/** The hourly day's offset, or "start → end" across a daylight-saving change. */
export function hourlyTimeZoneText(model: TrendModel): string {
  const first = model.slots[0]?.offsetMinutes ?? 0;
  const last = model.slots[model.slots.length - 1]?.offsetMinutes ?? first;
  const start = utcOffsetText(first);
  const end = utcOffsetText(last);
  return start === end ? start : `${start} → ${end}`;
}

/** Local wall-clock "HH:MM"; language independent like upstream `clockText`. */
export function clockText(instantMs: number, offsetMinutes: number): string {
  const local = new Date(instantMs + offsetMinutes * 60_000);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(local.getUTCHours())}:${pad(local.getUTCMinutes())}`;
}

export function hourText(slot: TrendSlot): string {
  return `${clockText(slot.start, slot.offsetMinutes)} ${utcOffsetText(slot.offsetMinutes)}`;
}

export interface TrendTick {
  /** Position in the domain, 0..1. */
  position: number;
  label: string;
  anchor: "start" | "middle" | "end";
}

/** Hourly ticks every four local hours plus the day's end as "24:00". */
export function hourlyTicks(model: TrendModel): TrendTick[] {
  const span = model.domain.end - model.domain.start;
  if (span <= 0) return [];
  const ticks: TrendTick[] = model.slots
    .filter((slot) => {
      const local = new Date(slot.start + slot.offsetMinutes * 60_000);
      return local.getUTCMinutes() === 0 && local.getUTCHours() % 4 === 0;
    })
    .map((slot) => ({
      position: (slot.start - model.domain.start) / span,
      label: clockText(slot.start, slot.offsetMinutes),
      anchor: slot.start === model.domain.start ? "start" : "middle",
    }));
  ticks.push({ position: 1, label: "24:00", anchor: "end" });
  return ticks;
}

/** Every `tickStride`-th day, week or month slot, labelled at its center. */
export function dailyTicks(model: TrendModel, label: (slot: TrendSlot) => string): TrendTick[] {
  const stride = tickStride(model);
  return model.slots
    .filter((_, index) => index % stride === 0)
    .map((slot) => {
      const position = slotFraction(model, slot);
      return { position: position.start + position.width / 2, label: label(slot), anchor: "middle" };
    });
}

/** Position of a slot in the domain, 0..1. */
export function slotFraction(model: TrendModel, slot: TrendInterval): { start: number; width: number } {
  const span = model.domain.end - model.domain.start;
  if (span <= 0) return { start: 0, width: 0 };
  return { start: (slot.start - model.domain.start) / span, width: (slot.end - slot.start) / span };
}

/**
 * First weekday of the user's locale, Sunday = 0 (Intl `weekInfo` uses
 * Monday = 1 … Sunday = 7). Monday when the runtime does not expose it.
 */
export function localeFirstWeekday(locale?: string): number {
  try {
    const tag = locale ?? (typeof navigator !== "undefined" ? navigator.language : "en-US");
    const info = new Intl.Locale(tag) as Intl.Locale & {
      weekInfo?: { firstDay?: number };
      getWeekInfo?: () => { firstDay?: number };
    };
    const firstDay = info.getWeekInfo?.().firstDay ?? info.weekInfo?.firstDay;
    if (typeof firstDay === "number" && firstDay >= 1 && firstDay <= 7) return firstDay % 7;
  } catch {
    // Fall through to Monday.
  }
  return 1;
}
