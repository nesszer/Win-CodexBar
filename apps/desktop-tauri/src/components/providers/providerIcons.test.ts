import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { TEST_PROVIDER_CATALOG } from "../../test/providerCatalog";
import { loadStyles, loadStylesheet, ruleBlock } from "../../test/styles";
import { PROVIDER_ICON_REGISTRY, brandColorOnDark, providerBarAccent } from "./providerIcons";

// Repository root, from apps/desktop-tauri/src/components/providers.
const REPO_ROOT = import.meta.dirname + "/../../../../../";

/**
 * The `ProviderId::X => "#RRGGBB"` rows of the Rust `brand_color` table in
 * `rust/src/core/provider.rs`, keyed by the lowercased variant name (which
 * is the registry id for every provider).
 */
function rustBrandColors(): Map<string, string> {
  // CI checks out with CRLF line endings; normalize so the "\n}\n" search below matches.
  const source = readFileSync(REPO_ROOT + "rust/src/core/provider.rs", "utf8").replace(/\r\n/g, "\n");
  const start = source.indexOf("pub fn brand_color(id: ProviderId)");
  const end = source.indexOf("\n}\n", start);
  const rows = new Map<string, string>();
  if (start < 0 || end < 0) return rows;
  for (const match of source
    .slice(start, end)
    .matchAll(/ProviderId::(\w+) => "(#[0-9A-Fa-f]{6})"/g)) {
    rows.set(match[1].toLowerCase(), match[2].toLowerCase());
  }
  return rows;
}

/** `--chart-<id>: rgb(r, g, b);` provider tokens from styles.css, as hex. */
function chartTokens(): Array<[string, string]> {
  const css = loadStyles();
  const tokens: Array<[string, string]> = [];
  for (const match of css.matchAll(
    /--chart-([a-z0-9]+): rgb\((\d+), (\d+), (\d+)\);/g,
  )) {
    const hex = [match[2], match[3], match[4]]
      .map((channel) => Number(channel).toString(16).padStart(2, "0"))
      .join("");
    tokens.push([match[1], `#${hex}`]);
  }
  return tokens;
}

// Upstream 0.70.0 palette audit (#4075, docs/provider-palette.md): the 16
// accents it adopted. The Rust side pins the same values and checks their
// contrast against the previous Windows accents.
const ADOPTED_ACCENTS: Array<[string, string]> = [
  ["abacus", "#814ee8"],
  ["amp", "#f34e3f"],
  ["augment", "#1aa049"],
  ["bedrock", "#01a88d"],
  ["clinepass", "#5487c8"],
  ["codebuff", "#00ff95"],
  ["commandcode", "#8c4edd"],
  ["cursor", "#f54e00"],
  ["deepseek", "#4d6bfe"],
  ["devin", "#317cff"],
  ["kiro", "#9046ff"],
  ["longcat", "#29e154"],
  ["mistral", "#ff5229"],
  ["neuralwatt", "#d55934"],
  ["sub2api", "#14b8a6"],
  ["venice", "#3c8fdd"],
];

// Mac CodexBar 0.70.0 brand colors adopted for parity (card PR 4,
// tray-card-spec section 4). Vercel is the one deliberate exception.
const MAC_PARITY_ACCENTS: Array<[string, string]> = [
  ["chutes", "#3184ff"],
  ["deepgram", "#6467f2"],
  ["doubao", "#3370ff"],
  ["exa", "#0143d9"],
  ["grok", "#10a37f"],
  ["groq", "#f56844"],
  ["helmcode", "#4934e1"],
  ["kilo", "#f27027"],
  ["litellm", "#4c89f0"],
  ["llmproxy", "#24b47e"],
  ["ollama", "#888888"],
  ["openaiapi", "#0f826e"],
  ["openrouter", "#6467f2"],
  ["perplexity", "#20b2aa"],
  ["poe", "#5d5cde"],
  ["qoder", "#10b981"],
  ["sakana", "#2975db"],
  ["stepfun", "#2196f2"],
  ["t3chat", "#f56647"],
  ["tinyapi", "#f97316"],
  ["typesafe", "#111111"],
  ["v0", "#111111"],
  ["warp", "#938bb4"],
  ["wayfinder", "#10a37f"],
  ["windsurf", "#34e8bb"],
  ["zed", "#084eff"],
];

