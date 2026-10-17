import type { TokenActivitySource } from "../types/bridge";
import { normalizeHiddenSourceIds } from "./usageSpendDashboard";

/**
 * Layout and data rules for the Usage & Spend token activity grid, ported
 * from upstream `SpendActivityHeatmap.swift`, `SpendActivityScrollableGrid.swift`
 * and `SpendActivityMonthLabelsLayout.swift` (v0.73.0).
 *
 * Days are whole UTC day numbers built from the backend's `YYYY-MM-DD` keys,
 * so no local clock or DST shift can move a cell.
 */

export const WEEK_COUNT = 53;
export const DAY_COUNT = 7;
export const RANGE_DAY_COUNT = 365;
export const MINIMUM_CELL_SIZE = 10;
export const CELL_SPACING = 3;
export const TOOLTIP_INSET = 8;
export const TOOLTIP_GAP = 5;
export const TOOLTIP_WIDTH = 148;
export const TOOLTIP_HEIGHT = 50;

const MS_PER_DAY = 86_400_000;

export interface TokenActivityPoint {
  day: string;
  totalTokens: number | null;
  isScanned: boolean;
}

export function saturatingAdd(left: number, right: number): number {
  const sum = left + right;
  return sum > Number.MAX_SAFE_INTEGER ? Number.MAX_SAFE_INTEGER : sum;
}

export function dayNumber(key: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(key);
  if (!match) return null;
  const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
  const time = Date.UTC(year, month - 1, day);
  const parsed = new Date(time);
  if (parsed.getUTCFullYear() !== year || parsed.getUTCMonth() !== month - 1 || parsed.getUTCDate() !== day) {
    return null;
  }
  return Math.round(time / MS_PER_DAY);
}

export function dayKey(day: number): string {
  return new Date(day * MS_PER_DAY).toISOString().slice(0, 10);
}

/** Sunday = 0 ... Saturday = 6. */
export function weekdayOf(day: number): number {
  return new Date(day * MS_PER_DAY).getUTCDay();
}

/**
 * Upstream `SpendDashboardModel.tokenActivity`: per day, sum every visible
 * source that scanned it. A day no source scanned is a window edge; a day a
 * scanning source cannot total is a real gap. Hidden sources are left out.
 */
export function mergeTokenActivity(
  sources: readonly TokenActivitySource[] | undefined,
  hidden: readonly string[],
): TokenActivityPoint[] {
  const hiddenIds = new Set(normalizeHiddenSourceIds(hidden));
  const visible = (sources ?? []).filter((source) => !hiddenIds.has(source.providerId.trim().toLowerCase()));
  const days = new Set<string>();
  const lookups = visible.map((source) => {
    const byDay = new Map<string, { totalTokens: number | null; isScanned: boolean }>();
    for (const entry of source.days) {
      if (dayNumber(entry.day) == null) continue;
      days.add(entry.day);
      byDay.set(entry.day, entry);
    }
    return byDay;
  });
  return [...days].sort().map((day) => {
    let total = 0;
    let scannedContributors = 0;
    let unresolved = false;
    for (const lookup of lookups) {
      const entry = lookup.get(day);
      if (!entry?.isScanned) continue;
      scannedContributors += 1;
      if (entry.totalTokens == null || !Number.isFinite(entry.totalTokens)) {
        unresolved = true;
        continue;
      }
      total = saturatingAdd(total, Math.max(entry.totalTokens, 0));
    }
    if (scannedContributors === 0) return { day, totalTokens: null, isScanned: false };
    if (unresolved) return { day, totalTokens: null, isScanned: true };
    return { day, totalTokens: total, isScanned: true };
  });
}

export interface ActivitySeries {
  daily: number[];
  isCovered: boolean[];
  /** Whether the scan window reached the day; see `SpendActivitySeries.isScanned`. */
  isScanned: boolean[];
  /** Day number of cell 0, the Sunday on or before `rangeStart`. */
  start: number;
  rangeStart: number;
  today: number;
}

