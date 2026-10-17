import { describe, expect, it } from "vitest";
import type { SpendContract, UsageSpendRow, UsageSpendSummary } from "../types/bridge";
import {
  formatCompactTokens,
  normalizeHiddenSourceIds,
  spendSources,
  spendTotalsByCurrency,
  toggleHiddenSource,
  visibleUsageSpendSummary,
} from "./usageSpendDashboard";

function row(providerId: string, periodCost: number | null, periodTokens: number | null, currency = "USD"): UsageSpendRow {
  return {
    providerId,
    displayName: providerId[0].toUpperCase() + providerId.slice(1),
    sevenDay: null,
    thirtyDay: null,
    periodCost,
    periodTokens,
    currency,
    source: "local",
    includedInOverview: true,
  };
}

function codexContract(): SpendContract {
  return {
    providerId: "codex",
    historyDays: 30,
    reportingPeriod: "rolling:30",
    knownCostUsd: 12,
    knownZero: false,
    provenance: "listPriceEstimate",
    priceCoverage: { priced: 4, unpriced: 0, unmetered: 0, estimated: 0 },
    priceCoverageRatio: 1,
    historyCoverageEstablished: true,
    tokenMix: { inputTokens: 100, outputTokens: 20, cacheReadTokens: 50, cacheCreationTokens: null, reasoningTokens: 5 },
    conversationCount: 2,
    models: [{ model: "gpt-5", costUsd: 12, inputTokens: 100, outputTokens: 20, cacheReadTokens: 50, totalTokens: 170, customPricing: false }],
    projects: [],
    conversations: [],
    daily: [],
    hourlyActivity: [],
    projectSourceStatus: null,
    customPricingActive: false,
    imports: [],
  } as SpendContract;
}

function summary(rows: UsageSpendRow[]): UsageSpendSummary {
  return { rows, contract: codexContract(), reportingPeriod: "rolling:30", reportingDay: "2026-10-11", dashboardTimezone: "UTC" };
}

describe("spendTotalsByCurrency", () => {
  it("sums cost, tokens and sources per currency with USD first", () => {
    const totals = spendTotalsByCurrency([
      row("codex", 25.42, 18_000_000),
      row("claude", 1.99, 695_000),
      row("zai", 3, 100, "cny"),
      row("cursor", null, 1_600_000),
    ]);
    expect(totals.map((entry) => entry.currency)).toEqual(["USD", "CNY"]);
    expect(totals[0].cost).toBeCloseTo(27.41, 5);
    expect(totals[0].tokens).toBe(20_295_000);
    expect(totals[0].sources).toBe(3);
    expect(totals[1]).toEqual({ currency: "CNY", cost: 3, tokens: 100, sources: 1 });
  });

  it("keeps an unknown total unknown instead of zero", () => {
    const [usd] = spendTotalsByCurrency([row("cursor", null, null)]);
    expect(usd).toEqual({ currency: "USD", cost: null, tokens: null, sources: 1 });
  });
});

describe("hidden sources", () => {
  it("normalizes ids the way the backend stores them", () => {
    expect(normalizeHiddenSourceIds([" Pi ", "claude", "", "CLAUDE"])).toEqual(["claude", "pi"]);
    expect(toggleHiddenSource(["pi"], "Claude", false)).toEqual(["claude", "pi"]);
    expect(toggleHiddenSource(["claude", "pi"], "claude", true)).toEqual(["pi"]);
  });

  it("drops hidden rows and blanks the detail contract when its provider is hidden", () => {
    const data = summary([row("codex", 25, 18), row("claude", 2, 7)]);

    const noClaude = visibleUsageSpendSummary(data, ["claude"]);
    expect(noClaude.rows.map((entry) => entry.providerId)).toEqual(["codex"]);
    expect(noClaude.contract.models).toHaveLength(1);

    const noCodex = visibleUsageSpendSummary(data, ["codex"]);
    expect(noCodex.rows.map((entry) => entry.providerId)).toEqual(["claude"]);
    expect(noCodex.contract.providerId).toBe("codex");
    expect(noCodex.contract.models).toEqual([]);
    expect(noCodex.contract.knownCostUsd).toBeNull();
    expect(noCodex.contract.tokenMix.inputTokens).toBeNull();
    expect(noCodex.contract.reportingPeriod).toBe("rolling:30");
  });

  it("drops hidden sources from the trend series", () => {
    const data = {
      ...summary([row("codex", 1, 1), row("claude", 1, 1)]),
      trend: {
        startDay: "2026-01-01",
        endDay: "2026-01-02",
        timeZone: "UTC",
        sources: ["claude", "codex"].map((id) => ({ id, providerId: id, displayName: id, knownZero: false })),
        daily: [
          { sourceId: "claude", day: "2026-01-01", costUsd: 1 },
          { sourceId: "codex", day: "2026-01-01", costUsd: 2 },
        ],
        hourly: [{ sourceId: "claude", hour: "2026-01-01T10:00:00Z", costUsd: 1 }],
        hourlyDays: [],
      },
    };
    const visible = visibleUsageSpendSummary(data, ["Claude"]);
    expect(visible.trend?.sources.map((source) => source.id)).toEqual(["codex"]);
    expect(visible.trend?.daily.map((point) => point.sourceId)).toEqual(["codex"]);
    expect(visible.trend?.hourly).toEqual([]);
  });

  it("lists every returned source plus hidden ones without a row", () => {
    const data = summary([row("codex", 1, 1), row("claude", 1, 1)]);
    expect(spendSources(data, ["claude", "pi"]).map((source) => source.id)).toEqual(["claude", "codex", "pi"]);
  });
});

describe("formatCompactTokens", () => {
  it("abbreviates large counts and marks unknown", () => {
    expect(formatCompactTokens(20_295_000)).toBe("20.3M");
    expect(formatCompactTokens(106_000)).toBe("106K");
    expect(formatCompactTokens(null)).toBe("—");
  });
});
