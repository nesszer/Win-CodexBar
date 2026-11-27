import { describe, expect, it } from "vitest";
import { fromFtl } from "../test/localeHarness";
import type { RateWindowSnapshot, WindowPaceSnapshot } from "../types/bridge";
import {
  metricResetText,
  metricRowPresentation,
  type MetricRowInput,
} from "./metricRowModel";

const en = fromFtl("en-US.ftl");
const NOW = new Date(2026, 9, 10, 15, 40).getTime();
const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

function rateWindow(overrides: Partial<RateWindowSnapshot> = {}): RateWindowSnapshot {
  return {
    usedPercent: 37,
    remainingPercent: 63,
    windowMinutes: 300,
    resetsAt: null,
    resetDescription: null,
    isExhausted: false,
    reservePercent: null,
    reserveDescription: null,
    isInformational: false,
    descriptionIsDetail: false,
    reserveWillLastToReset: false,
    reserveEtaSeconds: null,
    pace: null,
    monthlyLimitBlock: null,
    ...overrides,
  };
}

function pace(overrides: Partial<WindowPaceSnapshot> = {}): WindowPaceSnapshot {
  return {
    stage: "behind",
    deltaPercent: -23.4,
    expectedUsedPercent: 60,
    actualUsedPercent: 36.6,
    etaSeconds: null,
    willLastToReset: true,
    ...overrides,
  };
}

function input(overrides: Partial<MetricRowInput> = {}): MetricRowInput {
  return {
    snap: rateWindow(),
    lane: "primary",
    providerId: "claude",
    resetText: null,
    compact: false,
    showAsUsed: false,
    showResetWhenExhausted: false,
    paceEnabled: true,
    usageThresholds: { highUsageThreshold: 70, criticalUsageThreshold: 90, providerQuotaWarnings: {} },
    weeklyProgressWorkDays: null,
    quotaWarningMarkersVisible: true,
    workdayTickStyle: "subtle",
    now: NOW,
    ...overrides,
  };
}

function thresholds(highUsageThreshold: number, criticalUsageThreshold = 90) {
  return { highUsageThreshold, criticalUsageThreshold };
}

function weekly(overrides: Partial<RateWindowSnapshot> = {}): RateWindowSnapshot {
  return rateWindow({ usedPercent: 36.6, remainingPercent: 63.4, windowMinutes: 10080, pace: pace(), ...overrides });
}

function resetIn(ms: number, relative = true) {
  const snap = rateWindow({ resetsAt: new Date(NOW + ms).toISOString() });
  return metricResetText(snap, relative, NOW, en)?.replace(/\u202f/g, " ");
}