/** The last day the points cover, which the backend sets to its own today. */
export function latestDay(points: readonly TokenActivityPoint[]): number | null {
  let latest: number | null = null;
  for (const point of points) {
    const day = dayNumber(point.day);
    if (day != null && (latest == null || day > latest)) latest = day;
  }
  return latest;
}

/** Upstream `SpendActivitySeries.make`: 53 Sunday-first weeks ending today. */
export function buildActivitySeries(points: readonly TokenActivityPoint[], today: number): ActivitySeries {
  const totals = new Map<number, number>();
  const unknownDays = new Set<number>();
  const unscannedDays = new Set<number>();
  for (const point of points) {
    const day = dayNumber(point.day);
    if (day == null) continue;
    if (!point.isScanned) unscannedDays.add(day);
    if (point.totalTokens == null) {
      totals.delete(day);
      unknownDays.add(day);
      continue;
    }
    if (unknownDays.has(day)) continue;
    totals.set(day, saturatingAdd(totals.get(day) ?? 0, Math.max(point.totalTokens, 0)));
  }

  const rangeStart = today - (RANGE_DAY_COUNT - 1);
  const start = rangeStart - weekdayOf(rangeStart);
  const cellCount = WEEK_COUNT * DAY_COUNT;
  const daily = new Array<number>(cellCount).fill(0);
  const isCovered = new Array<boolean>(cellCount).fill(false);
  const isScanned = new Array<boolean>(cellCount).fill(false);
  for (let index = 0; index < cellCount; index += 1) {
    const date = start + index;
    if (date < rangeStart || date > today) continue;
    isScanned[index] = !unscannedDays.has(date);
    const total = totals.get(date);
    if (unknownDays.has(date) || total == null) continue;
    daily[index] = total;
    isCovered[index] = true;
  }
  return { daily, isCovered, isScanned, start, rangeStart, today };
}

export function seriesDay(series: ActivitySeries, index: number): number {
  return series.start + index;
}

export function isVisibleCell(series: ActivitySeries, index: number): boolean {
  if (index < 0 || index >= series.daily.length) return false;
  const day = seriesDay(series, index);
  return day >= series.rangeStart && day <= series.today;
}

export function visibleDayCount(series: ActivitySeries): number {
  return series.daily.filter((_, index) => isVisibleCell(series, index)).length;
}

export function coveredDayCount(series: ActivitySeries): number {
  return series.daily.filter((_, index) => isVisibleCell(series, index) && series.isCovered[index]).length;
}

export function hasUnknownCoverage(series: ActivitySeries): boolean {
  return coveredDayCount(series) < visibleDayCount(series);
}

export function seriesTotal(values: readonly number[]): number {
  return values.reduce((total, value) => saturatingAdd(total, value), 0);
}

export interface AggregateSeries {
  values: number[];
  isCovered: boolean[];
  isScanned: boolean[];
}

/**
 * Upstream `weeklyActivity`: days before the first scanned day are the window
 * edge and cannot make a week unavailable; every later day counts, so a stale
 * trailing day keeps its week unavailable.
 */
export function weeklyActivity(series: ActivitySeries): AggregateSeries {
  const firstScannedIndex = series.daily.findIndex(
    (_, index) => isVisibleCell(series, index) && series.isScanned[index],
  );
  const values: number[] = [];
  const isCovered: boolean[] = [];
  const isScanned: boolean[] = [];
  for (let start = 0; start < series.daily.length; start += DAY_COUNT) {
    const indices: number[] = [];
    for (let index = start; index < Math.min(start + DAY_COUNT, series.daily.length); index += 1) {
      if (isVisibleCell(series, index)) indices.push(index);
    }
    const counted = firstScannedIndex < 0 ? [] : indices.filter((index) => index >= firstScannedIndex);
    values.push(indices.reduce((total, index) => saturatingAdd(total, series.daily[index]), 0));
    isCovered.push(counted.length > 0 && counted.every((index) => series.isCovered[index]));
    isScanned.push(counted.length > 0);
  }
  return { values, isCovered, isScanned };
}

