import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LocaleKey } from "../../../i18n/keys";
import { dayKey, dayOrdinal } from "../../../lib/spendTrendModel";
import type { SpendTrend, SpendTrendHourlyDay } from "../../../types/bridge";
import { UsageSpendTrendPanel } from "./UsageSpendTrendPanel";

const t = (key: LocaleKey) => key;

const sources: SpendTrend["sources"] = [
  { id: "claude", providerId: "claude", displayName: "Claude", knownZero: false },
  { id: "codex", providerId: "codex", displayName: "Codex", knownZero: false },
];

function trend(overrides: Partial<SpendTrend> = {}): SpendTrend {
  return {
    startDay: "2026-03-01",
    endDay: "2026-03-30",
    timeZone: "Europe/Berlin",
    sources,
    daily: [
      { sourceId: "claude", day: "2026-03-10", costUsd: 2 },
      { sourceId: "codex", day: "2026-03-10", costUsd: 1 },
      { sourceId: "codex", day: "2026-03-29", costUsd: 5 },
    ],
    hourly: [],
    hourlyDays: [],
    ...overrides,
  };
}

/** Europe/Berlin on 2026-03-29 has 23 hours: 02:00 local never happens. */
function springForward(): SpendTrendHourlyDay {
  const start = Date.parse("2026-03-28T23:00:00Z");
  const hours = Array.from({ length: 23 }, (_, index) => ({
    start: new Date(start + index * 3_600_000).toISOString(),
    offsetMinutes: index < 2 ? 60 : 120,
  }));
  return { day: "2026-03-29", start: hours[0].start, end: "2026-03-29T22:00:00.000Z", hours };
}

function plainDay(day: string): SpendTrendHourlyDay {
  const start = Date.parse(`${day}T00:00:00Z`) - 3_600_000;
  const hours = Array.from({ length: 24 }, (_, index) => ({
    start: new Date(start + index * 3_600_000).toISOString(),
    offsetMinutes: 60,
  }));
  return { day, start: hours[0].start, end: new Date(start + 24 * 3_600_000).toISOString(), hours };
}

function hourlyTrend(): SpendTrend {
  const dst = springForward();
  const before = plainDay("2026-03-10");
  return trend({
    hourlyDays: [before, dst],
    hourly: [
      { sourceId: "claude", hour: before.hours[9].start, costUsd: 2 },
      { sourceId: "claude", hour: dst.hours[3].start, costUsd: 1.25 },
      { sourceId: "codex", hour: dst.hours[3].start, costUsd: 0.75 },
    ],
  });
}