describe("metricResetText", () => {
  it.each([
    [2 * HOUR, "Resets in 2h"],
    [HOUR + 59 * MINUTE + 30_000, "Resets in 2h"],
    [2 * HOUR + 5 * MINUTE, "Resets in 2h 5m"],
    [DAY + 3 * HOUR, "Resets in 1d 3h"],
    [DAY + 5 * MINUTE, "Resets in 1d 5m"],
    [2 * DAY, "Resets in 2d"],
    [45_000, "Resets in 1m"],
    [500, "Resets now"],
    [-MINUTE, "Resets now"],
  ])("counts %d ms down to %j with ceiling minutes", (ms, expected) => {
    expect(resetIn(ms)).toBe(expected);
  });

  it.each([
    [new Date(2026, 9, 10, 18, 5), "Resets 6:05 PM"],
    [new Date(2026, 9, 11, 9, 24), "Resets tomorrow, 9:24 AM"],
    [new Date(2026, 9, 14, 9, 24), "Resets Oct 14, 9:24 AM"],
  ])("formats the absolute reset %s as today, tomorrow or a date", (date, expected) => {
    expect(resetIn(date.getTime() - NOW, false)).toBe(expected);
  });

  it("falls back to the provider reset description without a timestamp", () => {
    expect(metricResetText(rateWindow({ resetDescription: "resets in 3h" }), true, NOW, en)).toBe(
      "Resets in 3h",
    );
    expect(
      metricResetText(
        rateWindow({ resetDescription: "750 / 1000 credits left", descriptionIsDetail: true }),
        true,
        NOW,
        en,
      ),
    ).toBeNull();
    expect(metricResetText(rateWindow({ resetsAt: "not a date" }), true, NOW, en)).toBeNull();
  });

  // Upstream `syntheticRegenDetail`: "Regenerates %@" around the unlocalized
  // `resetCountdownDescription`, relative even under the absolute reset style.
  it.each([
    [45 * MINUTE, true, "Regenerates in 45m"],
    [45 * MINUTE, false, "Regenerates in 45m"],
    [2 * HOUR + 5 * MINUTE, true, "Regenerates in 2h 5m"],
    [DAY + 3 * HOUR, true, "Regenerates in 1d 3h"],
    [2 * DAY + 3 * HOUR, true, "Regenerates in 2d 3h"],
    [2 * DAY, true, "Regenerates in 2d"],
    [500, true, "Regenerates now"],
  ])("counts a regen lane %d ms down to %j (relative %s)", (ms, relative, expected) => {
    const snap = rateWindow({
      resetsAt: new Date(NOW + ms).toISOString(),
      resetDescription: "76% after next regen · Full in ~3 regens",
      descriptionIsDetail: true,
      resetRegenerates: true,
    });
    expect(metricResetText(snap, relative, NOW, en)).toBe(expected);
  });

  // The countdown is a bare localized duration; each template supplies its own
  // preposition, so no locale inherits the English "in " prefix.
  it.each([
    ["uk-UA.ftl", 45 * MINUTE, "Регенерує через 45 хв"],
    ["uk-UA.ftl", 2 * DAY + 3 * HOUR, "Регенерує через 2 дн. 3 год"],
    ["uk-UA.ftl", 500, "Регенерує зараз"],
    ["ja-JP.ftl", 45 * MINUTE, "45 分後に再生成"],
  ])("localizes the regen countdown in %s for %d ms", (file, ms, expected) => {
    const snap = rateWindow({
      resetsAt: new Date(NOW + ms).toISOString(),
      resetRegenerates: true,
    });
    expect(metricResetText(snap, true, NOW, fromFtl(file))?.replace(/\u202f/g, " ")).toBe(expected);
  });
});

describe("metricRowPresentation percent and reset", () => {
  it.each([
    [37, false, "63% left"],
    [37, true, "37% used"],
    [37.5, false, "62% left"],
    [36.5, false, "64% left"],
    [99.6, false, "<1% left"],
    [0.4, true, "<1% used"],
    [115, true, "100% used"],
    [115, false, "0% left"],
    [-4, false, "100% left"],
  ])("labels %d%% used with showAsUsed %s as %j", (used, showAsUsed, expected) => {
    const row = metricRowPresentation(
      input({ snap: rateWindow({ usedPercent: used, remainingPercent: 100 - used }), showAsUsed }),
      en,
    );
    expect(row.percentText).toBe(expected);
  });

  it("keeps the reset text beside the title and drops it in compact rows", () => {
    expect(metricRowPresentation(input({ resetText: "Resets in 2h" }), en).resetText).toBe("Resets in 2h");
    expect(
      metricRowPresentation(input({ resetText: "Resets in 2h", compact: true }), en).resetText,
    ).toBeNull();
  });

  it("replaces an exhausted percentage with a future reset, even in compact rows", () => {
    const snap = rateWindow({
      usedPercent: 100,
      remainingPercent: 0,
      isExhausted: true,
      resetsAt: new Date(NOW + HOUR).toISOString(),
    });
    const row = metricRowPresentation(
      input({ snap, resetText: "Resets in 1h", showResetWhenExhausted: true, compact: true }),
      en,
    );
    expect(row.percentText).toBeNull();
    expect(row.resetText).toBe("Resets in 1h");
  });

  it("keeps the exhausted percentage when the reset already passed", () => {
    const snap = rateWindow({
      usedPercent: 100,
      remainingPercent: 0,
      isExhausted: true,
      resetsAt: new Date(NOW - HOUR).toISOString(),
    });
    const row = metricRowPresentation(
      input({ snap, resetText: "Resets now", showResetWhenExhausted: true }),
      en,
    );
    expect(row.percentText).toBe("0% left");
  });
});

