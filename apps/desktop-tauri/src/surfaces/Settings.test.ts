import { describe, expect, it } from "vitest";
import { TAB_META } from "./Settings";

describe("Settings navigation", () => {
  it("lists providers separately after general", () => {
    expect(TAB_META.slice(0, 2)).toEqual([
      { id: "general", labelKey: "TabGeneral" },
      { id: "providers", labelKey: "TabProviders" },
    ]);
  });
});
