import { describe, expect, it } from "vitest";
import type {
  ModelRateLimitSnapshot,
  ProviderThroughputSnapshot,
  ThroughputModelSnapshot,
} from "../types/bridge";
import {
  draftsFromLimits,
  hasReportedTraffic,
  type LimitDraft,
  MAX_RATE_LIMIT,
  parseLimitDrafts,
  throughputLimitLine,
  tpmBars,
} from "./openaiThroughput";

const START = 1_788_220_800;

function model(name: string, overrides: Partial<ThroughputModelSnapshot> = {}): ThroughputModelSnapshot {
  return {
    name,
    latestTpm: 0,
    peakTpm: 0,
    totalTokens: 0,
    peakLimitCountedTokens: 0,
    limitCountedTokensByMinute: [],
    ...overrides,
  };
}

/** Three reported minutes, then two pending minutes after the frontier. */
function throughput(overrides: Partial<ProviderThroughputSnapshot> = {}): ProviderThroughputSnapshot {
  return {
    minutes: [
      { start: START, totalTokens: 1_000, requests: 1, isPending: false },
      { start: START + 60, totalTokens: 4_000, requests: 2, isPending: false },
      { start: START + 120, totalTokens: 2_000, requests: 1, isPending: false },
      { start: START + 180, totalTokens: 0, requests: 0, isPending: true },
      { start: START + 240, totalTokens: 0, requests: 0, isPending: true },
    ],
    frontier: START + 120,
    latestTokensPerMinute: 2_000,
    peakTokensPerMinute: 4_000,
    peakAt: START + 60,
    fiveMinuteAverageTokensPerMinute: 2_333,
    models: [],
    windowMinutes: 60,
    fetchedAt: START + 300,
    projectId: null,
    ...overrides,
  };
}

describe("tpmBars", () => {
  it("keeps the regular grid order and flags pending minutes", () => {
    const bars = tpmBars(throughput());
    expect(bars.map((bar) => bar.tokens)).toEqual([1_000, 4_000, 2_000, 0, 0]);
    expect(bars.map((bar) => bar.isPending)).toEqual([false, false, false, true, true]);
  });
});

describe("hasReportedTraffic", () => {
  it("ignores pending minutes", () => {
    const pendingOnly = throughput({
      minutes: [{ start: START, totalTokens: 0, requests: 0, isPending: true }],
    });
    expect(hasReportedTraffic(pendingOnly)).toBe(false);
    expect(hasReportedTraffic(throughput())).toBe(true);
  });
});

describe("throughputLimitLine", () => {
  it("picks the busiest model that has a manual limit", () => {
    const snapshot = throughput({
      models: [
        model("gpt-5", { peakLimitCountedTokens: 3_000 }),
        model("gpt-4o", { peakLimitCountedTokens: 9_000 }),
        model("unlimited", { peakLimitCountedTokens: 50_000 }),
      ],
    });
    const limits: ModelRateLimitSnapshot[] = [
      { model: "gpt-5", tokensPerMinute: 10_000 },
      { model: "gpt-4o", tokensPerMinute: 12_000 },
    ];
    expect(throughputLimitLine(snapshot, limits)).toEqual({
      model: "gpt-4o",
      peakTokens: 9_000,
      tokensPerMinuteLimit: 12_000,
      percent: 75,
    });
  });

  it("returns null without a matching limit", () => {
    const snapshot = throughput({ models: [model("gpt-5", { peakLimitCountedTokens: 1 })] });
    expect(throughputLimitLine(snapshot, [])).toBeNull();
    expect(throughputLimitLine(snapshot, [{ model: "other", tokensPerMinute: 10 }])).toBeNull();
  });

  it("never applies organization limits to a project-scoped snapshot", () => {
    const scoped = throughput({
      projectId: "proj_1",
      models: [model("gpt-5", { peakLimitCountedTokens: 9_000 })],
    });
    expect(throughputLimitLine(scoped, [{ model: "gpt-5", tokensPerMinute: 10_000 }])).toBeNull();
  });
});

describe("parseLimitDrafts", () => {
  const row = (
    model: string,
    tokensPerMinute: string,
    requestsPerMinute = "",
  ): LimitDraft => ({ model, tokensPerMinute, requestsPerMinute });

  it("trims rows, maps an empty or zero requests field to no limit, and keeps order", () => {
    const result = parseLimitDrafts([
      row("  gpt-5 ", "40000000", "0"),
      row("gpt-4o", " 2000000 ", " 5000 "),
      row("o3", "1"),
    ]);
    expect(result).toEqual({
      ok: true,
      limits: [
        { model: "gpt-5", tokensPerMinute: 40_000_000, requestsPerMinute: null },
        { model: "gpt-4o", tokensPerMinute: 2_000_000, requestsPerMinute: 5_000 },
        { model: "o3", tokensPerMinute: 1, requestsPerMinute: null },
      ],
    });
  });

  it("reports a bad row by its error key and index instead of dropping it", () => {
    expect(parseLimitDrafts([row("", "1")])).toEqual({
      ok: false,
      error: "OpenAiLimitErrorModel",
      index: 0,
    });
    expect(parseLimitDrafts([row("gpt-5", "1"), row("gpt-5", "2")])).toEqual({
      ok: false,
      error: "OpenAiLimitErrorDuplicate",
      index: 1,
    });
    expect(parseLimitDrafts([row("gpt-5", "0")])).toMatchObject({ error: "OpenAiLimitErrorTpm" });
    expect(parseLimitDrafts([row("gpt-5", "1.5")])).toMatchObject({ error: "OpenAiLimitErrorTpm" });
    expect(parseLimitDrafts([row("gpt-5", "abc")])).toMatchObject({ error: "OpenAiLimitErrorTpm" });
    expect(parseLimitDrafts([row("gpt-5", String(MAX_RATE_LIMIT + 1))])).toMatchObject({
      error: "OpenAiLimitErrorTpm",
    });
    expect(parseLimitDrafts([row("gpt-5", "1", "x")])).toMatchObject({ error: "OpenAiLimitErrorRpm" });
    expect(parseLimitDrafts([row("gpt-\n5", "1")])).toMatchObject({ error: "OpenAiLimitErrorLine" });
    expect(parseLimitDrafts([row("m".repeat(201), "1")])).toMatchObject({
      error: "OpenAiLimitErrorLine",
    });
    // Counted in code points, like the shell: 200 astral characters still fit.
    expect(parseLimitDrafts([row("😀".repeat(200), "1")])).toMatchObject({ ok: true });
  });

  it("accepts an empty list, which clears the stored limits", () => {
    expect(parseLimitDrafts([])).toEqual({ ok: true, limits: [] });
  });

  it("round-trips saved limits through the editor drafts", () => {
    const saved: ModelRateLimitSnapshot[] = [
      { model: "gpt-5", tokensPerMinute: 40_000_000, requestsPerMinute: null },
      { model: "gpt-4o", tokensPerMinute: 2_000_000, requestsPerMinute: 5_000 },
    ];
    expect(parseLimitDrafts(draftsFromLimits(saved))).toEqual({ ok: true, limits: saved });
  });
});
