export const WIDTH = 280;
export const DATE_EDGE_PADDING = 36;
export const BAR_GAP = 2;
export const PLOT_WIDTH = WIDTH - DATE_EDGE_PADDING * 2;
const SCROLLABLE_BAR_WIDTH = 1.5;

export function shouldRenderCenterMax(count: number): boolean {
  return count >= 3;
}

export function getScrollableChartWidth(count: number): number {
  const barCount = Math.max(1, count);
  const plotWidth = barCount * SCROLLABLE_BAR_WIDTH + (barCount - 1) * BAR_GAP;
  return Math.max(WIDTH, DATE_EDGE_PADDING * 2 + plotWidth);
}

export function getBarWidth(count: number, width = WIDTH): number {
  const barCount = Math.max(1, count);
  const plotWidth = width - DATE_EDGE_PADDING * 2;
  return Math.max(1, (plotWidth - (barCount - 1) * BAR_GAP) / barCount);
}

export function getBarX(index: number, count: number, width = WIDTH): number {
  return DATE_EDGE_PADDING + index * (getBarWidth(count, width) + BAR_GAP);
}

export function getBarCenter(index: number, count: number, width = WIDTH): number {
  return getBarX(index, count, width) + getBarWidth(count, width) / 2;
}

export function getLineX(index: number, count: number): number {
  if (count <= 1) return DATE_EDGE_PADDING;
  return DATE_EDGE_PADDING + (index / (count - 1)) * PLOT_WIDTH;
}