describe("provider icon registry", () => {
  it("has explicit icon metadata for every provider in the catalog", () => {
    for (const [id] of TEST_PROVIDER_CATALOG) {
      expect(PROVIDER_ICON_REGISTRY[id], id).toBeDefined();
    }
  });

  it("ships the upstream Atlas Cloud glyph tinted by the brand color", () => {
    const svg = PROVIDER_ICON_REGISTRY.atlascloud.svgPath;
    expect(svg).toContain("<svg");
    expect(svg).toContain('fill="currentColor"');
    expect(svg).not.toContain('fill="#000"');
  });

  it("ships the official Devin hexagon mark as a currentColor template", () => {
    const entry = PROVIDER_ICON_REGISTRY.devin;
    expect(entry.svgPath).toContain('viewBox="0 0 128 128"');
    expect(entry.svgPath).toContain('fill="currentColor"');
    expect(entry.svgPath).not.toContain("<rect");
    expect(entry.brandColor.toLowerCase()).toBe("#317cff");
  });

  it("does not expose the retired Crof provider", () => {
    expect(PROVIDER_ICON_REGISTRY).not.toHaveProperty("crof");
  });

  it("uses the accents adopted by the upstream 0.70.0 palette audit", () => {
    for (const [id, color] of ADOPTED_ACCENTS) {
      expect(PROVIDER_ICON_REGISTRY[id]?.brandColor.toLowerCase(), id).toBe(color);
    }
  });

  it("uses the Mac 0.70.0 brand colors", () => {
    for (const [id, color] of MAC_PARITY_ACCENTS) {
      expect(PROVIDER_ICON_REGISTRY[id]?.brandColor.toLowerCase(), id).toBe(color);
    }
  });

  it("keeps Vercel on a mid neutral instead of Mac white", () => {
    // User decision 2026-10-10: Mac #FFFFFF is invisible on the light track.
    expect(PROVIDER_ICON_REGISTRY.vercel.brandColor.toLowerCase()).toBe("#737373");
  });

  it("paints ElevenLabs bars in the label color, as Mac does", () => {
    expect(providerBarAccent("elevenlabs")).toBe("var(--mac-text, rgba(0, 0, 0, 0.85))");
    expect(providerBarAccent("ElevenLabs", null)).toBe("var(--mac-text, rgba(0, 0, 0, 0.85))");
    // The icon keeps its brand color.
    expect(PROVIDER_ICON_REGISTRY.elevenlabs.brandColor.toLowerCase()).toBe("#111827");
  });

  it("uses the user's override, else the brand color, for card bars", () => {
    expect(providerBarAccent("claude")).toBe("#cc7c5e");
    expect(providerBarAccent("claude", "#123456")).toBe("#123456");
    expect(providerBarAccent("elevenlabs", "#ff0000")).toBe("#ff0000");
  });

  it("lifts only near-black brand colors on dark surfaces", () => {
    // Literal values: #111111 is 1.19:1 on #222222, #707070 is the first
    // 10% white mix that reaches 3:1.
    expect(brandColorOnDark("#111111")).toBe("#707070");
    expect(brandColorOnDark("#000000")).toBe("#808080");
    expect(brandColorOnDark("#111827")).toBe("#70747d");
    // Colors that already read on dark keep their exact value.
    expect(brandColorOnDark("#10a37f")).toBeNull();
    expect(brandColorOnDark("#2563eb")).toBeNull();
    expect(brandColorOnDark("#737373")).toBeNull();
    expect(brandColorOnDark("#4c00ff")).toBeNull();
    expect(brandColorOnDark("var(--mac-text)")).toBeNull();

    const lifted = Object.entries(PROVIDER_ICON_REGISTRY)
      .filter(([, icon]) => brandColorOnDark(icon.brandColor) !== null)
      .map(([id]) => id)
      .sort();
    expect(lifted).toEqual([
      "aixy",
      "deepinfra",
      "elevenlabs",
      "flora",
      "linkup",
      "manus",
      "parallel",
      "replicate",
      "synthetic",
      "typesafe",
      "v0",
    ]);
  });

  it("keeps the brand color in light scopes and the lifted tint in dark ones", () => {
    const css = loadStyles();
    const dark = css.indexOf('[data-theme="dark"] .provider-icon {');
    const light = css.indexOf('[data-theme="light"] .provider-icon {');
    expect(dark).toBeGreaterThan(-1);
    // The tray panel nests data-theme="light" inside a dark document, so the
    // light rule must come later to win at equal specificity.
    expect(light).toBeGreaterThan(dark);
    expect(ruleBlock(css, '[data-theme="dark"] .provider-icon')).toContain(
      "var(--provider-brand-dark, var(--provider-brand",
    );
    expect(ruleBlock(css, '[data-theme="light"] .provider-icon')).not.toContain(
      "--provider-brand-dark",
    );

    const floatbar = loadStylesheet("floatbar/FloatBar.css");
    expect(ruleBlock(floatbar, ".floatbar .floatbar__provider-icon .provider-icon")).not.toContain(
      "--provider-brand-dark",
    );
    expect(
      ruleBlock(
        floatbar,
        ".floatbar--floating:not(.floatbar--light-bg) .floatbar__provider-icon .provider-icon",
      ),
    ).toContain("var(--provider-brand-dark, var(--provider-brand");
  });

  it("matches the Rust brand_color table for every provider", () => {
    const rust = rustBrandColors();
    expect(rust.size).toBeGreaterThan(0);
    expect([...rust.keys()].sort()).toEqual(Object.keys(PROVIDER_ICON_REGISTRY).sort());
    for (const [id, color] of rust) {
      expect(PROVIDER_ICON_REGISTRY[id]?.brandColor.toLowerCase(), id).toBe(color);
    }
  });

  it("keeps provider chart tokens on the brand color", () => {
    const checked = chartTokens().filter(([id]) => PROVIDER_ICON_REGISTRY[id] !== undefined);
    expect(checked.map(([id]) => id)).toEqual(
      expect.arrayContaining([
        "amp",
        "augment",
        "codebuff",
        "cursor",
        "deepgram",
        "deepseek",
        "grok",
        "groq",
        "kiro",
        "llmproxy",
        "mistral",
        "windsurf",
        "xai",
      ]),
    );
    for (const [id, color] of checked) {
      expect(color, `--chart-${id}`).toBe(PROVIDER_ICON_REGISTRY[id].brandColor.toLowerCase());
    }
  });
});
