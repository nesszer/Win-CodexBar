import { useState, type CSSProperties } from "react";
import { useLocale } from "../../hooks/useLocale";
import type { DashboardPoint, InlineDashboardModel } from "../../lib/inlineDashboard";

/**
 * Upstream frames the chart at 58 px: a 13 px caption2 scale line, a 2 px gap,
 * then the bars, so a full bar is 43 px tall.
 */
const BAR_AREA_HEIGHT = 58 - 13 - 2;

/** Upstream `MiniUsageBars.height`: 1 px for no data, otherwise at least 3 px. */
export function barHeight(value: number | null, maxValue: number): number {
  if (value == null || maxValue <= 0) return 1;
  const ratio = Math.min(1, value / maxValue);
  if (ratio <= 0) return 1;
  return Math.max(3, ratio * BAR_AREA_HEIGHT);
}

/** Upstream `MiniUsageBars.fill`: provider color at 0.42 + ratio × 0.58, ratio ≥ 0.18. */
export function barOpacity(value: number | null, maxValue: number): number {
  if (value == null) return 0;
  const ratio = Math.max(0.18, maxValue > 0 ? Math.min(1, value / maxValue) : 0);
  return Math.round((0.42 + ratio * 0.58) * 1000) / 1000;
}

function UsageBars({
  points,
  maxValue,
  scaleLabel,
}: {
  points: DashboardPoint[];
  maxValue: number;
  scaleLabel: string | null;
}) {
  const { t } = useLocale();
  const [hovered, setHovered] = useState<number | null>(null);
  const hoverTitle = hovered != null ? points[hovered]?.title ?? null : null;
  const style = { "--bar-count": points.length } as CSSProperties;
  return (
    <div className="card-dash__chart" onMouseLeave={() => setHovered(null)}>
      <div
        className={`card-dash__scale${hoverTitle ? " card-dash__scale--hover" : ""}`}
        aria-hidden={hoverTitle ? true : undefined}
      >
        {hoverTitle ?? scaleLabel ?? " "}
      </div>
      <div
        className="card-dash__bars"
        style={style}
        role="img"
        aria-label={t("PanelThirtyDayCostHistogram")}
      >
        {points.map((point, index) => (
          <span
            key={`${point.date}-${index}`}
            className={`card-dash__bar${hovered === index ? " card-dash__bar--selected" : ""}`}
            data-empty={point.value == null && !point.hasUnpricedUsage ? "true" : undefined}
            data-unpriced={point.hasUnpricedUsage ? "true" : undefined}
            title={point.title}
            onMouseEnter={() => setHovered(index)}
            style={{
              height: `${point.hasUnpricedUsage ? 3 : barHeight(point.value, maxValue)}px`,
              opacity: point.hasUnpricedUsage ? 0.55 : barOpacity(point.value, maxValue),
              outline: point.hasUnpricedUsage ? "1px dashed currentColor" : undefined,
              background: point.hasUnpricedUsage ? "transparent" : undefined,
            }}
          />
        ))}
      </div>
    </div>
  );
}

/**
 * The card's inline usage dashboard (upstream `InlineUsageDashboardContent`):
 * a two-column KPI grid, a 58 px daily cost chart, recent quota windows and
 * short detail lines.
 */
export default function InlineDashboard({ model }: { model: InlineDashboardModel }) {
  const { t } = useLocale();
  return (
    <section className="card-dash">
      <div className="card-dash__kpis">
        {model.kpis.map((kpi) => (
          <div className="card-dash__kpi" key={kpi.title}>
            <span className="card-dash__kpi-title">{kpi.title}</span>
            <span
              className={`card-dash__kpi-value${kpi.emphasis ? " card-dash__kpi-value--emphasis" : ""}`}
            >
              {kpi.value}
            </span>
          </div>
        ))}
      </div>
      {model.points.length > 0 && (
        <UsageBars points={model.points} maxValue={model.maxValue} scaleLabel={model.scaleLabel} />
      )}
      {model.windows.length > 0 && (
        <div className="card-dash__windows">
          <span className="card-dash__caption">{t("PanelRecentWindows")}</span>
          {model.windows.map((window) => (
            <div className="card-dash__window" key={window.id}>
              <div className="card-dash__window-line">
                <span className="card-dash__window-title">{window.title}</span>
                <span className="card-dash__window-value">{window.value}</span>
              </div>
              {window.note && <span className="card-dash__caption">{window.note}</span>}
              <span className="card-dash__caption card-dash__window-range">{window.range}</span>
            </div>
          ))}
        </div>
      )}
      {model.detailLines.length > 0 && (
        <div className="card-dash__details">
          {model.detailLines.map((line) => (
            <span className="card-dash__detail" key={line}>
              {line}
            </span>
          ))}
        </div>
      )}
    </section>
  );
}
