import { describe, expect, it } from "vitest";
import { accentSelectionStyle } from "./menuSelection";

describe("accentSelectionStyle", () => {
  it("paints the selection with the Windows accent and white text when white reads on it", () => {
    expect(accentSelectionStyle("#0078d4")).toEqual({
      "--mac-selection-bg": "#0078d4",
      "--mac-selection-text": "#fff",
    });
  });

  it("switches to black text on a light accent", () => {
    expect(accentSelectionStyle("#ffb900")).toEqual({
      "--mac-selection-bg": "#ffb900",
      "--mac-selection-text": "#000",
    });
  });

  it("uses solid black on a mid gray accent, where 85% black would fall below 4.5:1", () => {
    expect(accentSelectionStyle("#7a7a7a")).toEqual({
      "--mac-selection-bg": "#7a7a7a",
      "--mac-selection-text": "#000",
    });
  });

  it("keeps the stylesheet defaults without an accent", () => {
    expect(accentSelectionStyle(null)).toEqual({});
    expect(accentSelectionStyle("accent")).toEqual({});
  });
});
