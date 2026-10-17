/**
 * Signed pace percentage, e.g. `+12.5%`. `deltaPercent` is always usage ahead
 * of pace (positive = deficit); `reservePositive` only flips the displayed sign
 * (upstream `menuBarPaceReservePositive`), so colours and stages keep their
 * meaning. A delta that rounds to zero stays unsigned in both modes.
 */
export function formatSignedPace(deltaPercent: number, reservePositive: boolean): string {
  const magnitude = Math.abs(deltaPercent).toFixed(1);
  if (Number(magnitude) === 0) return `${magnitude}%`;
  const sign = deltaPercent >= 0 !== reservePositive ? "+" : "-";
  return `${sign}${magnitude}%`;
}
