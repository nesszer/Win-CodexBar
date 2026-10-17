import { describe, expect, it } from "vitest";
import type { SpendTrend, SpendTrendDailyPoint, SpendTrendHourlyDay, SpendTrendSource } from "../types/bridge";
import {
  bucketAt,
  buildDailyTrendModel,
  buildHourlyTrendModel,
  dayKey,
  dayOrdinal,
  drillTarget,
  emptyState,
  focusedHourlyDay,
  hourText,
  hourlyDays,
  hourlyTicks,
  hourlyTimeZoneText,
  needsScrolling,
  overviewUnit,
  resolveIsolatedSource,
  sourceLabel,
  tickStride,
  toggleIsolatedSource,
  unitEnd,
  unitStart,
  utcOffsetText,
  visibleDayCount,
} from "./spendTrendModel";

const codex: SpendTrendSource = { id: "codex", providerId: "codex", displayName: "Codex", knownZero: false };
const claude: SpendTrendSource = { id: "claude", providerId: "claude", displayName: "Claude", knownZero: false };

function addDays(day: string, count: number): string {
  return dayKey((dayOrdinal(day) as number) + count);
}

function trend(overrides: Partial<SpendTrend> = {}): SpendTrend {
  return {
    startDay: "2026-03-01",
    endDay: "2026-03-30",
    timeZone: "UTC",
    sources: [codex, claude],
    daily: [],
    hourly: [],
    hourlyDays: [],
    ...overrides,
  };
}

function windowOf(days: number, endDay = "2026-06-30"): Pick<SpendTrend, "startDay" | "endDay"> {
  return { startDay: addDays(endDay, -(days - 1)), endDay };
}

/** Europe/Berlin on 2026-03-29: clocks jump from 02:00 to 03:00, so the day has 23 hours. */
function berlinSpringForward(): SpendTrendHourlyDay {
  const start = Date.parse("2026-03-28T23:00:00Z");
  const hours = Array.from({ length: 23 }, (_, index) => ({
    start: new Date(start + index * 3_600_000).toISOString(),
    offsetMinutes: index < 2 ? 60 : 120,
  }));
  return { day: "2026-03-29", start: hours[0].start, end: "2026-03-29T22:00:00.000Z", hours };
}

describe("day keys", () => {
  it("round-trips calendar days and rejects impossible dates", () => {
    expect(dayKey(dayOrdinal("2024-02-29") as number)).toBe("2024-02-29");
    expect(dayOrdinal("2026-02-29")).toBeNull();
    expect(dayOrdinal("2026-3-1")).toBeNull();
  });
});

describe("overview unit thresholds", () => {
  it("switches to weeks after 45 days and to months after 180 days", () => {
    expect(overviewUnit(45)).toBe("day");
    expect(overviewUnit(46)).toBe("week");
    expect(overviewUnit(180)).toBe("week");
    expect(overviewUnit(181)).toBe("month");
  });

  it("buckets a 46-day window into weeks and a 181-day window into months", () => {
    expect(buildDailyTrendModel(trend(windowOf(45)))?.unit).toBe("day");
    expect(buildDailyTrendModel(trend(windowOf(46)))?.unit).toBe("week");
    expect(buildDailyTrendModel(trend(windowOf(180)))?.unit).toBe("week");
    expect(buildDailyTrendModel(trend(windowOf(181)))?.unit).toBe("month");
  });
});

describe("unit bounds", () => {
  const wednesday = dayOrdinal("2026-04-15") as number;

  it("starts weeks on the locale's first weekday", () => {
    expect(dayKey(unitStart(wednesday, "week", 1))).toBe("2026-04-13");
    expect(dayKey(unitStart(wednesday, "week", 0))).toBe("2026-04-12");
    expect(dayKey(unitEnd(wednesday, "week", 1))).toBe("2026-04-20");
  });

  it("uses calendar months, including leap February", () => {
    const leap = dayOrdinal("2024-02-10") as number;
    expect(dayKey(unitStart(leap, "month"))).toBe("2024-02-01");
    expect(unitEnd(leap, "month") - unitStart(leap, "month")).toBe(29);
  });
});

