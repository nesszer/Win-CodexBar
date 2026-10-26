import { useId, useState, type CSSProperties } from "react";
import { useLocale } from "../../hooks/useLocale";
import type {
  DashboardKpi,
  DashboardPoint,
  InlineDashboardModel,
} from "../../lib/inlineDashboard";

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
            data-empty={point.value == null ? "true" : undefined}
            title={point.title}
            onMouseEnter={() => setHovered(index)}
            style={{
              height: `${barHeight(point.value, maxValue)}px`,
              opacity: barOpacity(point.value, maxValue),
            }}
          />
        ))}
      </div>
    </div>
  );
}

/** The model's six-KPI layout (a History window longer than the quota window). */
const SIX_KPI_LAYOUT = 6;
const SIX_KPI_HISTORY_INDEX = 4;

/**
 * Collapsed summary: today, then the selected History window. In the six-KPI
 * layout that window follows the quota window's KPIs; elsewhere it is the
 * second KPI.
 */
export function summaryKpis(kpis: DashboardKpi[]): DashboardKpi[] {
  const history = kpis.length === SIX_KPI_LAYOUT ? kpis[SIX_KPI_HISTORY_INDEX] : kpis[1];
  return [kpis[0], history].filter((kpi): kpi is DashboardKpi => kpi != null);
}

/**
 * The card's Cost section (upstream `InlineUsageDashboardContent`): a titled,
 * collapsible block. Collapsed it summarises today and the History window;
 * expanded it adds the two-column KPI grid, a 58 px daily cost chart, recent
 * quota windows and short detail lines.
 */
export default function InlineDashboard({
  model,
  onToggle,
  collapsible = true,
}: {
  model: InlineDashboardModel;
  onToggle?: () => void;
  /** The classic look shows the whole dashboard with no Cost toggle. */
  collapsible?: boolean;
}) {
  const { t } = useLocale();
  const [expanded, setExpanded] = useState(false);
  const bodyId = useId();
  if (!collapsible) {
    return (
      <section className="card-dash">
        <DashboardBody model={model} />
      </section>
    );
  }
  return (
    <section className="card-dash">
      <button
        type="button"
        className="card-dash__toggle"
        aria-expanded={expanded}
        aria-controls={bodyId}
        onClick={() => {
          setExpanded((value) => !value);
          onToggle?.();
        }}
      >
        <span className="card-dash__title">{t("CostTitle")}</span>
        <svg className="card-dash__chevron" viewBox="0 0 16 16" aria-hidden="true">
          <path d="M6 3.5L10.5 8L6 12.5" />
        </svg>
      </button>
      {!expanded && (
        <div className="card-dash__summary">
          {summaryKpis(model.kpis).map((kpi) => (
            <span className="card-dash__summary-line" key={kpi.title}>
              {kpi.title}: {kpi.value}
            </span>
          ))}
        </div>
      )}
      <div className="card-dash__body" id={bodyId} hidden={!expanded}>
        {expanded && <DashboardBody model={model} />}
      </div>
    </section>
  );
}

function DashboardBody({ model }: { model: InlineDashboardModel }) {
  const { t } = useLocale();
  return (
    <>
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
    </>
  );
}
