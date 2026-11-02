import { describe, expect, it } from "vitest";
import { clampTrayScalePercent, fitTrayWidthToWorkArea } from "./trayScale";

describe("clampTrayScalePercent", () => {
  it("keeps the panel scale between 100% and 200%", () => {
    expect(clampTrayScalePercent(150)).toBe(150);
    expect(clampTrayScalePercent(40)).toBe(100);
    expect(clampTrayScalePercent(260)).toBe(200);
    expect(clampTrayScalePercent(Number.NaN)).toBe(100);
  });
});

describe("fitTrayWidthToWorkArea", () => {
  it("caps a wide scaled panel at the work area, in logical px", () => {
    // 560 px at 200 % on a 1024 px work area at 100 % DPI.
    expect(fitTrayWidthToWorkArea(1120, 1024, 1)).toBe(1008);
    expect(fitTrayWidthToWorkArea(1120, 1920, 1.5)).toBe(1120);
    expect(fitTrayWidthToWorkArea(1120, 1920, 2)).toBe(944);
  });

  it("leaves the width alone when it fits or the work area is unknown", () => {
    expect(fitTrayWidthToWorkArea(310, 1920, 1)).toBe(310);
    expect(fitTrayWidthToWorkArea(1120, null, 1)).toBe(1120);
    expect(fitTrayWidthToWorkArea(1120, 1366, Number.NaN)).toBe(1120);
  });
});