describe("daily model", () => {
  const daily: SpendTrendDailyPoint[] = [
    { sourceId: "codex", day: "2026-03-02", costUsd: 2 },
    { sourceId: "claude", day: "2026-03-02", costUsd: 3 },
    { sourceId: "codex", day: "2026-03-05", costUsd: 5 },
    { sourceId: "claude", day: "2026-03-06", costUsd: null },
    { sourceId: "claude", day: "2026-03-07", costUsd: 0 },
    { sourceId: "codex", day: "2026-02-28", costUsd: 99 },
  ];

  it("stacks sources per day, keeps unknown and zero days undrawn, and ignores days outside the window", () => {
    const model = buildDailyTrendModel(trend({ daily }));
    expect(model?.slots).toHaveLength(30);
    expect(model?.buckets.map((bucket) => dayKey(bucket.key))).toEqual(["2026-03-02", "2026-03-05"]);
    expect(model?.buckets[0].segments).toEqual([
      { sourceId: "claude", cost: 3, start: 0, end: 3 },
      { sourceId: "codex", cost: 2, start: 3, end: 5 },
    ]);
    expect(model?.total).toBe(10);
  });

  it("reports the first of tied peaks", () => {
    const model = buildDailyTrendModel(trend({ daily }));
    expect(dayKey(model?.peak?.key as number)).toBe("2026-03-02");
    expect(model?.yMax).toBeCloseTo(5.75);
  });

  it("isolates one source without changing the window", () => {
    const model = buildDailyTrendModel(trend({ daily }), { sourceId: "claude" });
    expect(model?.buckets.map((bucket) => bucket.total)).toEqual([3]);
    expect(model?.total).toBe(3);
    expect(model?.slots).toHaveLength(30);
  });

  it("has no total when nothing is drawable", () => {
    const model = buildDailyTrendModel(trend({ daily: [{ sourceId: "codex", day: "2026-03-03", costUsd: 0 }] }));
    expect(model?.total).toBeNull();
    expect(model?.peak).toBeNull();
  });

  it("widens partial weeks to whole units but sums only days inside the window", () => {
    const window = windowOf(60, "2026-06-30");
    const model = buildDailyTrendModel(
      trend({
        ...window,
        daily: [
          { sourceId: "codex", day: addDays(window.startDay, -1), costUsd: 50 },
          { sourceId: "codex", day: window.startDay, costUsd: 4 },
        ],
      }),
    );
    expect(model?.unit).toBe("week");
    expect(model?.domain.start).toBe(unitStart(dayOrdinal(window.startDay) as number, "week", 1));
    expect(model?.buckets[0].total).toBe(4);
  });
});

describe("scrolling", () => {
  it("scrolls daily charts longer than 32 days and shows 31 at a time", () => {
    const short = buildDailyTrendModel(trend(windowOf(32))) as NonNullable<ReturnType<typeof buildDailyTrendModel>>;
    const long = buildDailyTrendModel(trend(windowOf(33))) as NonNullable<ReturnType<typeof buildDailyTrendModel>>;
    expect(needsScrolling(short)).toBe(false);
    expect(visibleDayCount(short)).toBe(31);
    expect(needsScrolling(long)).toBe(true);
    expect(visibleDayCount(long)).toBe(31);
  });

  it("never scrolls week or month buckets", () => {
    const weekly = buildDailyTrendModel(trend(windowOf(120))) as NonNullable<ReturnType<typeof buildDailyTrendModel>>;
    expect(weekly.unit).toBe("week");
    expect(needsScrolling(weekly)).toBe(false);
  });

  it("thins axis labels on long daily charts", () => {
    const month = buildDailyTrendModel(trend(windowOf(30))) as NonNullable<ReturnType<typeof buildDailyTrendModel>>;
    const week = buildDailyTrendModel(trend(windowOf(7))) as NonNullable<ReturnType<typeof buildDailyTrendModel>>;
    expect(tickStride(month)).toBe(7);
    expect(tickStride(week)).toBe(1);
  });
});

