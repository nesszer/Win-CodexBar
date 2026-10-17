import {
  useCallback,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
} from "react";
import { formatCompactTokens } from "../../../lib/usageSpendDashboard";
import { formatSpendDay } from "../../../lib/usageSpendBreakdown";
import {
  DAY_COUNT,
  TOOLTIP_WIDTH,
  WEEK_COUNT,
  buildActivitySeries,
  coveredDayCount,
  cumulativeActivity,
  dailyLevels,
  dayKey,
  effectiveTooltipHeight,
  gridFrame,
  hasUnknownCoverage,
  isVisibleCell,
  isVisibleWeek,
  latestDay,
  monthMarkers,
  navigationCandidate,
  placeMonthLabels,
  revealOffset,
  scrollNavigation,
  selectionAfterActivating,
  seriesDay,
  seriesTotal,
  tooltipCenterX,
  tooltipOriginY,
  visibleDayCount,
  weekFill,
  weekStartDay,
  weeklyActivity,
  type ActivitySeries,
  type AggregateSeries,
  type GridFrame,
  type GridMove,
  type ScrollDirection,
  type TokenActivityPoint,
  type VisibleSpan,
} from "../../../lib/tokenActivity";
import type { LocaleKey } from "../../../i18n/keys";

type Translate = (key: LocaleKey) => string;

export type TokenActivityMode = "daily" | "weekly" | "cumulative";

const MODES: { id: TokenActivityMode; label: LocaleKey }[] = [
  { id: "daily", label: "TokenActivityDaily" },
  { id: "weekly", label: "TokenActivityWeekly" },
  { id: "cumulative", label: "TokenActivityCumulative" },
];

export const TOKEN_ACTIVITY_MODE_STORAGE_KEY = "codexbar.spendActivityViewMode";
const MONTH_ROW_HEIGHT = 18;
const DAILY_HEADER_HEIGHT = MONTH_ROW_HEIGHT + 3;

function readStoredMode(): TokenActivityMode {
  try {
    const stored = window.localStorage.getItem(TOKEN_ACTIVITY_MODE_STORAGE_KEY);
    if (stored === "daily" || stored === "weekly" || stored === "cumulative") return stored;
  } catch {
    // Storage can be unavailable; the grid then starts on Daily.
  }
  return "daily";
}

function storeMode(mode: TokenActivityMode) {
  try {
    window.localStorage.setItem(TOKEN_ACTIVITY_MODE_STORAGE_KEY, mode);
  } catch {
    // Not persisting the mode is harmless.
  }
}

const monthFormatter = new Intl.DateTimeFormat(undefined, { month: "short", timeZone: "UTC" });
const formatMonth = (day: number) => monthFormatter.format(new Date(dayKey(day) + "T00:00:00Z"));
const formatDay = (day: number) => formatSpendDay(dayKey(day));

function coverageText(series: ActivitySeries, t: Translate): string {
  return `${t("TokenActivityCoverage")}: ${coveredDayCount(series)} / ${visibleDayCount(series)}`;
}

interface Props {
  points: TokenActivityPoint[];
  selectedDay: string | null;
  onSelectDay: (day: string | null) => void;
  t: Translate;
}

/**
 * Upstream v0.73.0 `SpendActivityHeatmapView`: the last 53 Sunday-first weeks
 * of tokens as a Daily, Weekly or Cumulative grid. Today is the backend's last
 * day, so the grid follows the cost bucket zone rather than the local clock.
 */
