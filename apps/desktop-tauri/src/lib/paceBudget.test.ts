import { describe, expect, it } from "vitest";
import type { RateWindowSnapshot } from "../types/bridge";
import { getPaceBudget, getPaceChartSnapshot } from "./paceBudget";

function snapshot(
  overrides: Partial<RateWindowSnapshot> = {},
): RateWindowSnapshot {
  return {
    usedPercent: 20,
    remainingPercent: 80,
    windowMinutes: 10 * 60,
    resetsAt: "2026-06-12T10:00:00.000Z",
    resetDescription: null,
    isExhausted: false,
    reservePercent: 20,
    reserveDescription: "Lasts until reset",
    ...overrides,
  };
}

describe("getPaceBudget", () => {
  it("returns additional usage allowed at each horizon", () => {
    const budget = getPaceBudget(
      snapshot(),
      new Date("2026-06-12T04:00:00.000Z"),
    );

    expect(budget).toEqual({
      now: 20,
      nextHour: 30,
      nextFiveHours: 70,
      today: 80,
    });
  });

  it("never recommends negative usage when actual usage is ahead of pace", () => {
    const budget = getPaceBudget(
      snapshot({ usedPercent: 65, remainingPercent: 35 }),
      new Date("2026-06-12T04:00:00.000Z"),
    );

    expect(budget).toMatchObject({ now: 0, nextHour: 0 });
  });

  it("caps horizons at reset and budgets at remaining quota", () => {
    const budget = getPaceBudget(
      snapshot({
        usedPercent: 10,
        remainingPercent: 12,
        resetsAt: "2026-06-12T05:00:00.000Z",
      }),
      new Date("2026-06-12T04:30:00.000Z"),
    );

    expect(budget).toEqual({
      now: 12,
      nextHour: 12,
      nextFiveHours: 12,
      today: 12,
    });
  });

  it("returns no budget for incomplete, expired, or exhausted windows", () => {
    const now = new Date("2026-06-12T04:00:00.000Z");

    expect(getPaceBudget(snapshot({ resetsAt: null }), now)).toBeNull();
    expect(getPaceBudget(snapshot({ windowMinutes: null }), now)).toBeNull();
    expect(
      getPaceBudget(snapshot({ resetsAt: "2026-06-12T03:00:00.000Z" }), now),
    ).toBeNull();
    expect(getPaceBudget(snapshot({ isExhausted: true }), now)).toBeNull();
  });
});

describe("getPaceChartSnapshot", () => {
  it("projects the current average burn rate to reset", () => {
    const chart = getPaceChartSnapshot(
      snapshot({ usedPercent: 30 }),
      new Date("2026-06-12T04:00:00.000Z"),
    );

    expect(chart).toEqual({
      elapsedPercent: 40,
      usedPercent: 30,
      projectedPercent: 75,
    });
  });

  it("projects ahead-of-pace usage above the ideal reset endpoint", () => {
    const chart = getPaceChartSnapshot(
      snapshot({ usedPercent: 50 }),
      new Date("2026-06-12T04:00:00.000Z"),
    );

    expect(chart).toEqual({
      elapsedPercent: 40,
      usedPercent: 50,
      projectedPercent: 100,
    });
  });

  it("clips over-quota usage to the visible chart bounds", () => {
    const chart = getPaceChartSnapshot(
      snapshot({ usedPercent: 115 }),
      new Date("2026-06-12T04:00:00.000Z"),
    );

    expect(chart).toEqual({
      elapsedPercent: 40,
      usedPercent: 100,
      projectedPercent: 100,
    });
  });

  it("returns no chart without a current, valid rate window", () => {
    const now = new Date("2026-06-12T04:00:00.000Z");

    expect(getPaceChartSnapshot(snapshot({ resetsAt: null }), now)).toBeNull();
    expect(getPaceChartSnapshot(snapshot({ windowMinutes: null }), now)).toBeNull();
    expect(
      getPaceChartSnapshot(
        snapshot({ resetsAt: "2026-06-12T03:00:00.000Z" }),
        now,
      ),
    ).toBeNull();
  });
});
