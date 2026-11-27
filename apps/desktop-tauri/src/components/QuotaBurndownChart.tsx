import type { QuotaBurndownSnapshot, QuotaBurndownPointSnapshot } from "../types/bridge";
import type { LocaleKey } from "../i18n/keys";
import { formatRelativeUpdated } from "../lib/relativeTime";
import { useNow } from "../hooks/useNow";

const CHART_WIDTH = 300;
const CHART_HEIGHT = 76;
const CHART_PADDING = 4;

function xFor(timeMs: number, startMs: number, endMs: number): number {
  const span = Math.max(1, endMs - startMs);
  const fraction = Math.min(1, Math.max(0, (timeMs - startMs) / span));
  return CHART_PADDING + fraction * (CHART_WIDTH - 2 * CHART_PADDING);
}

function yFor(remainingPercent: number): number {
  const clamped = Math.min(100, Math.max(0, remainingPercent));
  return (
    CHART_HEIGHT -
    CHART_PADDING -
    (clamped / 100) * (CHART_HEIGHT - 2 * CHART_PADDING)
  );
}

function polyline(points: QuotaBurndownPointSnapshot[], startMs: number, endMs: number): string {
  return points
    .map((point) => {
      const timeMs = Date.parse(point.capturedAt);
      const x = xFor(timeMs, startMs, endMs);
      const y = yFor(point.remainingPercent);
      return `${x.toFixed(2)},${y.toFixed(2)}`;
    })
    .join(" ");
}

/**
 * Recorded remaining-quota burndown (upstream 0.70.0 #4085): the solid line
 * joins recorded remaining percentages (never extended past the last
 * capture), the dashed line is the even-use guide from 100% at the window
 * start to 0% at reset. Expired windows never reach this component (the
 * backend model yields None for them).
 */
export default function QuotaBurndownChart({
  burndown,
  t,
  nowMs,
}: {
  burndown: QuotaBurndownSnapshot;
  t: (key: LocaleKey) => string;
  nowMs?: number;
}) {
  // Called before the early returns below; keeps the capture age advancing.
  const liveNowMs = useNow();
  if (burndown.samples.length === 0) return null;

  const startMs = Date.parse(burndown.start);
  const resetMs = Date.parse(burndown.reset);
  if (!Number.isFinite(startMs) || !Number.isFinite(resetMs) || resetMs <= startMs) {
    return null;
  }

  const last = burndown.samples[burndown.samples.length - 1];
  const lastMs = Date.parse(last.capturedAt);
  const idealStartX = xFor(Date.parse(burndown.ideal[0].capturedAt), startMs, resetMs);
  const idealStartY = yFor(burndown.ideal[0].remainingPercent);
  const idealEndX = xFor(Date.parse(burndown.ideal[1].capturedAt), startMs, resetMs);
  const idealEndY = yFor(burndown.ideal[1].remainingPercent);
  const remaining = Math.round(last.remainingPercent);
  const captureAge = formatRelativeUpdated(lastMs, t, nowMs ?? liveNowMs);

  return (
    <div className="quota-burndown-chart">
      <svg
        viewBox={`0 0 ${CHART_WIDTH} ${CHART_HEIGHT}`}
        role="img"
        aria-label={t("BurndownChartAriaLabel")}
        data-series={burndown.series}
      >
        <line
          className="quota-burndown-chart__grid"
          x1={CHART_PADDING}
          y1={yFor(0)}
          x2={CHART_WIDTH - CHART_PADDING}
          y2={yFor(0)}
        />
        <line
          className="quota-burndown-chart__ideal"
          x1={idealStartX}
          y1={idealStartY}
          x2={idealEndX}
          y2={idealEndY}
        />
        <polyline
          className="quota-burndown-chart__observed"
          points={polyline(burndown.samples, startMs, resetMs)}
        />
        <circle
          className="quota-burndown-chart__point"
          cx={xFor(lastMs, startMs, resetMs)}
          cy={yFor(last.remainingPercent)}
          r="3"
        />
      </svg>
      <div className="quota-burndown-chart__axis">
        <span data-boundary="start">{formatBoundary(burndown.start, resetMs - startMs)}</span>
        <span data-boundary="reset">{formatBoundary(burndown.reset, resetMs - startMs)}</span>
      </div>
      <div className="quota-burndown-chart__caption">
        <span>{`${remaining}% ${t("PanelLeftSuffix")}`}</span>
        <span>{`${t("ClaudeSwapHistoricalUsage")} · ${captureAge}`}</span>
      </div>
    </div>
  );
}

/** Weekly/monthly-length windows show a calendar date; session ones stay compact. */
function formatBoundary(iso: string, spanMs: number): string {
  const date = new Date(iso);
  if (!Number.isFinite(date.getTime())) return "—";
  const time = date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  if (spanMs >= 24 * 60 * 60 * 1000) {
    const day = date.toLocaleDateString([], { weekday: "short", month: "short", day: "numeric" });
    return `${day} ${time}`;
  }
  return time;
}