/**
 * Upstream `cumulative()`: running totals whose coverage starts at the first
 * scanned week; once a scanned week is unknown every later total is too.
 */
export function cumulativeActivity(weekly: AggregateSeries): AggregateSeries {
  let total = 0;
  let hasStarted = false;
  let coverageIsComplete = true;
  const values: number[] = [];
  const isCovered: boolean[] = [];
  weekly.values.forEach((value, index) => {
    total = saturatingAdd(total, value);
    if (weekly.isScanned[index]) hasStarted = true;
    if (hasStarted) coverageIsComplete = coverageIsComplete && weekly.isCovered[index];
    values.push(total);
    isCovered.push(hasStarted && coverageIsComplete);
  });
  return { values, isCovered, isScanned: [...weekly.isScanned] };
}

/** Upstream `SpendActivityLevels.dailyLevels`: quartiles of the busiest day. */
export function dailyLevels(values: readonly number[]): number[] {
  const maxValue = values.reduce((max, value) => Math.max(max, value), 0);
  return values.map((value) => {
    if (value <= 0 || maxValue <= 0) return 0;
    const ratio = value / maxValue;
    if (ratio > 0.75) return 4;
    if (ratio > 0.5) return 3;
    if (ratio > 0.25) return 2;
    return 1;
  });
}

/** Filled rows (from the bottom) of a weekly column; any activity fills at least one. */
export function weekFill(value: number, maxValue: number, rows = DAY_COUNT): number {
  const raw = maxValue > 0 ? Math.round((value / maxValue) * rows) : 0;
  return value > 0 ? Math.max(raw, 1) : 0;
}

export function weekStartDay(series: ActivitySeries, column: number): number {
  return seriesDay(series, column * DAY_COUNT);
}

export function isVisibleWeek(series: ActivitySeries, column: number): boolean {
  if (column < 0 || column >= WEEK_COUNT) return false;
  const start = weekStartDay(series, column);
  const end = start + DAY_COUNT - 1;
  return start <= series.today && end >= series.rangeStart;
}

export interface GridFrame {
  width: number;
  height: number;
  pitch: number;
  cell: number;
}

/** Fill the container, but never shrink a day below the minimum cell plus gap. */
export function gridFrame(containerWidth: number, columns = WEEK_COUNT): GridFrame {
  const minimumWidth = Math.max(columns, 0) * (MINIMUM_CELL_SIZE + CELL_SPACING);
  const width = Math.max(containerWidth, minimumWidth);
  const pitch = columns > 0 ? width / columns : 0;
  return { width, height: pitch * DAY_COUNT, pitch, cell: Math.max(pitch - CELL_SPACING, 0) };
}

export interface VisibleSpan {
  minX: number;
  maxX: number;
}

export function tooltipCenterX(anchorX: number, tooltipWidth: number, gridWidth: number, visible?: VisibleSpan): number {
  const lowerEdge = Math.max(visible?.minX ?? 0, 0);
  const upperEdge = Math.min(visible?.maxX ?? gridWidth, gridWidth);
  const center = (lowerEdge + upperEdge) / 2;
  const halfWidth = tooltipWidth / 2;
  const lower = Math.min(lowerEdge + halfWidth + TOOLTIP_INSET, center);
  const upper = Math.max(upperEdge - halfWidth - TOOLTIP_INSET, center);
  return Math.min(Math.max(anchorX, lower), upper);
}

export function effectiveTooltipHeight(gridHeight: number): number {
  return Math.min(TOOLTIP_HEIGHT, gridHeight);
}

export function tooltipOriginY(anchorY: number, tooltipHeight: number, gridHeight: number): number {
  const maxOrigin = Math.max(gridHeight - tooltipHeight, 0);
  const above = anchorY - tooltipHeight - TOOLTIP_GAP;
  const candidate = above >= TOOLTIP_INSET ? above : anchorY + TOOLTIP_GAP;
  return Math.min(Math.max(candidate, 0), maxOrigin);
}

export type GridMove = "left" | "right" | "up" | "down";

