import { describe, expect, it } from "vitest";
import { barRect, fractionAt, niceTicks, PLOT_EDGE_PADDING, segmentPath, slotRect, valueY } from "./stackedTrendGeometry";

describe("stacked trend geometry", () => {
  const frame = { width: 236, height: 100 };

  it("maps slot fractions inside the edge padding and centers a 72% bar", () => {
    const slot = slotRect(frame, { start: 0.5, width: 0.25 });
    expect(slot).toEqual({ x: PLOT_EDGE_PADDING + 100, width: 50 });
    expect(barRect(slot)).toEqual({ x: slot.x + 7, width: 36 });
  });

  it("finds the domain fraction under the pointer and ignores the padding", () => {
    expect(fractionAt(frame, PLOT_EDGE_PADDING + 50)).toBeCloseTo(0.25);
    expect(fractionAt(frame, 4)).toBeNull();
    expect(fractionAt(frame, frame.width - 4)).toBeNull();
  });

  it("puts zero on the baseline and yMax at the top", () => {
    expect(valueY(frame, 0, 10)).toBe(100);
    expect(valueY(frame, 10, 10)).toBe(0);
    expect(valueY(frame, 5, 0)).toBe(100);
  });

  it("rounds only a top-of-stack segment and never past its height", () => {
    expect(segmentPath(0, 10, 20, 50, false)).toBe("M0,50V10H20V50Z");
    expect(segmentPath(0, 10, 20, 50, true)).toContain("Q0,10 3,10");
    expect(segmentPath(0, 49, 20, 50, true)).toContain("Q0,49 1,49");
  });

  it("chooses about four round axis steps", () => {
    expect(niceTicks(11.5)).toEqual([0, 5, 10]);
    expect(niceTicks(0.9)).toEqual([0, 0.25, 0.5, 0.75]);
    expect(niceTicks(0)).toEqual([0]);
  });
});