export function TokenActivityGrid({ points, selectedDay, onSelectDay, t }: Props) {
  const [mode, setModeState] = useState<TokenActivityMode>(readStoredMode);
  const setMode = useCallback((next: TokenActivityMode) => {
    setModeState(next);
    storeMode(next);
  }, []);
  const today = latestDay(points);
  const series = useMemo(() => (today == null ? null : buildActivitySeries(points, today)), [points, today]);
  if (!series) return null;

  const hasActivity = series.daily.some((value) => value > 0);
  const unknownCoverage = hasUnknownCoverage(series);
  const total = formatCompactTokens(seriesTotal(series.daily));
  const weekly = weeklyActivity(series);
  let summary: string | null = null;
  if (hasActivity) {
    summary = unknownCoverage ? `${total} · ${coverageText(series, t)}` : `${total} ${t("TokenActivityInLastYear")}`;
  } else if (unknownCoverage) {
    summary = `${t("TokenActivityUnavailable")} · ${coverageText(series, t)}`;
  }

  return (
    <section className="usage-spend__card token-activity" data-testid="token-activity">
      <div className="token-activity__header">
        <div>
          <h4 className="token-activity__title">{t("TokenActivityTitle")}</h4>
          {summary && <p className="settings-section__caption token-activity__summary">{summary}</p>}
        </div>
        <div
          className="provider-detail-segmented token-activity__modes"
          role="radiogroup"
          aria-label={t("TokenActivityView")}
        >
          {MODES.map((option) => (
            <button
              key={option.id}
              type="button"
              role="radio"
              aria-checked={option.id === mode}
              className={`provider-detail-segmented__option${option.id === mode ? " is-active" : ""}`}
              onClick={() => setMode(option.id)}
            >
              {t(option.label)}
            </button>
          ))}
        </div>
      </div>

      {hasActivity || unknownCoverage ? (
        <>
          {mode === "daily" && (
            <>
              <DailyGrid series={series} selectedDay={selectedDay} onSelectDay={onSelectDay} t={t} />
              <div className="token-activity__legend">
                <span>{t("TokenActivityLess")}</span>
                {[0, 1, 2, 3, 4].map((level) => (
                  <Swatch key={level} level={level} />
                ))}
                <span>{t("TokenActivityMore")}</span>
                {unknownCoverage && (
                  <>
                    <Swatch level={null} className="token-activity__swatch--gap" />
                    <span>{t("TokenActivityUnavailable")}</span>
                  </>
                )}
              </div>
            </>
          )}
          {mode === "weekly" && (
            <>
              <WeekGrid series={series} activity={weekly} cumulative={false} t={t} />
              <Caption text={t("TokenActivityEachColumnWeek")} showsUnavailable={weekly.isCovered.includes(false)} t={t} />
            </>
          )}
          {mode === "cumulative" && (
            <>
              <WeekGrid series={series} activity={cumulativeActivity(weekly)} cumulative t={t} />
              <Caption text={t("TokenActivityRunningTotal")} showsUnavailable={unknownCoverage} t={t} />
            </>
          )}
        </>
      ) : (
        <p className="token-activity__empty">{t("TokenActivityNoActivity")}</p>
      )}
    </section>
  );
}

function Caption({ text, showsUnavailable, t }: { text: string; showsUnavailable: boolean; t: Translate }) {
  return (
    <div className="token-activity__caption">
      <span>{text}</span>
      {showsUnavailable && (
        <span className="token-activity__caption-gap">
          <Swatch level={null} />
          <span>{t("TokenActivityUnavailable")}</span>
        </span>
      )}
    </div>
  );
}

/** Upstream `SpendActivityPalette.drawCell`: a slash marks missing history without relying on hue. */
function Cell({ x, y, size, level }: { x: number; y: number; size: number; level: number | null }) {
  const corner = Math.min(size * 0.22, 2.5);
  if (level != null) {
    return (
      <rect
        className={`token-activity__cell token-activity__cell--l${level}`}
        x={x}
        y={y}
        width={size}
        height={size}
        rx={corner}
      />
    );
  }
  const inset = Math.min(size * 0.25, 2);
  return (
    <g className="token-activity__cell token-activity__cell--unavailable">
      <rect x={x} y={y} width={size} height={size} rx={corner} className="token-activity__gap-fill" />
      <rect
        x={x + 0.5}
        y={y + 0.5}
        width={Math.max(size - 1, 0)}
        height={Math.max(size - 1, 0)}
        rx={corner}
        className="token-activity__gap-stroke"
      />
      <line
        x1={x + inset}
        y1={y + size - inset}
        x2={x + size - inset}
        y2={y + inset}
        className="token-activity__gap-stroke"
      />
    </g>
  );
}

