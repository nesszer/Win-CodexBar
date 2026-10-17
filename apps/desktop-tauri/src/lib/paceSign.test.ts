import { describe, expect, it } from "vitest";
import { formatSignedPace } from "./paceSign";

describe("formatSignedPace", () => {
  it("shows usage ahead of pace as positive by default", () => {
    expect(formatSignedPace(12.5, false)).toBe("+12.5%");
    expect(formatSignedPace(-8.25, false)).toBe("-8.3%");
  });

  it("shows reserve as positive when the preference is on", () => {
    expect(formatSignedPace(-8.25, true)).toBe("+8.3%");
    expect(formatSignedPace(12.5, true)).toBe("-12.5%");
  });

  it("keeps zero unsigned in both modes", () => {
    for (const reservePositive of [false, true]) {
      expect(formatSignedPace(0, reservePositive)).toBe("0.0%");
      expect(formatSignedPace(0.04, reservePositive)).toBe("0.0%");
      expect(formatSignedPace(-0.04, reservePositive)).toBe("0.0%");
    }
  });
});
