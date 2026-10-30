import { describe, expect, it } from "vitest";
import type { CodexWorkspacesSessionUsage } from "../types/bridge";
import { formatSessionCost, inspectSessions, sessionCost, sessionMatches } from "./sessionInspector";

function session(
  id: string,
  overrides: Partial<CodexWorkspacesSessionUsage> = {},
): CodexWorkspacesSessionUsage {
  return {
    id,
    projectId: "p",
    displayTitle: `Title ${id}`,
    cwd: null,
    startedAt: null,
    latestActivity: null,
    totals: { inputTokens: 0, cachedInputTokens: 0, outputTokens: 0, totalTokens: 0 },
    costEstimate: { knownUsd: 0, unknownTokens: 0 },
    topModel: null,
    turnPerformance: null,
    ...overrides,
  };
}

const ids = (rows: CodexWorkspacesSessionUsage[]) => rows.map((row) => row.id);

describe("sessionInspector", () => {
  it("matches the shown title or the id, ignoring case and surrounding spaces", () => {
    const row = session("abc-123", { displayTitle: "Port the Inspector" });
    expect(sessionMatches(row, "  inspector ")).toBe(true);
    expect(sessionMatches(row, "ABC")).toBe(true);
    expect(sessionMatches(row, "missing")).toBe(false);
    expect(sessionMatches(row, "   ")).toBe(true);
  });

  it("does not find a hidden title through search", () => {
    const hidden = session("s-1", { displayTitle: "Hidden conversation" });
    expect(sessionMatches(hidden, "secret project")).toBe(false);
  });

  it("orders by latest activity, newest first by default, unknown last", () => {
    const rows = [
      session("a", { latestActivity: "2026-01-01T10:00:00Z" }),
      session("b", { latestActivity: null }),
      session("c", { latestActivity: "2026-01-02T10:00:00Z" }),
    ];
    expect(ids(inspectSessions(rows, ""))).toEqual(["c", "a", "b"]);
    expect(ids(inspectSessions(rows, "", "activity", true))).toEqual(["a", "c", "b"]);
  });

  it("orders by tokens and cost, breaking ties by latest activity then id", () => {
    const totals = (totalTokens: number) => ({ inputTokens: 0, cachedInputTokens: 0, outputTokens: 0, totalTokens });
    const rows = [
      session("b", { totals: totals(10), latestActivity: "2026-01-01T00:00:00Z" }),
      session("a", { totals: totals(10), latestActivity: "2026-01-01T00:00:00Z" }),
      session("c", { totals: totals(10), latestActivity: "2026-01-03T00:00:00Z" }),
      session("d", { totals: totals(50) }),
    ];
    expect(ids(inspectSessions(rows, "", "tokens"))).toEqual(["d", "c", "a", "b"]);
    expect(ids(inspectSessions(rows, "", "tokens", true))).toEqual(["c", "a", "b", "d"]);

    const priced = [
      session("x", { costEstimate: { knownUsd: 0, unknownTokens: 40 } }),
      session("y", { costEstimate: { knownUsd: 0.2, unknownTokens: 0 } }),
      session("z", { costEstimate: { knownUsd: 1.5, unknownTokens: 3 } }),
    ];
    expect(ids(inspectSessions(priced, "", "cost"))).toEqual(["z", "y", "x"]);
    expect(ids(inspectSessions(priced, "", "cost", true))).toEqual(["y", "z", "x"]);
  });

  it("treats a session with only unpriced usage as unknown cost", () => {
    expect(sessionCost(session("a", { costEstimate: { knownUsd: 0, unknownTokens: 5 } }))).toBeNull();
    expect(sessionCost(session("b", { costEstimate: { knownUsd: 0, unknownTokens: 0 } }))).toBe(0);
    expect(sessionCost(session("c", { costEstimate: { knownUsd: 0.3, unknownTokens: 5 } }))).toBe(0.3);
  });

  it("keeps up to six decimals below one dollar", () => {
    expect(formatSessionCost(null)).toBe("—");
    expect(formatSessionCost(0)).toBe("$0.00");
    expect(formatSessionCost(12.345)).toBe("$12.35");
    expect(formatSessionCost(0.4213)).toBe("$0.4213");
    expect(formatSessionCost(0.5)).toBe("$0.50");
    expect(formatSessionCost(0.0000123)).toBe("$0.000012");
    expect(formatSessionCost(0.0000001)).toBe("<$0.000001");
  });
});