/** Upstream `SpendActivityGridNavigation.candidate`; rows stay inside their week. */
export function navigationCandidate(current: number, move: GridMove, rows = DAY_COUNT): number | null {
  if (rows <= 0) return null;
  const row = current % rows;
  switch (move) {
    case "left":
      return current - rows;
    case "right":
      return current + rows;
    case "up":
      return row > 0 ? current - 1 : null;
    case "down":
      return row < rows - 1 ? current + 1 : null;
  }
}

export type ScrollDirection = "earlier" | "later";

export interface ScrollNavigation {
  isScrollable: boolean;
  canScrollEarlier: boolean;
  canScrollLater: boolean;
  /** Week column to align to the leading edge after a page move. */
  targetColumn: (direction: ScrollDirection) => number;
}

/** Upstream `SpendActivityScrollNavigation`. */
export function scrollNavigation(contentWidth: number, viewportWidth: number, offset: number): ScrollNavigation {
  const maximumOffset = Math.max(contentWidth - viewportWidth, 0);
  const boundedOffset = Math.min(Math.max(offset, 0), maximumOffset);
  const isScrollable = viewportWidth > 0 && maximumOffset > 0.5;
  return {
    isScrollable,
    canScrollEarlier: isScrollable && boundedOffset > 0.5,
    canScrollLater: isScrollable && boundedOffset < maximumOffset - 0.5,
    targetColumn(direction) {
      const pitch = contentWidth / WEEK_COUNT;
      if (!(pitch > 0)) return 0;
      // Keep one week visible across the page move.
      const step = Math.max(viewportWidth - 2 * pitch, pitch);
      const destination =
        direction === "earlier" ? Math.max(boundedOffset - step, 0) : Math.min(boundedOffset + step, maximumOffset);
      const column = direction === "earlier" ? Math.floor(destination / pitch) : Math.ceil(destination / pitch);
      return Math.min(Math.max(column, 0), WEEK_COUNT - 1);
    },
  };
}

/** Scroll offset that brings `column` fully into view, or null when it already is. */
export function revealOffset(column: number, pitch: number, viewportWidth: number, offset: number): number | null {
  const left = column * pitch;
  const right = left + pitch;
  if (left < offset) return left;
  if (right > offset + viewportWidth) return Math.max(right - viewportWidth, 0);
  return null;
}

export interface MonthMarker {
  column: number;
  label: string;
}

/** A label for every week that starts in the first seven days of a month. */
export function monthMarkers(series: ActivitySeries, formatMonth: (day: number) => string): MonthMarker[] {
  const markers: MonthMarker[] = [];
  let lastLabel = "";
  for (let column = 0; column < WEEK_COUNT; column += 1) {
    const day = weekStartDay(series, column);
    if (new Date(day * MS_PER_DAY).getUTCDate() > 7) continue;
    const label = formatMonth(day);
    if (label === lastLabel) continue;
    markers.push({ column, label });
    lastLabel = label;
  }
  return markers;
}

/**
 * Upstream `SpendActivityMonthLabelsLayout`: placed right to left, each label
 * stays inside the grid and clear of the label after it.
 */
export function placeMonthLabels(offsets: readonly number[], widths: readonly number[], gridWidth: number): number[] {
  const positions = new Array<number>(offsets.length).fill(0);
  let trailing = gridWidth;
  for (let index = offsets.length - 1; index >= 0; index -= 1) {
    const x = Math.max(0, Math.min(offsets[index], trailing - (widths[index] ?? 0)));
    positions[index] = x;
    trailing = x - 4;
  }
  return positions;
}

/**
 * Upstream `SpendActivityDaySelection.day`: a covered day toggles the
 * selection; `undefined` means the cell cannot be selected.
 */
export function selectionAfterActivating(
  series: ActivitySeries,
  index: number,
  selectedDay: string | null,
): string | null | undefined {
  if (!isVisibleCell(series, index) || !series.isCovered[index]) return undefined;
  const day = dayKey(seriesDay(series, index));
  return day === selectedDay ? null : day;
}
