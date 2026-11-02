import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getProviderChartData: vi.fn(),
  getDeepSeekPricingStatus: vi.fn(),
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
  claudeAccountsList: vi.fn(),
  getCodexAccountsState: vi.fn(),
}));

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  ...tauriMocks,
}));
vi.mock("@tauri-apps/api/event", () => import("../test/mocks/event"));

import { LocaleProvider } from "../i18n/LocaleProvider";
import { makeRateWindow, makeUsageSnapshot } from "../test/fixtures";
import { buildBundle } from "../test/localeHarness";
import type { ProviderUsageSnapshot } from "../types/bridge";
import MenuCard from "./MenuCard";

function rateWindow(used: number) {
  return makeRateWindow(used, { reserveWillLastToReset: false, reserveEtaSeconds: null });
}

function snapshot(updatedAt: string, used: number): ProviderUsageSnapshot {
  return makeUsageSnapshot("claude", {
    displayName: "Claude",
    primary: rateWindow(used),
    selectedMetric: rateWindow(used),
    sourceLabel: "oauth",
    updatedAt,
    errorState: "unknown",
    fetchDurationMs: null,
  });
}

function card(updatedAt: string, used = 10) {
  return (
    <LocaleProvider>
      <MenuCard
        provider={snapshot(updatedAt, used)}
        display={{ hideEmail: false, resetTimeRelative: true }}
      />
    </LocaleProvider>
  );
}

describe("MenuCard chart refresh", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.claudeAccountsList.mockResolvedValue([]);
    tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle({}));
    tauriMocks.getDeepSeekPricingStatus.mockResolvedValue(null);
    tauriMocks.getProviderChartData.mockResolvedValue({
      providerId: "claude",
      costHistory: [],
      tokensHistory: [],
      creditsHistory: [],
      usageBreakdown: [],
      localUsage: null,
    });
  });

  it("re-reads chart data after same-day usage updates and across midnight", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      vi.setSystemTime(new Date(2026, 9, 7, 23, 0, 0));
      const { rerender } = render(card("2026-10-07T23:00:00Z"));
      await waitFor(() => expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(1));

      // A plain rerender must not start a second scan.
      rerender(card("2026-10-07T23:00:00Z"));
      expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(1);
      // A refresh that only moves the timestamp must not rescan either.
      vi.setSystemTime(new Date(2026, 9, 7, 23, 15, 0));
      rerender(card("2026-10-07T23:15:00Z"));
      expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(1);
      // Published usage changes must reach the chart on the same day too.
      vi.setSystemTime(new Date(2026, 9, 7, 23, 30, 0));
      rerender(card("2026-10-07T23:30:00Z", 12));
      await waitFor(() => expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(2));

      // The next update lands after local midnight; Today must be re-read.
      vi.setSystemTime(new Date(2026, 9, 8, 0, 5, 0));
      rerender(card("2026-10-08T00:05:00Z", 12));
      await waitFor(() => expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(3));
    } finally {
      vi.useRealTimers();
    }
  });
});