describe("metricRowPresentation pace", () => {
  it("shows reserve and the lasts-until-reset note on a weekly window", () => {
    const row = metricRowPresentation(input({ lane: "secondary", snap: weekly() }), en);
    expect(row.metaText).toBe("23% in reserve · Lasts until reset");
    expect(row.bar.pacePercent).toBe(40);
    expect(row.bar.paceDeficit).toBe(false);
  });

  it("shows a weekly deficit with its run-out countdown", () => {
    const snap = weekly({
      usedPercent: 52.6,
      remainingPercent: 47.4,
      pace: pace({
        stage: "ahead",
        deltaPercent: 12.6,
        expectedUsedPercent: 40,
        actualUsedPercent: 52.6,
        etaSeconds: 2 * 86_400 + 5 * 3600,
        willLastToReset: false,
      }),
    });
    const row = metricRowPresentation(input({ lane: "secondary", snap, showAsUsed: true }), en);
    expect(row.metaText).toBe("13% in deficit · Runs out in 2d 5h");
    expect(row.bar.pacePercent).toBe(40);
    expect(row.bar.paceDeficit).toBe(true);
  });

  it("projects a session window empty instead of running out", () => {
    const snap = rateWindow({
      usedPercent: 54,
      remainingPercent: 46,
      pace: pace({
        stage: "slightly_ahead",
        deltaPercent: 4,
        expectedUsedPercent: 50,
        actualUsedPercent: 54,
        etaSeconds: 5400,
        willLastToReset: false,
      }),
    });
    expect(metricRowPresentation(input({ snap }), en).metaText).toBe(
      "4% in deficit · Projected empty in 1h 30m",
    );
    const now = { ...snap, pace: { ...snap.pace!, etaSeconds: 0 } };
    expect(metricRowPresentation(input({ snap: now }), en).metaText).toBe(
      "4% in deficit · Projected empty now",
    );
  });

  it("calls a rounded zero delta or an on-track stage on pace without a pace marker", () => {
    const tiny = metricRowPresentation(
      input({ lane: "secondary", snap: weekly({ pace: pace({ deltaPercent: -0.4 }) }) }),
      en,
    );
    expect(tiny.metaText).toBe("On pace · Lasts until reset");
    const onTrack = metricRowPresentation(
      input({
        lane: "secondary",
        snap: weekly({ pace: pace({ stage: "on_track", deltaPercent: 3, willLastToReset: false }) }),
      }),
      en,
    );
    expect(onTrack.metaText).toBe("On pace");
    expect(onTrack.bar.pacePercent).toBeNull();
  });

  it("runs a weekly window out now even below the 3% expected floor", () => {
    const snap = weekly({
      pace: pace({ stage: "far_ahead", deltaPercent: 40, expectedUsedPercent: 2, etaSeconds: 0, willLastToReset: false }),
    });
    expect(metricRowPresentation(input({ lane: "secondary", snap }), en).metaText).toBe(
      "40% in deficit · Runs out now",
    );
    const later = weekly({
      pace: pace({ stage: "far_ahead", deltaPercent: 40, expectedUsedPercent: 2, etaSeconds: 3600, willLastToReset: false }),
    });
    expect(metricRowPresentation(input({ lane: "secondary", snap: later }), en).metaText).toBeNull();
  });

  it.each<[string, Partial<MetricRowInput>]>([
    ["pace is turned off", { lane: "secondary", snap: weekly(), paceEnabled: false }],
    ["the window is used up", { lane: "secondary", snap: weekly({ usedPercent: 100, remainingPercent: 0 }) }],
    ["a session expects under 3%", { snap: rateWindow({ pace: pace({ expectedUsedPercent: 2 }) }) }],
    ["the lane is tertiary", { lane: "tertiary", snap: weekly() }],
    ["an extra lane is not 5h or 7d", { lane: "extra", snap: weekly({ windowMinutes: 1440 }) }],
    ["the primary window length is unknown", { snap: rateWindow({ windowMinutes: null, pace: pace() }) }],
    ["the window has no pace", { lane: "secondary", snap: weekly({ pace: null }) }],
  ])("hides pace when %s", (_, overrides) => {
    const row = metricRowPresentation(input(overrides), en);
    expect(row.metaText).toBeNull();
    expect(row.bar.pacePercent).toBeNull();
  });

  it("shows pace on extra lanes that are exactly a session or a week", () => {
    const row = metricRowPresentation(input({ lane: "extra", snap: weekly() }), en);
    expect(row.metaText).toBe("23% in reserve · Lasts until reset");
  });

  // Upstream MenuCardView+ModelHelpers.swift:179-186 and
  // MenuCardView.swift:1407-1415: a regenerating window keeps its length but
  // shows the regen line instead of pace. Workday ticks stay (:1433-1437).
  it("hides pace on a regenerating window that keeps its length", () => {
    const session = rateWindow({ windowMinutes: 300, resetRegenerates: true, pace: pace() });
    const sessionRow = metricRowPresentation(input({ snap: session }), en);
    expect(sessionRow.metaText).toBeNull();
    expect(sessionRow.bar.pacePercent).toBeNull();
    expect(sessionRow.bar.paceDeficit).toBe(false);

    const week = weekly({ resetRegenerates: true });
    const weekRow = metricRowPresentation(
      input({ lane: "secondary", snap: week, weeklyProgressWorkDays: 5, usageThresholds: null }),
      en,
    );
    expect(weekRow.metaText).toBeNull();
    expect(weekRow.bar.pacePercent).toBeNull();
    expect(weekRow.bar.markers).toEqual([
      { percent: 20, kind: "workday" },
      { percent: 40, kind: "workday" },
      { percent: 60, kind: "workday" },
      { percent: 80, kind: "workday" },
    ]);
  });

  describe("reset-window pace (upstream ProviderPaceCapability)", () => {
    // Copilot premium requests: no window length, 60% used half way through
    // a 28-day cycle, scored by the bridge against the resolved month.
    const copilot = (overrides: Partial<WindowPaceSnapshot> = {}) =>
      rateWindow({
        usedPercent: 60,
        remainingPercent: 40,
        windowMinutes: null,
        pace: pace({
          stage: "ahead",
          deltaPercent: 10,
          expectedUsedPercent: 50,
          actualUsedPercent: 60,
          etaSeconds: 806_400,
          willLastToReset: false,
          resetWindow: true,
          ...overrides,
        }),
      });

    it("shows weekly pace on a primary window with no length", () => {
      const row = metricRowPresentation(input({ providerId: "copilot", snap: copilot() }), en);
      expect(row.metaText).toBe("10% in deficit · Runs out in 9d 8h");
      expect(row.bar.pacePercent).toBe(50);
      expect(row.bar.paceDeficit).toBe(true);
    });

    it("shows pace on the tertiary lane", () => {
      const snap = { ...copilot(), windowMinutes: 40_320 };
      const row = metricRowPresentation(input({ providerId: "cursor", lane: "tertiary", snap }), en);
      expect(row.metaText).toBe("10% in deficit · Runs out in 9d 8h");
    });

    it("keeps the 3% expected floor", () => {
      const snap = copilot({ expectedUsedPercent: 2, etaSeconds: 3600 });
      expect(metricRowPresentation(input({ providerId: "copilot", snap }), en).metaText).toBeNull();
    });

    it("leaves extra lanes on their exact-window rule", () => {
      const snap = { ...copilot(), windowMinutes: 40_320 };
      const row = metricRowPresentation(input({ providerId: "copilot", lane: "extra", snap }), en);
      expect(row.metaText).toBeNull();
    });
  });

  it("hides the pace line in compact rows but keeps the bar marker", () => {
    const row = metricRowPresentation(input({ lane: "secondary", snap: weekly(), compact: true }), en);
    expect(row.metaText).toBeNull();
    expect(row.bar.pacePercent).toBe(40);
  });
});

