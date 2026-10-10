import { useId } from "react";
import type { UsageBarModel } from "../lib/metricRowModel";

const HEIGHT = 6;
const RADIUS = 3;
const CUT_OPACITY = 0.9;
const WARNING_CUT = 5;
const WARNING_STRIPE = 1;
const WORKDAY_TICK = 1;
const PACE_CUT = 6;
const PACE_STRIPE = 2;

/** A rect `width` px wide, centred on a percentage of the track. */
function centered(percent: number, width: number) {
  return { x: `${percent}%`, width, transform: `translate(${-width / 2} 0)` };
}

/** White keeps, each black cut keeps a tenth, like a 0.9 destination-out on macOS. */
function CutMask({ id, cuts, width }: { id: string; cuts: number[]; width: number }) {
  return (
    <mask
      id={id}
      maskUnits="userSpaceOnUse"
      x={0}
      y={0}
      width="100%"
      height={HEIGHT}
      colorInterpolation="sRGB"
    >
      <rect width="100%" height={HEIGHT} fill="white" />
      {cuts.map((percent) => (
        <rect
          key={percent}
          {...centered(percent, width)}
          height={HEIGHT}
          fill="black"
          fillOpacity={CUT_OPACITY}
          shapeRendering="crispEdges"
        />
      ))}
    </mask>
  );
}

/** The 6px usage bar of a metric row, after the macOS `UsageProgressBar`. */
export default function UsageProgressBar({ bar, label }: { bar: UsageBarModel; label: string }) {
  const id = useId().replace(/:/g, "");
  const warnings = bar.markers.filter((marker) => marker.kind === "warning").map((marker) => marker.percent);
  const workdays = bar.markers.filter((marker) => marker.kind === "workday").map((marker) => marker.percent);
  const pace = bar.pacePercent;
  return (
    <svg
      className="menu-metric__progress"
      width="100%"
      height={HEIGHT}
      role="progressbar"
      aria-label={label}
      aria-valuenow={bar.valuePercent}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuetext={bar.valueText}
    >
      {warnings.length > 0 && <CutMask id={`${id}-warnings`} cuts={warnings} width={WARNING_CUT} />}
      {pace != null && <CutMask id={`${id}-pace`} cuts={[pace]} width={PACE_CUT} />}
      <g mask={pace != null ? `url(#${id}-pace)` : undefined}>
        <g mask={warnings.length > 0 ? `url(#${id}-warnings)` : undefined}>
          <rect className="menu-metric__progress-track" width="100%" height={HEIGHT} rx={RADIUS} />
          {bar.fillPercent > 0 && (
            <rect
              className="menu-metric__progress-fill"
              width={`${bar.fillPercent}%`}
              height={HEIGHT}
              rx={RADIUS}
            />
          )}
        </g>
        {warnings.map((percent) => (
          <rect
            key={percent}
            className="menu-metric__progress-warning"
            {...centered(percent, WARNING_STRIPE)}
            height={HEIGHT}
            shapeRendering="crispEdges"
          />
        ))}
        {workdays.map((percent) => (
          <rect
            key={percent}
            className="menu-metric__progress-workday"
            {...centered(percent, WORKDAY_TICK)}
            y={HEIGHT / 2}
            height={HEIGHT / 2}
            shapeRendering="crispEdges"
          />
        ))}
      </g>
      {pace != null && (
        <rect
          className="menu-metric__progress-pace"
          data-deficit={String(!bar.paceOnTop)}
          {...centered(pace, PACE_STRIPE)}
          height={HEIGHT}
          shapeRendering="crispEdges"
        />
      )}
    </svg>
  );
}
