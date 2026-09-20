import { describe, expect, it } from "vitest";

import {
  formatUsageSpendReportingDay,
  filterUsageSpendSummaryForOverview,
  usageSpendShareFooter,
  usageSpendSubscriptionCaption,
} from "./usageSpendSharing";
import type { SpendContract, UsageSpendSummary } from "../types/bridge";

describe("usage spend sharing", () => {
  it.each([
    [0, "0 subscriptions"],
    [1, "1 subscription"],
    [2, "2 subscriptions"],
    [12, "12 subscriptions"],
  ])("uses the correct subscription caption for %i", (count, expected) => {
    expect(usageSpendSubscriptionCaption(count)).toBe(expected);
  });

  it("preserves the reporting day across dashboard timezones", () => {
    expect(formatUsageSpendReportingDay("2026-09-19", "Pacific/Kiritimati")).toBe("Sep 19, 2026");
    expect(formatUsageSpendReportingDay("2026-09-19", "America/Los_Angeles")).toBe("Sep 19, 2026");
    expect(formatUsageSpendReportingDay("2026-10-01", "Pacific/Norfolk")).toBe("Oct 1, 2026");
  });

  it("falls back for an invalid dashboard timezone or date", () => {
    expect(formatUsageSpendReportingDay("2026-10-01", "Not/AZone")).toBe("2026-10-01");
    expect(formatUsageSpendReportingDay("2026-02-31", "UTC")).toBe("2026-02-31");
  });

  it("builds the report footer from the included day and row count", () => {
    expect(
      usageSpendShareFooter({
        rows: [{
          providerId: "codex",
          displayName: "Codex",
          sevenDay: null,
          thirtyDay: null,
          currency: "USD",
          source: "local",
        }],
        reportingDay: "2026-09-19",
        dashboardTimezone: "UTC",
      }),
    ).toBe("Data through Sep 19, 2026 · 1 subscription");
  });

  it("keeps hidden sources out of the Overview share summary", () => {
    const summary: UsageSpendSummary = {
      contract: {} as SpendContract,
      reportingDay: "2026-09-19",
      dashboardTimezone: "UTC",
      rows: [
        {
          providerId: "codex",
          displayName: "Codex",
          sevenDay: 1,
          thirtyDay: 2,
          currency: "USD",
          source: "local",
          includedInOverview: true,
        },
        {
          providerId: "claude",
          displayName: "Claude",
          sevenDay: 3,
          thirtyDay: 4,
          currency: "USD",
          source: "hidden",
          includedInOverview: false,
        },
      ],
    };

    expect(filterUsageSpendSummaryForOverview(summary).rows.map((row) => row.providerId)).toEqual([
      "codex",
    ]);
  });
});
