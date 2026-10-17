import { describe, expect, it } from "vitest";
import claudeRaw from "../../assets/brand-icons/Brand-ProviderIcon-claude.svg?raw";
import { brandArtworkFor, spendIconScale } from "./brandIcons";

describe("brandArtworkFor", () => {
  it.each([
    ["claude", "svg"],
    ["bedrock", "svg"],
    ["mistral", "svg"],
    ["muse", "svg"],
    ["vertexai", "svg"],
    ["codex", "image"],
    ["antigravity", "image"],
  ])("resolves original brand artwork for %s", (providerId, kind) => {
    const artwork = brandArtworkFor(providerId);
    expect(artwork?.kind).toBe(kind);
  });

  it("returns the bundled brand SVG untouched, without monochrome tinting", () => {
    const artwork = brandArtworkFor("claude");
    expect(artwork).toEqual({ kind: "svg", markup: claudeRaw });
  });

  it("resolves aliases to the curated provider", () => {
    expect(brandArtworkFor("Vertex AI")?.kind).toBe("svg");
    expect(brandArtworkFor("aws bedrock")?.kind).toBe("svg");
    expect(brandArtworkFor("Mistral AI")?.kind).toBe("svg");
  });

  it("returns null for providers without curated brand artwork", () => {
    for (const providerId of ["cursor", "gemini", "openaiapi", "azureopenai", "unknown-provider"]) {
      expect(brandArtworkFor(providerId), providerId).toBeNull();
    }
  });
});

describe("spendIconScale", () => {
  it("compensates transparent padding per brand artwork", () => {
    expect(spendIconScale("codex", true)).toBe(1.17);
    expect(spendIconScale("antigravity", true)).toBe(1.38);
    expect(spendIconScale("bedrock", true)).toBe(0.84);
    expect(spendIconScale("muse", true)).toBe(1.08);
    expect(spendIconScale("vertexai", true)).toBe(1.08);
  });

  it("uses the monochrome compensation for template artwork", () => {
    expect(spendIconScale("codex", false)).toBe(1.24);
    expect(spendIconScale("antigravity", false)).toBe(1.14);
    expect(spendIconScale("bedrock", false)).toBe(1);
    expect(spendIconScale("muse", false)).toBe(1);
  });

  it("scales Cursor's monochrome mark and leaves other providers at 1", () => {
    expect(spendIconScale("cursor", false)).toBe(1.25);
    expect(spendIconScale("claude", true)).toBe(1);
    expect(spendIconScale("unknown-provider", false)).toBe(1);
  });
});
