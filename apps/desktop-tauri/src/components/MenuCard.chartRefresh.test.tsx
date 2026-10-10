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
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

import { LocaleProvider } from "../i18n/LocaleProvider";
import { makeRateWindow, makeUsageSnapshot } from "../test/fixtures";
import { buildBundle } from "../test/localeHarness";
import type { ProviderUsageSnapshot } from "../types/bridge";
import MenuCard from "./MenuCard";

const window0 = makeRateWindow(10, { reserveWillLastToReset: false, reserveEtaSeconds: null });

function snapshot(updatedAt: string): ProviderUsageSnapshot {
  return makeUsageSnapshot("claude", {
    displayName: "Claude",
    primary: window0,
    selectedMetric: window0,
    sourceLabel: "oauth",
    updatedAt,
    errorState: "unknown",
    fetchDurationMs: null,
  });
}

function card(updatedAt: string) {
  return (
    <LocaleProvider>
      <MenuCard
        provider={snapshot(updatedAt)}
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

  it("re-reads chart data only when the local day changes across an update", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    try {
      vi.setSystemTime(new Date(2026, 9, 7, 23, 0, 0));
      const { rerender } = render(card("2026-10-07T23:00:00Z"));
      await waitFor(() => expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(1));

      // Plain rerender and a same-day provider update do not refetch.
      rerender(card("2026-10-07T23:00:00Z"));
      vi.setSystemTime(new Date(2026, 9, 7, 23, 30, 0));
      rerender(card("2026-10-07T23:30:00Z"));
      expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(1);

      // The next update lands after local midnight; Today must be re-read.
      vi.setSystemTime(new Date(2026, 9, 8, 0, 5, 0));
      rerender(card("2026-10-08T00:05:00Z"));
      await waitFor(() => expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(2));
    } finally {
      vi.useRealTimers();
    }
  });
});