function Swatch({ level, className }: { level: number | null; className?: string }) {
  return (
    <svg
      className={`token-activity__swatch${className ? ` ${className}` : ""}`}
      width={10}
      height={10}
      viewBox="0 0 10 10"
      aria-hidden="true"
      data-level={level ?? "unavailable"}
    >
      <Cell x={0} y={0} size={10} level={level} />
    </svg>
  );
}

function Tooltip({
  title,
  subtitle,
  centerX,
  originY,
  width,
  height,
}: {
  title: string;
  subtitle: string;
  centerX: number;
  originY: number;
  width: number;
  height: number;
}) {
  return (
    <div
      className="token-activity__tooltip"
      role="tooltip"
      style={{ left: centerX - width / 2, top: originY, width, height }}
    >
      <strong>{title}</strong>
      <span>{subtitle}</span>
    </div>
  );
}

/**
 * Upstream `SpendActivityScrollableGrid`: keeps the year continuous at a
 * readable cell size, starts on the most recent weeks and pages by buttons.
 */
function ScrollableGrid({
  headerHeight = 0,
  revealColumn,
  onScrolled,
  t,
  children,
}: {
  headerHeight?: number;
  revealColumn?: number | null;
  onScrolled?: () => void;
  t: Translate;
  children: (frame: GridFrame, visible: VisibleSpan) => ReactNode;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const anchored = useRef(false);
  const [viewportWidth, setViewportWidth] = useState(0);
  const [offset, setOffset] = useState(0);
  const frame = gridFrame(viewportWidth);
  const navigation = scrollNavigation(frame.width, viewportWidth, offset);

  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const measure = () => setViewportWidth(element.clientWidth);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  // Upstream anchors the scroll view to its trailing edge: open on today.
  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element || anchored.current || viewportWidth <= 0) return;
    anchored.current = true;
    const end = Math.max(frame.width - viewportWidth, 0);
    element.scrollLeft = end;
    setOffset(element.scrollLeft || end);
  }, [frame.width, viewportWidth]);

  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element || revealColumn == null || viewportWidth <= 0) return;
    const next = revealOffset(revealColumn, frame.pitch, viewportWidth, element.scrollLeft);
    if (next == null) return;
    element.scrollLeft = next;
    setOffset(next);
  }, [revealColumn, frame.pitch, viewportWidth]);

  const page = (direction: ScrollDirection) => {
    const element = scroller.current;
    if (!element) return;
    const left = navigation.targetColumn(direction) * frame.pitch;
    element.scrollLeft = left;
    setOffset(left);
    onScrolled?.();
  };

  const visibleWidth = viewportWidth > 0 ? viewportWidth : frame.width;
  return (
    <div className="token-activity__scroll">
      <div
        ref={scroller}
        className="token-activity__scroller"
        dir="ltr"
        data-testid="token-activity-scroller"
        onScroll={(event) => {
          setOffset(event.currentTarget.scrollLeft);
          onScrolled?.();
        }}
      >
        <div className="token-activity__content" style={{ width: frame.width, height: headerHeight + frame.height }}>
          {children(frame, { minX: offset, maxX: offset + visibleWidth })}
        </div>
      </div>
      {navigation.isScrollable && (
        <div className="token-activity__nav">
          <button
            type="button"
            className="credential-btn credential-btn--secondary"
            data-testid="token-activity-earlier"
            disabled={!navigation.canScrollEarlier}
            onClick={() => page("earlier")}
          >
            <span aria-hidden="true">‹</span> {t("TokenActivityEarlier")}
          </button>
          <button
            type="button"
            className="credential-btn credential-btn--secondary"
            data-testid="token-activity-later"
            disabled={!navigation.canScrollLater}
            onClick={() => page("later")}
          >
            {t("TokenActivityLater")} <span aria-hidden="true">›</span>
          </button>
        </div>
      )}
    </div>
  );
}

