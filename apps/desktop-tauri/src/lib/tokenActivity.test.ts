import { describe, expect, it } from "vitest";
import {
  buildActivitySeries,
  coveredDayCount,
  cumulativeActivity,
  dailyLevels,
  dayKey,
  dayNumber,
  gridFrame,
  hasUnknownCoverage,
  isVisibleCell,
  isVisibleWeek,
  latestDay,
  mergeTokenActivity,
  monthMarkers,
  navigationCandidate,
  placeMonthLabels,
  revealOffset,
  scrollNavigation,
  selectionAfterActivating,
  seriesDay,
  tooltipCenterX,
  tooltipOriginY,
  visibleDayCount,
  weekFill,
  weekdayOf,
  weeklyActivity,
  type TokenActivityPoint,
} from "./tokenActivity";

function day(key: string): number {
  const value = dayNumber(key);
  if (value == null) throw new Error(`bad day ${key}`);
  return value;
}

/** Every day of the year ending `today`, scanned, with `tokens(dayNumber)`. */
function fullYear(today: string, tokens: (date: number) => number | null = () => 0): TokenActivityPoint[] {
  const end = day(today);
  return Array.from({ length: 365 }, (_, offset) => {
    const date = end - 364 + offset;
    return { day: dayKey(date), totalTokens: tokens(date), isScanned: true };
  });
}

describe("day keys", () => {
  it("accepts leap days only in leap years", () => {
    expect(dayNumber("2024-02-29")).not.toBeNull();
    expect(dayNumber("2023-02-29")).toBeNull();
    expect(dayNumber("2024-13-01")).toBeNull();
    expect(dayNumber("not-a-day")).toBeNull();
  });

  it("round-trips across a year boundary", () => {
    const newYear = day("2025-01-01");
    expect(dayKey(newYear - 1)).toBe("2024-12-31");
    expect(dayKey(day("2024-02-28") + 1)).toBe("2024-02-29");
    expect(dayKey(day("2023-02-28") + 1)).toBe("2023-03-01");
  });

  it("numbers weekdays from Sunday", () => {
    expect(weekdayOf(day("2024-01-07"))).toBe(0);
    expect(weekdayOf(day("2024-01-13"))).toBe(6);
  });
});

describe("mergeTokenActivity", () => {
  it("sums scanned sources per day and leaves hidden sources out", () => {
    const merged = mergeTokenActivity(
      [
        { providerId: "codex", days: [{ day: "2024-05-01", totalTokens: 10, isScanned: true }] },
        { providerId: "claude", days: [{ day: "2024-05-01", totalTokens: 5, isScanned: true }] },
        { providerId: "Hidden", days: [{ day: "2024-05-01", totalTokens: 1000, isScanned: true }] },
      ],
      ["hidden"],
    );
    expect(merged).toEqual([{ day: "2024-05-01", totalTokens: 15, isScanned: true }]);
  });

  it("keeps a window edge apart from a real gap", () => {
    const merged = mergeTokenActivity(
      [
        {
          providerId: "codex",
          days: [
            { day: "2024-05-01", totalTokens: null, isScanned: false },
            { day: "2024-05-02", totalTokens: null, isScanned: true },
            { day: "2024-05-03", totalTokens: 7, isScanned: true },
          ],
        },
        {
          providerId: "claude",
          days: [
            { day: "2024-05-01", totalTokens: null, isScanned: false },
            { day: "2024-05-02", totalTokens: 4, isScanned: true },
            { day: "2024-05-03", totalTokens: null, isScanned: false },
          ],
        },
      ],
      [],
    );
    expect(merged).toEqual([
      { day: "2024-05-01", totalTokens: null, isScanned: false },
      { day: "2024-05-02", totalTokens: null, isScanned: true },
      { day: "2024-05-03", totalTokens: 7, isScanned: true },
    ]);
  });

  it("is empty without sources", () => {
    expect(mergeTokenActivity(undefined, [])).toEqual([]);
  });
});

