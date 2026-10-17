/**
 * Pixel geometry for the stacked spend trend chart. Positions come from slot
 * fractions of the model domain, so hours keep their real length on
 * daylight-saving days and partial weeks or months keep their full width.
 */

/** Upstream `.plotDimension(startPadding: 18, endPadding: 18)`. */
export const PLOT_EDGE_PADDING = 18;
/** Upstream `BarMark(width: .ratio(0.72))`. */
export const BAR_WIDTH_RATIO = 0.72;
export const TOP_RADIUS = 3;

export interface PlotFrame {
  /** Full plot width, wider than the viewport when the chart scrolls. */
  width: number;
  height: number;
}

export interface SlotRect {
  x: number;
  width: number;
}

/** Where a slot sits horizontally, and the narrower bar drawn inside it. */
export function slotRect(frame: PlotFrame, fraction: { start: number; width: number }): SlotRect {
  const usable = Math.max(0, frame.width - PLOT_EDGE_PADDING * 2);
  return { x: PLOT_EDGE_PADDING + fraction.start * usable, width: fraction.width * usable };
}

export function barRect(slot: SlotRect): SlotRect {
  const width = Math.max(1, slot.width * BAR_WIDTH_RATIO);
  return { x: slot.x + (slot.width - width) / 2, width };
}

/** The domain fraction under an x offset, or null outside the padded plot. */
export function fractionAt(frame: PlotFrame, x: number): number | null {
  const usable = frame.width - PLOT_EDGE_PADDING * 2;
  if (usable <= 0) return null;
  const fraction = (x - PLOT_EDGE_PADDING) / usable;
  return fraction >= 0 && fraction < 1 ? fraction : null;
}

export function valueY(frame: PlotFrame, value: number, yMax: number): number {
  if (yMax <= 0) return frame.height;
  return frame.height - (Math.max(0, value) / yMax) * frame.height;
}

/**
 * A stack segment as an SVG path. Only the top of the stack rounds its upper
 * corners (upstream `spendTopOfStackIDs`), so stacked sources read as one bar.
 */
export function segmentPath(x: number, top: number, width: number, bottom: number, rounded: boolean): string {
  const height = Math.max(0, bottom - top);
  const radius = rounded ? Math.min(TOP_RADIUS, width / 2, height) : 0;
  const right = x + width;
  if (radius <= 0) return `M${x},${bottom}V${top}H${right}V${bottom}Z`;
  return [
    `M${x},${bottom}`,
    `V${top + radius}`,
    `Q${x},${top} ${x + radius},${top}`,
    `H${right - radius}`,
    `Q${right},${top} ${right},${top + radius}`,
    `V${bottom}`,
    "Z",
  ].join("");
}

/** About `desired` round axis values from 0 up to `max` (1, 2, 2.5 or 5 steps). */
export function niceTicks(max: number, desired = 4): number[] {
  if (!Number.isFinite(max) || max <= 0) return [0];
  const raw = max / Math.max(1, desired);
  const magnitude = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((factor) => factor * magnitude).find((value) => value >= raw) ?? raw;
  const ticks: number[] = [];
  for (let value = 0; value <= max + step * 1e-9; value += step) ticks.push(Number(value.toPrecision(12)));
  return ticks;
}