function tooltipSize(frame: GridFrame, visible: VisibleSpan) {
  return {
    width: Math.min(TOOLTIP_WIDTH, Math.max(visible.maxX - visible.minX - 8, 1)),
    height: effectiveTooltipHeight(frame.height),
  };
}

function pointerPosition(event: MouseEvent<Element>) {
  const bounds = event.currentTarget.getBoundingClientRect();
  return { x: event.clientX - bounds.left, y: event.clientY - bounds.top };
}

function cellAt(x: number, y: number, pitch: number): { column: number; row: number } | null {
  if (!(pitch > 0) || x < 0 || y < 0) return null;
  const column = Math.floor(x / pitch);
  const row = Math.floor(y / pitch);
  if (column >= WEEK_COUNT || row >= DAY_COUNT) return null;
  return { column, row };
}

function isFocusVisible(element: Element): boolean {
  try {
    return element.matches(":focus-visible");
  } catch {
    return false;
  }
}

const KEY_MOVES: Record<string, GridMove | undefined> = {
  ArrowLeft: "left",
  ArrowRight: "right",
  ArrowUp: "up",
  ArrowDown: "down",
};

function DailyGrid({
  series,
  selectedDay,
  onSelectDay,
  t,
}: {
  series: ActivitySeries;
  selectedDay: string | null;
  onSelectDay: (day: string | null) => void;
  t: Translate;
}) {
  const levels = useMemo(() => dailyLevels(series.daily), [series]);
  const markers = useMemo(() => monthMarkers(series, formatMonth), [series]);
  const [hovered, setHovered] = useState<number | null>(null);
  const [keyboardIndex, setKeyboardIndex] = useState<number | null>(null);
  const valueId = useId();
  const activeIndex = hovered ?? keyboardIndex;
  const lastVisibleIndex = useMemo(() => {
    for (let index = series.daily.length - 1; index >= 0; index -= 1) {
      if (isVisibleCell(series, index)) return index;
    }
    return null;
  }, [series]);

  const describe = (index: number) =>
    `${formatDay(seriesDay(series, index))}: ${
      series.isCovered[index] ? formatCompactTokens(series.daily[index]) : t("TokenActivityUnavailable")
    }`;
  const totalText = formatCompactTokens(seriesTotal(series.daily));
  const value =
    activeIndex != null
      ? describe(activeIndex)
      : hasUnknownCoverage(series)
        ? `${totalText} · ${coverageText(series, t)}`
        : totalText;

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    // Arrow keys on the Earlier / More recent buttons keep their own meaning.
    if (event.target !== event.currentTarget) return;
    const move = KEY_MOVES[event.key];
    if (move) {
      event.preventDefault();
      setHovered(null);
      const current = keyboardIndex ?? lastVisibleIndex;
      if (current == null) return;
      const candidate = navigationCandidate(current, move);
      setKeyboardIndex(candidate != null && isVisibleCell(series, candidate) ? candidate : current);
      return;
    }
    if ((event.key === "Enter" || event.key === " ") && keyboardIndex != null) {
      event.preventDefault();
      const next = selectionAfterActivating(series, keyboardIndex, selectedDay);
      if (next !== undefined) onSelectDay(next);
    }
  };

  return (
    <div
      className="token-activity__focus"
      role="group"
      tabIndex={0}
      aria-label={t("TokenActivityTitle")}
      aria-describedby={valueId}
      data-testid="token-activity-daily"
      onKeyDown={onKeyDown}
      onFocus={(event) => {
        // Upstream focuses the newest day; a pointer click must not leave a stray tooltip.
        if (event.target === event.currentTarget && isFocusVisible(event.currentTarget)) {
          setKeyboardIndex((current) => current ?? lastVisibleIndex);
        }
      }}
    >
      <span id={valueId} className="token-activity__sr" aria-live="polite">
        {value}
      </span>
      <ScrollableGrid
        headerHeight={DAILY_HEADER_HEIGHT}
        revealColumn={keyboardIndex == null ? null : Math.floor(keyboardIndex / DAY_COUNT)}
        onScrolled={() => setHovered(null)}
        t={t}
      >
        {(frame, visible) => {
          const cell = frame.cell;
          const inset = (frame.pitch - cell) / 2;
          const offsets = markers.map((marker) => marker.column * frame.pitch);
          let tooltip: ReactNode = null;
          let highlight: ReactNode = null;
          if (activeIndex != null && isVisibleCell(series, activeIndex)) {
            const column = Math.floor(activeIndex / DAY_COUNT);
            const row = activeIndex % DAY_COUNT;
            const anchorX = column * frame.pitch + frame.pitch / 2;
            const anchorY = row * frame.pitch + frame.pitch / 2;
            highlight = (
              <rect
                className="token-activity__highlight"
                x={column * frame.pitch + inset}
                y={row * frame.pitch + inset}
                width={cell}
                height={cell}
                rx={Math.min(cell * 0.22, 2.5)}
              />
            );
            if (visible.minX <= anchorX && visible.maxX >= anchorX) {
              const size = tooltipSize(frame, visible);
              tooltip = (
                <Tooltip
                  title={
                    series.isCovered[activeIndex]
                      ? formatCompactTokens(series.daily[activeIndex])
                      : t("TokenActivityUnavailable")
                  }
                  subtitle={formatDay(seriesDay(series, activeIndex))}
                  centerX={tooltipCenterX(anchorX, size.width, frame.width, visible)}
                  originY={tooltipOriginY(anchorY, size.height, frame.height)}
                  width={size.width}
                  height={size.height}
                />
              );
            }
          }
          return (
            <>
              <MonthLabels markers={markers} offsets={offsets} gridWidth={frame.width} />
              <div className="token-activity__grid" style={{ width: frame.width, height: frame.height }}>
                <svg
                  width={frame.width}
                  height={frame.height}
                  data-testid="token-activity-cells"
                  onMouseMove={(event) => {
                    const { x, y } = pointerPosition(event);
                    const hit = cellAt(x, y, frame.pitch);
                    const index = hit ? hit.column * DAY_COUNT + hit.row : null;
                    setHovered(index != null && isVisibleCell(series, index) ? index : null);
                  }}
                  onMouseLeave={() => setHovered(null)}
                  onClick={(event) => {
                    const { x, y } = pointerPosition(event);
                    const hit = cellAt(x, y, frame.pitch);
                    if (!hit) return;
                    const next = selectionAfterActivating(series, hit.column * DAY_COUNT + hit.row, selectedDay);
                    if (next !== undefined) onSelectDay(next);
                  }}
                >
                  {series.daily.map((_, index) => {
                    if (!isVisibleCell(series, index)) return null;
                    const column = Math.floor(index / DAY_COUNT);
                    const row = index % DAY_COUNT;
                    const day = dayKey(seriesDay(series, index));
                    return (
                      <g
                        key={index}
                        data-day={day}
                        data-selected={day === selectedDay ? "true" : undefined}
                        className={day === selectedDay ? "token-activity__selected" : undefined}
                      >
                        <title>{describe(index)}</title>
                        <Cell
                          x={column * frame.pitch + inset}
                          y={row * frame.pitch + inset}
                          size={cell}
                          level={series.isCovered[index] ? levels[index] : null}
                        />
                      </g>
                    );
                  })}
                  {highlight}
                </svg>
                {tooltip}
              </div>
            </>
          );
        }}
      </ScrollableGrid>
    </div>
  );
}

