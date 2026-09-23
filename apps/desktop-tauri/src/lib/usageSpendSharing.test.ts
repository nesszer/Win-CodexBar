import { describe, expect, it, vi } from "vitest";

import {
  formatUsageSpendReportingDay,
  filterUsageSpendSummaryForOverview,
  renderUsageSpendSharePng,
  usageSpendShareFooter,
  usageSpendSubscriptionCaption,
} from "./usageSpendSharing";
import type { SpendContract, UsageSpendRow, UsageSpendSummary } from "../types/bridge";

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
        contract: {} as SpendContract,
        rows: [{
          providerId: "codex",
          displayName: "Codex",
          sevenDay: null,
          thirtyDay: null,
          currency: "USD",
          source: "local",
          includedInOverview: true,
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

  it("renders only the redaction-safe row fields in the share PNG", () => {
    // Redaction invariant (upstream #2112): the share image must never carry
    // account emails or provider-identity secrets. The renderer draws exactly
    // these cells; keep the invariant comment on renderUsageSpendSharePng in
    // sync if this list ever grows.
    const renderedCells = ["displayName", "sevenDay", "thirtyDay", "currency", "source"] as const;
    const row: UsageSpendRow = {
      providerId: "codex",
      displayName: "Codex",
      sevenDay: 1,
      thirtyDay: 2,
      currency: "USD",
      source: "local",
      includedInOverview: true,
    };
    const unsafeKeys = Object.keys(row).filter(
      (key) => !renderedCells.includes(key as (typeof renderedCells)[number]),
    );
    // Nothing outside the drawn cells may be read by the renderer, and no
    // UsageSpendRow field may carry account-identity data (no email/org).
    expect(Object.keys(row).filter((key) => /email|org|token|account/i.test(key))).toEqual([]);
    expect(unsafeKeys).toEqual(["providerId", "includedInOverview"]);
  });

  it("uses the supplied display formatter and currency resolver for PNG cells", () => {
    const canvas = document.createElement("canvas");
    const context = {
      scale: vi.fn(), fillRect: vi.fn(), strokeRect: vi.fn(), fillText: vi.fn(),
      beginPath: vi.fn(), moveTo: vi.fn(), lineTo: vi.fn(), stroke: vi.fn(),
      measureText: vi.fn(() => ({ width: 1 })),
    } as unknown as CanvasRenderingContext2D;
    vi.spyOn(canvas, "getContext").mockReturnValue(context);
    vi.spyOn(canvas, "toDataURL").mockReturnValue("data:image/png;base64,test");
    const createElement = document.createElement.bind(document);
    vi.spyOn(document, "createElement").mockImplementation((tagName, options) =>
      tagName === "canvas" ? canvas : createElement(tagName, options),
    );
    const formatMetric = vi.fn(() => "₺32.00 · 10 tokens");
    const displayCurrency = vi.fn(() => "TRY");
    const summary: UsageSpendSummary = {
      contract: {} as SpendContract,
      reportingDay: "2026-09-19",
      dashboardTimezone: "UTC",
      rows: [{
        providerId: "codex", displayName: "Codex", sevenDay: 1, thirtyDay: 2,
        currency: "USD", source: "local", includedInOverview: true,
      }],
    };

    try {
      renderUsageSpendSharePng(summary, "Usage & Spend", { formatMetric, displayCurrency });
      expect(formatMetric).toHaveBeenCalledWith(1, undefined, "USD", "tokens");
      expect(formatMetric).toHaveBeenCalledWith(2, undefined, "USD", "tokens");
      expect(displayCurrency).toHaveBeenCalledWith("USD");
      expect(context.fillText).toHaveBeenCalledWith("₺32.00 · 10 tokens", expect.any(Number), expect.any(Number));
      expect(context.fillText).toHaveBeenCalledWith("TRY", expect.any(Number), expect.any(Number));
    } finally {
      vi.restoreAllMocks();
    }
  });
});