describe("buildActivitySeries", () => {
  it("starts on the Sunday on or before the first of 365 days", () => {
    for (const today of ["2024-12-31", "2025-01-01", "2024-03-01", "2023-03-01", "2025-06-15"]) {
      const series = buildActivitySeries([], day(today));
      expect(weekdayOf(series.start)).toBe(0);
      expect(series.rangeStart).toBe(day(today) - 364);
      expect(series.start).toBeLessThanOrEqual(series.rangeStart);
      expect(series.rangeStart - series.start).toBeLessThan(7);
      expect(visibleDayCount(series)).toBe(365);
      expect(series.daily).toHaveLength(53 * 7);
    }
  });

  it("covers a leap year ending on Dec 31", () => {
    const series = buildActivitySeries(fullYear("2024-12-31", () => 1), day("2024-12-31"));
    expect(dayKey(series.rangeStart)).toBe("2024-01-02");
    expect(dayKey(series.start)).toBe("2023-12-31");
    expect(isVisibleCell(series, 1)).toBe(false);
    expect(isVisibleCell(series, 2)).toBe(true);
    expect(dayKey(seriesDay(series, 366))).toBe("2024-12-31");
    expect(isVisibleCell(series, 367)).toBe(false);
    expect(coveredDayCount(series)).toBe(365);
    expect(hasUnknownCoverage(series)).toBe(false);
  });

  it("crosses into a new year on Jan 1", () => {
    const series = buildActivitySeries(fullYear("2025-01-01"), day("2025-01-01"));
    expect(dayKey(series.rangeStart)).toBe("2024-01-03");
    expect(dayKey(series.start)).toBe("2023-12-31");
    expect(series.daily.filter((_, index) => isVisibleCell(series, index))).toHaveLength(365);
  });

  it("marks unknown days unavailable and unscanned days outside the scan window", () => {
    const today = day("2024-06-30");
    const points = fullYear("2024-06-30", (date) => (date === today - 1 ? null : 3));
    points[0] = { ...points[0], totalTokens: null, isScanned: false };
    const series = buildActivitySeries(points, today);
    const gap = today - 1 - series.start;
    const edge = series.rangeStart - series.start;
    expect(series.isCovered[gap]).toBe(false);
    expect(series.isScanned[gap]).toBe(true);
    expect(series.isCovered[edge]).toBe(false);
    expect(series.isScanned[edge]).toBe(false);
    expect(coveredDayCount(series)).toBe(363);
    expect(hasUnknownCoverage(series)).toBe(true);
  });

  it("finds the latest day of the points", () => {
    expect(latestDay(fullYear("2024-06-30"))).toBe(day("2024-06-30"));
    expect(latestDay([])).toBeNull();
  });
});

describe("weekly and cumulative activity", () => {
  // 2024-12-31 is a Tuesday; cell 0 is Sunday 2023-12-31.
  const today = day("2024-12-31");

  it("ignores the leading window edge but keeps real gaps unavailable", () => {
    const firstScanned = day("2024-12-01"); // a Sunday
    const gapDay = day("2024-12-18");
    const points = fullYear("2024-12-31", (date) => (date === gapDay ? null : 10)).map((point) =>
      day(point.day) < firstScanned ? { ...point, totalTokens: null, isScanned: false } : point,
    );
    const series = buildActivitySeries(points, today);
    const weekly = weeklyActivity(series);
    const firstWeek = (firstScanned - series.start) / 7;
    const gapWeek = Math.floor((gapDay - series.start) / 7);

    expect(weekly.values).toHaveLength(53);
    expect(weekly.isScanned[firstWeek - 1]).toBe(false);
    expect(weekly.isScanned[firstWeek]).toBe(true);
    expect(weekly.values[firstWeek]).toBe(70);
    expect(weekly.isCovered[firstWeek]).toBe(true);
    expect(weekly.isCovered[gapWeek]).toBe(false);
    expect(weekly.values[gapWeek]).toBe(60);

    const cumulative = cumulativeActivity(weekly);
    expect(cumulative.isCovered[firstWeek - 1]).toBe(false);
    expect(cumulative.isCovered[firstWeek]).toBe(true);
    expect(cumulative.isCovered[gapWeek - 1]).toBe(true);
    expect(cumulative.isCovered[gapWeek]).toBe(false);
    expect(cumulative.isCovered[52]).toBe(false);
    expect(cumulative.values[52]).toBe(weekly.values.reduce((total, value) => total + value, 0));
  });

  it("keeps a fully scanned year covered to the end", () => {
    const series = buildActivitySeries(fullYear("2024-12-31", () => 1), today);
    const cumulative = cumulativeActivity(weeklyActivity(series));
    expect(cumulative.isCovered.every(Boolean)).toBe(true);
    expect(cumulative.values[52]).toBe(365);
  });

  it("limits visible weeks to the range", () => {
    const series = buildActivitySeries([], today);
    expect(isVisibleWeek(series, 0)).toBe(true);
    expect(isVisibleWeek(series, 52)).toBe(true);
    expect(isVisibleWeek(series, 53)).toBe(false);
    expect(isVisibleWeek(series, -1)).toBe(false);
  });

  it("fills at least one row for any activity", () => {
    expect(weekFill(0, 100)).toBe(0);
    expect(weekFill(1, 100)).toBe(1);
    expect(weekFill(50, 100)).toBe(4);
    expect(weekFill(100, 100)).toBe(7);
    expect(weekFill(5, 0)).toBe(1);
  });
});

describe("dailyLevels", () => {
  it("buckets by quartile of the busiest day", () => {
    expect(dailyLevels([0, 1, 25, 26, 50, 51, 75, 76, 100])).toEqual([0, 1, 1, 2, 2, 3, 3, 4, 4]);
  });

  it("is all zero without activity", () => {
    expect(dailyLevels([0, 0, 0])).toEqual([0, 0, 0]);
  });
});

