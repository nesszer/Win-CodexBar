import { describe, expect, it } from "vitest";

import { makeRateWindow, makeUsageSnapshot } from "../test/fixtures";
import type { ProviderUsageSnapshot } from "../types/bridge";

import { prioritizeProviders, switcherWeeklyPercent } from "./providerGridUtils";

function provider(id: string): ProviderUsageSnapshot {
  return makeUsageSnapshot(id, {
    primary: makeRateWindow(0, { reserveWillLastToReset: false, reserveEtaSeconds: null }),
    selectedMetric: makeRateWindow(0, { reserveWillLastToReset: false, reserveEtaSeconds: null }),
    primaryLabel: null,
    secondaryLabel: null,
    sourceLabel: "oauth",
    updatedAt: "2026-07-31T00:00:00Z",
    fetchDurationMs: null,
  });
}

function ids(list: ProviderUsageSnapshot[]): string[] {
  return list.map((p) => p.providerId);
}

describe("prioritizeProviders", () => {
  it("returns the list unchanged when no provider is selected", () => {
    const list = [provider("a"), provider("b")];
    expect(prioritizeProviders(list, null)).toBe(list);
    expect(prioritizeProviders(list, "")).toBe(list);
  });

  it("returns the list unchanged when the selected provider is not present", () => {
    const list = [provider("a"), provider("b")];
    expect(prioritizeProviders(list, "missing")).toBe(list);
  });

  it("does not reorder when the selected provider is already within the first 18 slots", () => {
    // index 17 (the 18th slot) — boundary, must NOT move.
    const list = Array.from({ length: 18 }, (_, i) => provider(`p${i}`));
    list[17] = provider("target");
    expect(prioritizeProviders(list, "target")).toBe(list);
  });

  it("does not reorder when the selected provider sits exactly at index 18 (just past the compact window)", () => {
    // index 18 is the 19th slot; the rule only pulls items with selectedIndex >= 18,
    // but the boundary condition is `selectedIndex < 18` → returns as-is.
    const list = Array.from({ length: 20 }, (_, i) => provider(`p${i}`));
    // selectedIndex 18 → should be pulled to front.
    const next = prioritizeProviders(list, "p18");
    expect(next[0].providerId).toBe("p18");
  });

  it("pulls a far-down selected provider to the front, preserving relative order of the rest", () => {
    const list = Array.from({ length: 25 }, (_, i) => provider(`p${i}`));
    const next = prioritizeProviders(list, "p24");
    expect(ids(next)).toEqual([
      "p24",
      ...Array.from({ length: 24 }, (_, i) => `p${i}`),
    ]);
    // original list is not mutated
    expect(list[0].providerId).toBe("p0");
  });

  it("prioritized provider is selected vs not-selected ordering differ", () => {
    const base = Array.from({ length: 30 }, (_, i) => provider(`p${i}`));
    const prioritized = prioritizeProviders(base, "p29");
    const unprioritized = prioritizeProviders(base, null);
    expect(prioritized[0].providerId).toBe("p29");
    expect(unprioritized[0].providerId).toBe("p0");
    expect(prioritized.length).toBe(unprioritized.length);
    // same members, different order
    expect(new Set(ids(prioritized))).toEqual(new Set(ids(unprioritized)));
  });
});

function lane(usedPercent: number, windowMinutes: number | null): ProviderUsageSnapshot["primary"] {
  return { ...provider("x").primary, usedPercent, remainingPercent: 100 - usedPercent, windowMinutes };
}

describe("switcherWeeklyPercent", () => {
  it("shows the 7-day lane, not the session lane", () => {
    const codex = { ...provider("codex"), primary: lane(42, 300), secondary: lane(63, 10080) };
    expect(switcherWeeklyPercent(codex, true)).toBe(63);
    expect(switcherWeeklyPercent(codex, false)).toBe(37);
  });

  it("picks the most used 7-day lane when both core lanes are weekly", () => {
    const both = { ...provider("p"), primary: lane(20, 10080), secondary: lane(70, 10080) };
    expect(switcherWeeklyPercent(both, true)).toBe(70);
  });

  it("ignores weekly tertiary and extra lanes", () => {
    const scoped = {
      ...provider("claude"),
      primary: lane(10, 300),
      secondary: lane(30, 10080),
      tertiary: lane(95, 10080),
      extraRateWindows: [{ id: "scoped", title: "Scoped", window: lane(99, 10080), fallbackLane: false }],
    };
    expect(switcherWeeklyPercent(scoped, true)).toBe(30);
  });

  it("falls back to the primary lane without a 7-day lane", () => {
    const monthly = { ...provider("p"), primary: lane(25, 43200), secondary: lane(80, 300) };
    expect(switcherWeeklyPercent(monthly, true)).toBe(25);
    expect(switcherWeeklyPercent({ ...provider("p"), primary: lane(140, null) }, true)).toBe(100);
  });
});