describe("metricRowPresentation bar", () => {
  it.each([
    [37, false, 63],
    [37, true, 37],
    [99.6, false, 0],
    [0.3, false, 100],
    [115, true, 100],
    [115, false, 0],
    [62.4, false, 37.6],
  ])("fills %d%% used with showAsUsed %s to %d%%", (used, showAsUsed, fill) => {
    const row = metricRowPresentation(
      input({ snap: rateWindow({ usedPercent: used, remainingPercent: 100 - used }), showAsUsed }),
      en,
    );
    expect(row.bar.fillPercent).toBeCloseTo(fill, 9);
  });

  it("places the Windows usage thresholds as quota warning markers", () => {
    expect(metricRowPresentation(input(), en).bar.markers).toEqual([
      { percent: 10, kind: "warning" },
      { percent: 30, kind: "warning" },
    ]);
    expect(metricRowPresentation(input({ showAsUsed: true }), en).bar.markers).toEqual([
      { percent: 70, kind: "warning" },
      { percent: 90, kind: "warning" },
    ]);
  });

  it("resolves Custom quota warnings per window and field, falling back to global", () => {
    const providerQuotaWarnings = {
      claude: { session: { high: 75 }, weekly: { high: 75, critical: 95 } },
      codex: { session: { high: 50, critical: 60 } },
    };
    const usageThresholds = { ...thresholds(70), providerQuotaWarnings };
    const weeklyRow = metricRowPresentation(
      input({ lane: "secondary", snap: weekly(), showAsUsed: true, usageThresholds }),
      en,
    );
    expect(weeklyRow.bar.markers.map((m) => m.percent)).toEqual([75, 95]);
    const sessionRow = metricRowPresentation(input({ showAsUsed: true, usageThresholds }), en);
    expect(sessionRow.bar.markers.map((m) => m.percent)).toEqual([75, 90]);
    const tertiaryRow = metricRowPresentation(
      input({ lane: "tertiary", snap: weekly(), showAsUsed: true, usageThresholds }),
      en,
    );
    expect(tertiaryRow.bar.markers.map((m) => m.percent)).toEqual([75, 95]);
  });

  it("hides warning markers for a window whose quota warnings are Off", () => {
    const usageThresholds = {
      ...thresholds(70),
      providerQuotaWarnings: { claude: { session: { enabled: false, high: 40, critical: 60 } } },
    };
    expect(
      metricRowPresentation(input({ showAsUsed: true, usageThresholds }), en).bar.markers,
    ).toEqual([]);
    const weeklyRow = metricRowPresentation(
      input({ lane: "secondary", snap: weekly(), showAsUsed: true, usageThresholds }),
      en,
    );
    expect(weeklyRow.bar.markers.filter((m) => m.kind === "warning").map((m) => m.percent)).toEqual([
      70, 90,
    ]);
  });

  it("drops edge and duplicate thresholds and never marks extra lanes", () => {
    expect(metricRowPresentation(input({ usageThresholds: thresholds(0, 100) }), en).bar.markers).toEqual([]);
    expect(metricRowPresentation(input({ usageThresholds: thresholds(80, 80) }), en).bar.markers).toEqual([
      { percent: 20, kind: "warning" },
    ]);
    expect(metricRowPresentation(input({ lane: "extra", snap: weekly() }), en).bar.markers).toEqual([]);
  });

  it("draws no warning markers without threshold settings", () => {
    expect(metricRowPresentation(input({ usageThresholds: null }), en).bar.markers).toEqual([]);
  });

  it("adds work day ticks to a seven-day secondary window only", () => {
    const row = metricRowPresentation(
      input({ lane: "secondary", snap: weekly(), weeklyProgressWorkDays: 5, usageThresholds: thresholds(40) }),
      en,
    );
    expect(row.bar.markers).toEqual([
      { percent: 10, kind: "warning" },
      { percent: 20, kind: "workday" },
      { percent: 40, kind: "workday" },
      { percent: 60, kind: "warning" },
      { percent: 80, kind: "workday" },
    ]);
    for (const overrides of [
      { lane: "primary" as const, snap: weekly() },
      { lane: "secondary" as const, snap: weekly({ windowMinutes: 1440 }) },
      { lane: "secondary" as const, snap: weekly(), weeklyProgressWorkDays: 1 },
      { lane: "secondary" as const, snap: weekly(), weeklyProgressWorkDays: 8 },
      { lane: "secondary" as const, snap: weekly(), weeklyProgressWorkDays: null },
    ]) {
      const kinds = metricRowPresentation(input({ weeklyProgressWorkDays: 5, ...overrides }), en).bar.markers.map(
        (m) => m.kind,
      );
      expect(kinds).toEqual(["warning", "warning"]);
    }
  });

  it("hides quota warning markers when the toggle is off", () => {
    const row = metricRowPresentation(
      input({ lane: "secondary", snap: weekly(), weeklyProgressWorkDays: 5, quotaWarningMarkersVisible: false }),
      en,
    );
    expect(row.bar.markers.map((m) => m.kind)).toEqual(["workday", "workday", "workday", "workday"]);
    expect(metricRowPresentation(input({ quotaWarningMarkersVisible: false }), en).bar.valueText).toBe("63%");
  });

  it("hides work day ticks for the hidden style and passes the visible style to the bar", () => {
    const hidden = metricRowPresentation(
      input({ lane: "secondary", snap: weekly(), weeklyProgressWorkDays: 5, workdayTickStyle: "hidden" }),
      en,
    );
    expect(hidden.bar.markers.map((m) => m.kind)).toEqual(["warning", "warning"]);
    const strong = metricRowPresentation(
      input({ lane: "secondary", snap: weekly(), weeklyProgressWorkDays: 7, workdayTickStyle: "high_contrast" }),
      en,
    );
    expect(strong.bar.workdayTickStyle).toBe("high_contrast");
    expect(strong.bar.markers.filter((m) => m.kind === "workday")).toHaveLength(6);
  });

  it("describes the value and markers for assistive technology", () => {
    expect(metricRowPresentation(input(), en).bar).toMatchObject({
      valuePercent: 63,
      valueText: "63%. Quota warnings: 10%, 30%",
    });
    const row = metricRowPresentation(
      input({ lane: "secondary", snap: weekly(), weeklyProgressWorkDays: 3, showAsUsed: true }),
      en,
    );
    expect(row.bar.valuePercent).toBe(37);
    expect(row.bar.valueText).toBe("37%. Quota warnings: 70%, 90%. Work days: 33%, 67%");
    expect(
      metricRowPresentation(input({ lane: "extra", snap: weekly({ pace: null }) }), en).bar.valueText,
    ).toBe("63%");
  });
});

