import { describe, expect, it } from "vitest";
import { loadStyles } from "../test/styles";

function themeTokens(css: string, theme: "dark" | "light"): Set<string> {
  const names = new Set<string>();
  const blocks = css.matchAll(
    new RegExp(`(?:^|\\r?\\n)\\[data-theme="${theme}"\\]\\s*\\{([^}]*)\\}`, "g"),
  );
  for (const [, body] of blocks) {
    for (const [, name] of body.matchAll(/(--[\w-]+)\s*:/g)) {
      names.add(name);
    }
  }
  return names;
}

describe("theme tokens", () => {
  it("gives every dark theme token a light value", () => {
    const css = loadStyles();
    const dark = themeTokens(css, "dark");
    const light = themeTokens(css, "light");
    expect(dark.size).toBeGreaterThan(0);
    expect([...dark].filter((name) => !light.has(name))).toEqual([]);
  });
});
