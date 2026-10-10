/** Panel scale bounds in percent. The backend clamps `trayScalePercent` to
 *  the same range when it loads or saves settings. */
export const TRAY_SCALE_MIN = 100;
export const TRAY_SCALE_MAX = 200;
export const TRAY_SCALE_STEP = 5;

export function clampTrayScalePercent(value: number): number {
  if (!Number.isFinite(value)) return TRAY_SCALE_MIN;
  return Math.min(TRAY_SCALE_MAX, Math.max(TRAY_SCALE_MIN, value));
}