describe("drill-down", () => {
  it("opens a partial week at its real days inside the window", () => {
    const window = windowOf(60, "2026-06-30");
    const model = buildDailyTrendModel(trend(window)) as NonNullable<ReturnType<typeof buildDailyTrendModel>>;
    const first = model.slots[0];
    const drill = drillTarget(model, first.key, []);
    expect(drill).toEqual({
      kind: "interval",
      interval: { start: dayOrdinal(window.startDay), end: first.end },
    });
  });

  it("re-buckets a drilled month as days", () => {
    const window = windowOf(200, "2026-06-30");
    const overview = buildDailyTrendModel(trend(window)) as NonNullable<ReturnType<typeof buildDailyTrendModel>>;
    expect(overview.unit).toBe("month");
    const june = overview.slots[overview.slots.length - 1];
    const drill = drillTarget(overview, june.key, []);
    if (drill?.kind !== "interval") throw new Error("expected an interval drill");
    const focused = buildDailyTrendModel(trend(window), { focus: drill.interval });
    expect(focused?.unit).toBe("day");
    expect(focused?.slots).toHaveLength(30);
  });

  it("opens a day's hours only when that day has hourly data", () => {
    const model = buildDailyTrendModel(trend()) as NonNullable<ReturnType<typeof buildDailyTrendModel>>;
    const day = dayOrdinal("2026-03-29") as number;
    expect(drillTarget(model, day, [])).toBeNull();
    expect(drillTarget(model, day, ["2026-03-29"])).toEqual({ kind: "day", day: "2026-03-29" });
  });

  it("does not fall back to a nearby bucket for an empty slot", () => {
    const model = buildDailyTrendModel(
      trend({ daily: [{ sourceId: "codex", day: "2026-03-02", costUsd: 1 }] }),
    ) as NonNullable<ReturnType<typeof buildDailyTrendModel>>;
    expect(bucketAt(model, dayOrdinal("2026-03-03"))).toBeNull();
    expect(bucketAt(model, dayOrdinal("2026-03-02"))?.total).toBe(1);
  });
});

describe("hourly model", () => {
  const frame = berlinSpringForward();
  const hourlyTrend = trend({
    timeZone: "Europe/Berlin",
    hourlyDays: [frame],
    hourly: [
      { sourceId: "claude", hour: frame.hours[1].start, costUsd: 1.5 },
      { sourceId: "claude", hour: frame.hours[2].start, costUsd: 2 },
      { sourceId: "codex", hour: frame.hours[2].start, costUsd: 0.5 },
      { sourceId: "claude", hour: "2026-03-29T22:00:00.000Z", costUsd: 9 },
    ],
  });

  it("uses the day's real 23 hour slots and labels the offset change", () => {
    const model = buildHourlyTrendModel(hourlyTrend, "2026-03-29") as NonNullable<ReturnType<typeof buildHourlyTrendModel>>;
    expect(model.slots).toHaveLength(23);
    expect(model.total).toBe(4);
    expect(hourlyTimeZoneText(model)).toBe("UTC+01:00 → UTC+02:00");
    expect(hourText(model.slots[1])).toBe("01:00 UTC+01:00");
    expect(hourText(model.slots[2])).toBe("03:00 UTC+02:00");
  });

  it("places ticks on local clock hours and closes the day at 24:00", () => {
    const model = buildHourlyTrendModel(hourlyTrend, "2026-03-29") as NonNullable<ReturnType<typeof buildHourlyTrendModel>>;
    expect(hourlyTicks(model).map((tick) => tick.label)).toEqual(["00:00", "04:00", "08:00", "12:00", "16:00", "20:00", "24:00"]);
  });

  it("isolates a source in the hourly view", () => {
    const model = buildHourlyTrendModel(hourlyTrend, "2026-03-29", "codex");
    expect(model?.total).toBe(0.5);
  });

  it("lists only days with recorded hours and picks the focused day", () => {
    const empty: SpendTrendHourlyDay = { ...frame, day: "2026-03-30", start: frame.end, end: "2026-03-30T22:00:00.000Z", hours: [] };
    const days = hourlyDays({ ...hourlyTrend, hourly: hourlyTrend.hourly.slice(0, 3), hourlyDays: [frame, empty] });
    expect(days).toEqual(["2026-03-29"]);
    expect(focusedHourlyDay(["a", "b"], "a", "b")).toBe("a");
    expect(focusedHourlyDay(["a", "b"], null, "a")).toBe("a");
    expect(focusedHourlyDay(["a", "b"], "z", null)).toBe("b");
    expect(focusedHourlyDay([], null, null)).toBeNull();
  });

  it("returns null for a day without hourly frames", () => {
    expect(buildHourlyTrendModel(hourlyTrend, "2026-03-01")).toBeNull();
  });
});