describe("monthly daily target", () => {
  // A calendar-month allowance on the secondary lane with a synthetic target.
  function monthly(dailyTarget: RateWindowSnapshot["dailyTarget"]): RateWindowSnapshot {
    return rateWindow({
      usedPercent: 79,
      remainingPercent: 21,
      windowMinutes: 43_200,
      pace: pace(),
      dailyTarget,
    });
  }

  it("shows the per-day share when the window has no pace", () => {
    const snap = { ...monthly({ amount: 0, unit: "%" }), pace: null };
    expect(metricRowPresentation(input({ lane: "secondary", snap }), en).metaText).toContain(
      "≈ 0% / day to last until reset",
    );
  });

  it("shows a credit allowance's per-day share in the allowance's unit", () => {
    const row = metricRowPresentation(
      input({ lane: "secondary", snap: monthly({ amount: 21.0909, unit: "credits" }) }),
      en,
    );
    expect(row.metaText).toContain("≈ 21.09 credits / day to last until reset");
  });

  it("formats percent, request and unknown units", () => {
    const text = (dailyTarget: RateWindowSnapshot["dailyTarget"]) =>
      metricRowPresentation(input({ lane: "secondary", snap: monthly(dailyTarget) }), en).metaText ?? "";

    expect(text({ amount: 2.46, unit: "%" })).toContain("≈ 2.5% / day to last until reset");
    expect(text({ amount: 10.714, unit: "requests" })).toContain(
      "≈ 10.71 requests / day to last until reset",
    );
    expect(text({ amount: 5, unit: "tokens" })).not.toContain("/ day");
  });

  it("follows the pace setting and hides on compact rows", () => {
    const target = { amount: 21.09, unit: "credits" };
    expect(
      metricRowPresentation(input({ lane: "secondary", snap: monthly(target), paceEnabled: false }), en)
        .metaText,
    ).toBeNull();
    expect(
      metricRowPresentation(input({ lane: "secondary", snap: monthly(target), compact: true }), en).metaText,
    ).toBeNull();
  });

  it("shows nothing when the window carries no target", () => {
    const row = metricRowPresentation(input({ lane: "secondary", snap: monthly(undefined) }), en);
    expect(row.metaText ?? "").not.toContain("/ day");
  });
});
