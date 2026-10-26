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
