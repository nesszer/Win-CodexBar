import { describe, expect, it } from "vitest";
import { formatEta } from "./formatEta";

describe("formatEta", () => {
  it("formats durations using minutes, hours, and days", () => {
    expect(formatEta(0)).toBe("0m");
    expect(formatEta(30 * 60)).toBe("30m");
    expect(formatEta(60 * 60)).toBe("1h");
    expect(formatEta(23 * 60 * 60)).toBe("23h");
    expect(formatEta(24 * 60 * 60)).toBe("1d");
  });

  it("clamps negative durations to zero", () => {
    expect(formatEta(-60)).toBe("0m");
  });
});