function MonthLabels({
  markers,
  offsets,
  gridWidth,
}: {
  markers: { column: number; label: string }[];
  offsets: number[];
  gridWidth: number;
}) {
  const refs = useRef<(HTMLSpanElement | null)[]>([]);
  const [widths, setWidths] = useState<number[]>([]);
  useLayoutEffect(() => {
    const measured = markers.map((_, index) => refs.current[index]?.offsetWidth ?? 0);
    setWidths((current) =>
      current.length === measured.length && current.every((width, index) => width === measured[index])
        ? current
        : measured,
    );
  }, [markers]);
  const positions = placeMonthLabels(offsets, widths, gridWidth);
  return (
    <div className="token-activity__months" style={{ width: gridWidth, height: MONTH_ROW_HEIGHT }} aria-hidden="true">
      {markers.map((marker, index) => (
        <span
          key={marker.column}
          ref={(element) => {
            refs.current[index] = element;
          }}
          style={{ left: positions[index] }}
        >
          {marker.label}
        </span>
      ))}
    </div>
  );
}

function WeekGrid({
  series,
  activity,
  cumulative,
  t,
}: {
  series: ActivitySeries;
  activity: AggregateSeries;
  cumulative: boolean;
  t: Translate;
}) {
  const [hover, setHover] = useState<{ column: number; y: number } | null>(null);
  const maxValue = activity.values.reduce(
    (max, value, index) => (activity.isCovered[index] ? Math.max(max, value) : max),
    0,
  );
  const weekValue = (column: number) =>
    activity.isCovered[column] ? formatCompactTokens(activity.values[column]) : t("TokenActivityUnavailable");
  const hasUnavailable = activity.isCovered.some((covered, index) => isVisibleWeek(series, index) && !covered);
  const total = formatCompactTokens(
    cumulative ? (activity.values[activity.values.length - 1] ?? 0) : seriesTotal(activity.values),
  );
  const label = hasUnavailable ? `${total} · ${coverageText(series, t)}` : total;

  return (
    <div role="group" aria-label={`${t("TokenActivityTitle")}: ${label}`} data-testid="token-activity-weeks">
      <ScrollableGrid onScrolled={() => setHover(null)} t={t}>
        {(frame, visible) => {
          const cell = frame.cell;
          const inset = (frame.pitch - cell) / 2;
          let tooltip: ReactNode = null;
          if (hover) {
            const size = tooltipSize(frame, visible);
            tooltip = (
              <Tooltip
                title={weekValue(hover.column)}
                subtitle={formatDay(weekStartDay(series, hover.column))}
                centerX={tooltipCenterX(hover.column * frame.pitch + frame.pitch / 2, size.width, frame.width, visible)}
                originY={tooltipOriginY(hover.y, size.height, frame.height)}
                width={size.width}
                height={size.height}
              />
            );
          }
          return (
            <div className="token-activity__grid" style={{ width: frame.width, height: frame.height }}>
              <svg
                width={frame.width}
                height={frame.height}
                onMouseMove={(event) => {
                  const { x, y } = pointerPosition(event);
                  const hit = cellAt(x, y, frame.pitch);
                  setHover(hit && isVisibleWeek(series, hit.column) ? { column: hit.column, y } : null);
                }}
                onMouseLeave={() => setHover(null)}
              >
                {activity.values.map((value, column) => {
                  if (!isVisibleWeek(series, column)) return null;
                  const filled = weekFill(value, maxValue);
                  const covered = activity.isCovered[column];
                  return (
                    <g key={column} data-week={dayKey(weekStartDay(series, column))}>
                      <title>{`${formatDay(weekStartDay(series, column))}: ${weekValue(column)}`}</title>
                      {Array.from({ length: DAY_COUNT }, (_, row) => (
                        <Cell
                          key={row}
                          x={column * frame.pitch + inset}
                          y={row * frame.pitch + inset}
                          size={cell}
                          level={!covered ? null : row >= DAY_COUNT - filled ? 2 : 0}
                        />
                      ))}
                    </g>
                  );
                })}
              </svg>
              {tooltip}
            </div>
          );
        }}
      </ScrollableGrid>
    </div>
  );
}
