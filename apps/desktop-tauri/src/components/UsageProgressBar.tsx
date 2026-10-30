import { useId } from "react";
import type { UsageBarModel } from "../lib/metricRowModel";

const DEFAULT_HEIGHT = 6;
const CUT_OPACITY = 0.9;
const WARNING_CUT = 5;
const WARNING_STRIPE = 1;
// macOS UsageProgressBar: subtle ticks are 1px and half height, high contrast
// ticks are 2px and full height (opacity lives in the CSS classes).
const WORKDAY_TICK = { subtle: 1, high_contrast: 2 } as const;
const PACE_CUT = 6;
const PACE_STRIPE = 2;

function centered(percent: number, width: number) {
  return { x: `${percent}%`, width, transform: `translate(${-width / 2} 0)` };
}

function CutMask({
  id,
  cuts,
  width,
  height,
}: {
  id: string;
  cuts: number[];
  width: number;
  height: number;
}) {
  return (
    <mask
      id={id}
      maskUnits="userSpaceOnUse"
      x={0}
      y={0}
      width="100%"
      height={height}
      colorInterpolation="sRGB"
    >
      <rect width="100%" height={height} fill="white" />
      {cuts.map((percent) => (
        <rect
          key={percent}
          {...centered(percent, width)}
          height={height}
          fill="black"
          fillOpacity={CUT_OPACITY}
          shapeRendering="crispEdges"
        />
      ))}
    </mask>
  );
}

/** `height` is the bar thickness in px; the ends are fully rounded at any thickness. */
export default function UsageProgressBar({
  bar,
  label,
  height = DEFAULT_HEIGHT,
}: {
  bar: UsageBarModel;
  label: string;
  height?: number;
}) {
  const radius = height / 2;
  const id = useId().replace(/:/g, "");
  const warnings = bar.markers.filter((marker) => marker.kind === "warning").map((marker) => marker.percent);
  const workdays = bar.markers.filter((marker) => marker.kind === "workday").map((marker) => marker.percent);
  const pace = bar.pacePercent;
  const highContrastTicks = bar.workdayTickStyle === "high_contrast";
  return (
    <svg
      className="menu-metric__progress"
      width="100%"
      height={height}
      role="progressbar"
      aria-label={label}
      aria-valuenow={bar.valuePercent}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuetext={bar.valueText}
    >
      {warnings.length > 0 && <CutMask id={`${id}-warnings`} cuts={warnings} width={WARNING_CUT} height={height} />}
      {pace != null && <CutMask id={`${id}-pace`} cuts={[pace]} width={PACE_CUT} height={height} />}
      <g mask={pace != null ? `url(#${id}-pace)` : undefined}>
        <g mask={warnings.length > 0 ? `url(#${id}-warnings)` : undefined}>
          <rect className="menu-metric__progress-track" width="100%" height={height} rx={radius} />
          {bar.fillPercent > 0 && (
            <rect
              className="menu-metric__progress-fill"
              width={`${bar.fillPercent}%`}
              height={height}
              rx={radius}
            />
          )}
        </g>
        {warnings.map((percent) => (
          <rect
            key={percent}
            className="menu-metric__progress-warning"
            {...centered(percent, WARNING_STRIPE)}
            height={height}
            shapeRendering="crispEdges"
          />
        ))}
        {workdays.map((percent) => (
          <rect
            key={percent}
            className={`menu-metric__progress-workday${highContrastTicks ? " menu-metric__progress-workday--high-contrast" : ""}`}
            {...centered(percent, WORKDAY_TICK[bar.workdayTickStyle])}
            y={highContrastTicks ? 0 : height / 2}
            height={highContrastTicks ? height : height / 2}
            shapeRendering="crispEdges"
          />
        ))}
      </g>
      {pace != null && (
        <rect
          className="menu-metric__progress-pace"
          data-deficit={String(bar.paceDeficit)}
          {...centered(pace, PACE_STRIPE)}
          height={height}
          shapeRendering="crispEdges"
        />
      )}
    </svg>
  );
}
