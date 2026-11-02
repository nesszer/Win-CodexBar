export const TRAY_SCALE_MIN = 100;
export const TRAY_SCALE_MAX = 200;
export const TRAY_SCALE_STEP = 5;

export function clampTrayScalePercent(value: number): number {
  if (!Number.isFinite(value)) return TRAY_SCALE_MIN;
  return Math.min(TRAY_SCALE_MAX, Math.max(TRAY_SCALE_MIN, value));
}

export const TRAY_PANEL_WIDTH_DEFAULT = 310;
export const TRAY_PANEL_WIDTH_MIN = 280;
export const TRAY_PANEL_WIDTH_MAX = 560;

export function clampTrayPanelWidth(value: number): number {
  if (!Number.isFinite(value)) return TRAY_PANEL_WIDTH_DEFAULT;
  return Math.round(Math.min(TRAY_PANEL_WIDTH_MAX, Math.max(TRAY_PANEL_WIDTH_MIN, value)));
}

/** Gap kept between the panel and the work-area edges, as for the height. */
const TRAY_WORK_AREA_MARGIN = 16;

/**
 * Caps the scaled panel width (logical px) at the work-area width, which
 * `get_work_area_rect` reports in physical px. A 560 px panel at 200 % scale
 * would otherwise ask for 1,120 px.
 */
export function fitTrayWidthToWorkArea(
  width: number,
  workAreaPhysicalWidth: number | null,
  scaleFactor: number,
): number {
  if (workAreaPhysicalWidth == null || !Number.isFinite(workAreaPhysicalWidth)) return width;
  const scale = Number.isFinite(scaleFactor) && scaleFactor > 0 ? scaleFactor : 1;
  const available = Math.floor(workAreaPhysicalWidth / scale) - TRAY_WORK_AREA_MARGIN;
  if (available <= 0) return width;
  return Math.min(width, available);
}
