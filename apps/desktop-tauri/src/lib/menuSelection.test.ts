import { describe, expect, it } from "vitest";
import { accentSelectionStyle } from "./menuSelection";

describe("accentSelectionStyle", () => {
  it("paints the selection with the Windows accent and white text when white reads on it", () => {
    expect(accentSelectionStyle("#0078d4")).toEqual({
      "--mac-selection-bg": "#0078d4",
      "--mac-selection-text": "#fff",
    });
  });

  it("switches to dark text on a light accent", () => {
    expect(accentSelectionStyle("#ffb900")).toEqual({
      "--mac-selection-bg": "#ffb900",
      "--mac-selection-text": "rgba(0, 0, 0, 0.85)",
    });
  });

  it("keeps the stylesheet defaults without an accent", () => {
    expect(accentSelectionStyle(null)).toEqual({});
    expect(accentSelectionStyle("accent")).toEqual({});
  });
});
