import type { ProviderDisplayDetail } from "../types/bridge";

/**
 * One transient provider detail line: "{title}: {value} [secondary]"
 * plus an optional clamped progress bar.
 *
 * Shared by the tray menu card (`MenuCardDetails`) and the settings provider
 * detail (`UsageSection`); each surface passes its own layout classes.
 */
export function ProviderDisplayRow({
  detail,
  lineClassName,
  secondaryClassName,
  trackClassName,
  fillClassName,
}: {
  detail: ProviderDisplayDetail;
  lineClassName: string;
  secondaryClassName?: string;
  trackClassName: string;
  fillClassName: string;
}) {
  const progress = detail.progress;
  const progressPercent =
    progress &&
    Number.isFinite(progress.used) &&
    Number.isFinite(progress.total) &&
    progress.total > 0
      ? Math.max(0, Math.min(100, (progress.used / progress.total) * 100))
      : null;

  return (
    <div>
      <div className={lineClassName}>
        <span>{detail.title}: {detail.value}</span>
        {detail.secondaryValue && secondaryClassName && (
          <span className={secondaryClassName}>{detail.secondaryValue}</span>
        )}
      </div>
      {progressPercent != null && (
        <div className={trackClassName} aria-label={`${detail.title} progress`}>
          <div className={fillClassName} style={{ width: `${progressPercent}%` }} />
        </div>
      )}
    </div>
  );
}