describe("offset text", () => {
  it("formats whole, half-hour and negative offsets", () => {
    expect(utcOffsetText(0)).toBe("UTC");
    expect(utcOffsetText(330)).toBe("UTC+05:30");
    expect(utcOffsetText(-420)).toBe("UTC-07:00");
  });
});

describe("empty states", () => {
  it("reads 'No usage yet' when every shown source is a covered zero", () => {
    const zeroTrend = trend({ sources: [{ ...codex, knownZero: true }, { ...claude, knownZero: true }] });
    const model = buildDailyTrendModel(zeroTrend) as NonNullable<ReturnType<typeof buildDailyTrendModel>>;
    expect(emptyState(zeroTrend, model, null)).toBe("noUsage");
  });

  it("reads 'No usage yet' when an isolated source recorded a zero for every day in scope", () => {
    const window = { startDay: "2026-03-01", endDay: "2026-03-03" };
    const zeroDays = ["2026-03-01", "2026-03-02", "2026-03-03"].map((day) => ({ sourceId: "claude", day, costUsd: 0 }));
    const zeroTrend = trend({ ...window, daily: zeroDays });
    const model = buildDailyTrendModel(zeroTrend, { sourceId: "claude" }) as NonNullable<ReturnType<typeof buildDailyTrendModel>>;
    expect(emptyState(zeroTrend, model, "claude")).toBe("noUsage");
    expect(emptyState(zeroTrend, model, null)).toBe("unavailable");
  });

  it("reads 'Spend unavailable' when any day is unscanned or unpriced", () => {
    const window = { startDay: "2026-03-01", endDay: "2026-03-02" };
    const partial = trend({
      ...window,
      sources: [claude],
      daily: [
        { sourceId: "claude", day: "2026-03-01", costUsd: 0 },
        { sourceId: "claude", day: "2026-03-02", costUsd: null },
      ],
    });
    const model = buildDailyTrendModel(partial) as NonNullable<ReturnType<typeof buildDailyTrendModel>>;
    expect(emptyState(partial, model, null)).toBe("unavailable");
  });

  it("reads 'Spend unavailable' for an empty hourly day and when there are no sources", () => {
    const frame = berlinSpringForward();
    const hourlyTrend = trend({ hourlyDays: [frame], sources: [{ ...claude, knownZero: true }] });
    const hourly = buildHourlyTrendModel(hourlyTrend, frame.day) as NonNullable<ReturnType<typeof buildHourlyTrendModel>>;
    expect(emptyState(hourlyTrend, hourly, null)).toBe("unavailable");
    const none = trend({ sources: [] });
    expect(emptyState(none, buildDailyTrendModel(none) as NonNullable<ReturnType<typeof buildDailyTrendModel>>, null)).toBe(
      "unavailable",
    );
  });
});

describe("legend isolation", () => {
  it("toggles a source on and off", () => {
    expect(toggleIsolatedSource(null, "codex")).toBe("codex");
    expect(toggleIsolatedSource("codex", "claude")).toBe("claude");
    expect(toggleIsolatedSource("codex", "codex")).toBeNull();
  });

  it("drops isolation for a source that left the dashboard", () => {
    expect(resolveIsolatedSource("codex", [codex, claude])).toBe("codex");
    expect(resolveIsolatedSource("codex", [claude])).toBeNull();
  });

  it("disambiguates same-name sources with a stable ordinal", () => {
    const work = { ...codex, id: "codex:b" };
    const personal = { ...codex, id: "codex:a" };
    expect(sourceLabel(work, [work, personal, claude])).toBe("Codex · 2");
    expect(sourceLabel(personal, [work, personal, claude])).toBe("Codex · 1");
    expect(sourceLabel(claude, [work, personal, claude])).toBe("Claude");
  });
});
