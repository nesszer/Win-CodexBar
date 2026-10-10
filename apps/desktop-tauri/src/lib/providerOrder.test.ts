import { describe, expect, it } from "vitest";
import { orderProviderSnapshots } from "./providerOrder";
import { makeUsageSnapshot } from "../test/fixtures";
import type { ProviderCatalogEntry, ProviderUsageSnapshot } from "../types/bridge";

const catalog: ProviderCatalogEntry[] = [
  { id: "codex", displayName: "Codex", cookieDomain: null },
  { id: "claude", displayName: "Claude", cookieDomain: null },
  { id: "gemini", displayName: "Gemini", cookieDomain: null },
];

function snapshot(providerId: string, displayName: string): ProviderUsageSnapshot {
  return makeUsageSnapshot(providerId, {
    displayName,
    sourceLabel: "test",
    updatedAt: "2026-01-01T00:00:00Z",
  });
}

describe("orderProviderSnapshots", () => {
  it("uses persisted provider order before catalog order", () => {
    const ordered = orderProviderSnapshots(
      [
        snapshot("codex", "Codex"),
        snapshot("claude", "Claude"),
        snapshot("gemini", "Gemini"),
      ],
      catalog,
      ["codex", "claude", "gemini"],
      ["gemini", "claude", "codex"],
    );

    expect(ordered.map((provider) => provider.providerId)).toEqual([
      "gemini",
      "claude",
      "codex",
    ]);
  });
});