describe("geometry", () => {
  it("never shrinks a day below the minimum cell plus gap", () => {
    expect(gridFrame(200)).toEqual({ width: 689, height: 91, pitch: 13, cell: 10 });
    const wide = gridFrame(1060);
    expect(wide.width).toBe(1060);
    expect(wide.pitch).toBe(20);
    expect(wide.cell).toBe(17);
  });

  it("keeps the tooltip inside the visible span", () => {
    expect(tooltipCenterX(0, 148, 689)).toBe(82);
    expect(tooltipCenterX(689, 148, 689)).toBe(607);
    expect(tooltipCenterX(300, 148, 689)).toBe(300);
    expect(tooltipCenterX(0, 148, 689, { minX: 289, maxX: 689 })).toBe(371);
  });

  it("puts the tooltip below a cell near the top", () => {
    expect(tooltipOriginY(80, 50, 91)).toBe(25);
    expect(tooltipOriginY(6, 50, 91)).toBe(11);
    expect(tooltipOriginY(6, 50, 40)).toBe(0);
  });
});

describe("scrollNavigation", () => {
  it("disables scrolling when the whole grid fits", () => {
    const fits = scrollNavigation(689, 689, 0);
    expect(fits.isScrollable).toBe(false);
    expect(fits.canScrollEarlier).toBe(false);
    expect(fits.canScrollLater).toBe(false);
    expect(scrollNavigation(689, 0, 0).isScrollable).toBe(false);
  });

  it("enables only the direction with more content", () => {
    const atEnd = scrollNavigation(689, 400, 289);
    expect(atEnd.isScrollable).toBe(true);
    expect(atEnd.canScrollEarlier).toBe(true);
    expect(atEnd.canScrollLater).toBe(false);
    const atStart = scrollNavigation(689, 400, 0);
    expect(atStart.canScrollEarlier).toBe(false);
    expect(atStart.canScrollLater).toBe(true);
  });

  it("pages by the viewport less two weeks, aligned to a week", () => {
    expect(scrollNavigation(689, 400, 289).targetColumn("earlier")).toBe(0);
    expect(scrollNavigation(689, 400, 0).targetColumn("later")).toBe(23);
    expect(scrollNavigation(689, 200, 289).targetColumn("earlier")).toBe(8);
    expect(scrollNavigation(689, 20, 289).targetColumn("later")).toBe(24);
  });

  it("reveals a column only when it is clipped", () => {
    expect(revealOffset(30, 13, 200, 289)).toBeNull();
    expect(revealOffset(0, 13, 200, 289)).toBe(0);
    expect(revealOffset(52, 13, 200, 0)).toBe(689 - 200);
  });
});

describe("keyboard and selection", () => {
  it("moves by weeks and stays inside a week", () => {
    expect(navigationCandidate(14, "left")).toBe(7);
    expect(navigationCandidate(14, "right")).toBe(21);
    expect(navigationCandidate(14, "up")).toBeNull();
    expect(navigationCandidate(20, "down")).toBeNull();
    expect(navigationCandidate(15, "up")).toBe(14);
    expect(navigationCandidate(15, "down")).toBe(16);
  });

  it("toggles a covered day and refuses an unavailable one", () => {
    const today = day("2024-06-30");
    const points = fullYear("2024-06-30", (date) => (date === today ? null : 1));
    const series = buildActivitySeries(points, today);
    const yesterday = today - 1 - series.start;
    expect(selectionAfterActivating(series, yesterday, null)).toBe("2024-06-29");
    expect(selectionAfterActivating(series, yesterday, "2024-06-29")).toBeNull();
    expect(selectionAfterActivating(series, today - series.start, null)).toBeUndefined();
    expect(selectionAfterActivating(series, 53 * 7 - 1, null)).toBeUndefined();
  });
});

describe("month labels", () => {
  it("labels weeks that start in the first seven days of a month", () => {
    const series = buildActivitySeries([], day("2024-12-31"));
    const markers = monthMarkers(series, (date) => dayKey(date).slice(0, 7));
    expect(markers[0]).toEqual({ column: 1, label: "2024-01" });
    expect(markers.map((marker) => marker.label)).toEqual([
      "2024-01", "2024-02", "2024-03", "2024-04", "2024-05", "2024-06",
      "2024-07", "2024-08", "2024-09", "2024-10", "2024-11", "2024-12",
    ]);
  });

  it("places labels right to left without overlap or overflow", () => {
    expect(placeMonthLabels([0, 40, 100], [20, 20, 20], 110)).toEqual([0, 40, 90]);
    expect(placeMonthLabels([0, 30, 40], [20, 20, 20], 60)).toEqual([0, 16, 40]);
  });
});
