import { describe, expect, it } from "vitest";
import { clampTrayScalePercent } from "./trayScale";

describe("clampTrayScalePercent", () => {
  it("keeps the panel scale between 100% and 200%", () => {
    expect(clampTrayScalePercent(150)).toBe(150);
    expect(clampTrayScalePercent(40)).toBe(100);
    expect(clampTrayScalePercent(260)).toBe(200);
    expect(clampTrayScalePercent(Number.NaN)).toBe(100);
  });
});