describe("UsageSpendTrendPanel", () => {
  it("summarizes recorded spend and the highest bucket, and inspects the peak by default", () => {
    render(<UsageSpendTrendPanel trend={trend()} t={t} />);
    const summary = screen.getByTestId("spend-trend-summary");
    expect(summary.textContent).toContain("UsageSpendTrendRecorded");
    expect(summary.textContent).toContain("$8.00");
    expect(summary.textContent).toContain("UsageSpendTrendHighest");
    expect(summary.textContent).toContain("Mar 29, 2026 · $5.00");
    expect(screen.getByTestId("spend-trend-scope").textContent).toContain("UsageSpendTrendGroupDaily");

    const inspector = screen.getByTestId("spend-trend-inspection");
    expect((within(inspector).getByTestId("spend-trend-inspection-picker") as HTMLSelectElement).value).toBe(
      String(dayOrdinal("2026-03-29")),
    );
    expect(inspector.textContent).toContain("Codex$5.00");
    expect(within(inspector).queryByTestId("spend-trend-inspection-details")).toBeNull();
  });

  it("pins a bucket from the inspector menu and reads out each source", () => {
    render(<UsageSpendTrendPanel trend={trend()} t={t} />);
    fireEvent.change(screen.getByTestId("spend-trend-inspection-picker"), {
      target: { value: String(dayOrdinal("2026-03-10")) },
    });
    const inspector = screen.getByTestId("spend-trend-inspection");
    expect(inspector.textContent).toContain("Claude$2.00");
    expect(inspector.textContent).toContain("Codex$1.00");
    expect(inspector.textContent).toContain("$3.00");
  });

  it("reads 'No data available' when the pointer is over a slot without spend", () => {
    render(<UsageSpendTrendPanel trend={trend()} t={t} />);
    fireEvent.mouseMove(screen.getByTestId("spend-trend-hit"), { clientX: 20 });
    expect(screen.getByTestId("spend-trend-inspection").textContent).toContain("UsageSpendTrendNoData");
  });

  it("isolates a source from the legend and shows every source when clicked again", () => {
    render(<UsageSpendTrendPanel trend={trend()} t={t} />);
    const claude = screen.getByTestId("spend-trend-source-claude");
    const codex = screen.getByTestId("spend-trend-source-codex");
    expect(claude.getAttribute("title")).toBe("UsageSpendTrendIsolateHint");

    fireEvent.click(claude);
    expect(claude.getAttribute("aria-pressed")).toBe("true");
    expect(claude.className).toContain("is-selected");
    expect(codex.className).toContain("is-dimmed");
    expect(screen.getByTestId("spend-trend-summary").textContent).toContain("$2.00");

    fireEvent.click(claude);
    expect(claude.getAttribute("aria-pressed")).toBe("false");
    expect(codex.className).not.toContain("is-dimmed");
    expect(screen.getByTestId("spend-trend-summary").textContent).toContain("$8.00");
  });

  it("draws weeks for a long range, drills a week to its days and returns to the overview", () => {
    const end = "2026-06-30";
    const start = dayKey((dayOrdinal(end) as number) - 59);
    render(
      <UsageSpendTrendPanel
        trend={trend({
          startDay: start,
          endDay: end,
          daily: [
            { sourceId: "codex", day: "2026-06-02", costUsd: 4 },
            { sourceId: "claude", day: "2026-06-03", costUsd: 6 },
          ],
        })}
        t={t}
        firstWeekday={1}
      />,
    );
    expect(screen.getByTestId("spend-trend-scope").textContent).toContain("UsageSpendTrendGroupWeekly");
    expect(screen.getByTestId("spend-trend-summary").textContent).toContain("Jun 1, 2026 – Jun 7, 2026 · $10.00");

    fireEvent.click(screen.getByTestId("spend-trend-inspection-details"));
    const scope = screen.getByTestId("spend-trend-scope").textContent ?? "";
    expect(scope).toContain("Jun 1, 2026 – Jun 7, 2026");
    expect(scope).toContain("UsageSpendTrendGroupDaily");
    expect(screen.getAllByRole("option").length).toBeGreaterThan(0);

    fireEvent.click(screen.getByText("UsageSpendTrendOverview"));
    expect(screen.getByTestId("spend-trend-scope").textContent).toContain("UsageSpendTrendGroupWeekly");
  });

  it("opens a day's hours, labels the daylight-saving offset change and walks days", () => {
    render(<UsageSpendTrendPanel trend={hourlyTrend()} t={t} />);
    // The newest peak day has hours, so the inspector offers the hourly drill.
    fireEvent.click(screen.getByTestId("spend-trend-inspection-details"));

    expect(screen.getByText("UsageSpendHourlyEstimatedSpend")).toBeTruthy();
    const zone = screen.getByTestId("spend-trend-time-zone");
    expect(zone.textContent).toBe("UTC+01:00 → UTC+02:00");
    expect(zone.getAttribute("aria-label")).toBe("UsageSpendTrendTimeZone: UTC+01:00 → UTC+02:00");
    expect(zone.getAttribute("title")).toBe("Europe/Berlin");
    expect(screen.getByTestId("spend-trend-summary").textContent).toContain("UsageSpendTrendRecordedHourly");
    expect(screen.getByTestId("spend-trend-summary").textContent).toContain("Mar 29, 2026 · 04:00 UTC+02:00 · $2.00");
    expect(screen.getByTestId("spend-trend-chart").getAttribute("aria-description")).toBe(
      "UsageSpendTrendHourlySummary",
    );

    const previous = screen.getByLabelText("UsageSpendTrendPreviousDay") as HTMLButtonElement;
    const next = screen.getByLabelText("UsageSpendTrendNextDay") as HTMLButtonElement;
    expect(next.disabled).toBe(true);
    fireEvent.click(previous);
    expect((screen.getByTestId("spend-trend-day-picker") as HTMLSelectElement).value).toBe("2026-03-10");
    expect(previous.disabled).toBe(true);
    expect(screen.getByTestId("spend-trend-time-zone").textContent).toBe("UTC+01:00");

    fireEvent.click(screen.getByText("UsageSpendTrendOverview"));
    expect(screen.getByText("UsageSpendEstimatedSpend")).toBeTruthy();
    expect(screen.queryByTestId("spend-trend-time-zone")).toBeNull();
  });

  it("hides the Overview/Hour picker when no day has hourly spend", () => {
    render(<UsageSpendTrendPanel trend={trend()} t={t} />);
    expect(screen.queryByText("UsageSpendTrendHourly")).toBeNull();
    expect(screen.queryByText("UsageSpendTrendOverview")).toBeNull();
  });

  it("reads 'No usage yet' for covered zero sources", () => {
    render(
      <UsageSpendTrendPanel
        trend={trend({ daily: [], sources: sources.map((source) => ({ ...source, knownZero: true })) })}
        t={t}
      />,
    );
    const empty = screen.getByTestId("spend-trend-empty");
    expect(empty.getAttribute("data-state")).toBe("noUsage");
    expect(empty.textContent).toContain("UsageSpendTrendNoUsage");
    expect(screen.queryByTestId("spend-trend-summary")).toBeNull();
  });

  it("reads 'Spend unavailable' when spend is unknown", () => {
    render(
      <UsageSpendTrendPanel trend={trend({ daily: [{ sourceId: "codex", day: "2026-03-02", costUsd: null }] })} t={t} />,
    );
    const empty = screen.getByTestId("spend-trend-empty");
    expect(empty.getAttribute("data-state")).toBe("unavailable");
    expect(empty.textContent).toContain("UsageSpendTrendUnavailable");
  });

  it("keeps the brand icon hook beside legend entries", () => {
    render(
      <UsageSpendTrendPanel
        trend={trend()}
        t={t}
        renderSourceIcon={(source) => <i data-testid={`icon-${source.id}`} />}
      />,
    );
    expect(within(screen.getByTestId("spend-trend-source-codex")).getByTestId("icon-codex")).toBeTruthy();
  });
});
