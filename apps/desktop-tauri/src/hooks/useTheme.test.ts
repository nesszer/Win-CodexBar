import { describe, expect, it, vi } from "vitest";
import { resolveTheme } from "./useTheme";

describe("resolveTheme", () => {
  it("uses the OS light preference in auto mode", () => {
    vi.stubGlobal("matchMedia", (query: string) => ({
      matches: query === "(prefers-color-scheme: light)",
    }));

    expect(resolveTheme("auto")).toBe("light");

    vi.unstubAllGlobals();
  });

  it("falls back to dark in auto mode when light is not preferred", () => {
    vi.stubGlobal("matchMedia", () => ({ matches: false }));

    expect(resolveTheme("auto")).toBe("dark");

    vi.unstubAllGlobals();
  });
});
