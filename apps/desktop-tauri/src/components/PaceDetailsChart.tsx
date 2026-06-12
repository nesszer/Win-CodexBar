import type { RateWindowSnapshot } from "../types/bridge";
import { getPaceChartSnapshot } from "../lib/paceBudget";

const CHART_WIDTH = 300;
const CHART_HEIGHT = 76;
const CHART_PADDING = 4;

function chartX(percent: number): number {
  return CHART_PADDING + (percent / 100) * (CHART_WIDTH - 2 * CHART_PADDING);
}

function chartY(percent: number): number {
  return CHART_HEIGHT - CHART_PADDING
    - (percent / 100) * (CHART_HEIGHT - 2 * CHART_PADDING);
}

export default function PaceDetailsChart({
  snap,
}: {
  snap: RateWindowSnapshot;
}) {
  const chart = getPaceChartSnapshot(snap);
  if (!chart) return null;

  const startX = chartX(0);
  const startY = chartY(0);
  const nowX = chartX(chart.elapsedPercent);
  const actualY = chartY(chart.usedPercent);
  const resetX = chartX(100);
  const idealResetY = chartY(100);
  const projectionY = chartY(chart.projectedPercent);

  return (
    <div className="pace-details-chart">
      <svg
        viewBox={`0 0 ${CHART_WIDTH} ${CHART_HEIGHT}`}
        role="img"
        aria-label="Average usage pace and projection through the current rate window"
      >
        <line
          className="pace-details-chart__grid"
          x1={startX}
          y1={startY}
          x2={resetX}
          y2={startY}
        />
        <line
          className="pace-details-chart__ideal"
          x1={startX}
          y1={startY}
          x2={resetX}
          y2={idealResetY}
        />
        <polyline
          className="pace-details-chart__actual"
          points={`${startX},${startY} ${nowX},${actualY}`}
        />
        <line
          className="pace-details-chart__projection"
          x1={nowX}
          y1={actualY}
          x2={resetX}
          y2={projectionY}
        />
        <circle
          className="pace-details-chart__point"
          cx={nowX}
          cy={actualY}
          r="3"
        />
      </svg>
      <div className="pace-details-chart__legend">
        <span data-series="actual">Average so far</span>
        <span data-series="ideal">Ideal pace</span>
        <span data-series="projection">Projection</span>
      </div>
    </div>
  );
}
