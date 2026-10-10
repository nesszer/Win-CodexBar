import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getSettingsSnapshot: vi.fn(),
  getUsageSpendSummary: vi.fn(),
  updateSettings: vi.fn(),
  writeUsageSpendExport: vi.fn(),
}));

vi.mock("../../../lib/tauri", () => tauriMocks);
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => import("../../../test/mocks/event"));

const EN: Record<string, string> = {
  CostPeriodMonthToDate: "Month to date",
  CostPeriodAll: "All available history",
  CostPeriodToday: "Today",
  CostPeriodLastDays: "Last {} days",
  CostPeriodCustom: "Custom days",
  UsageSpendModelsPeriodCaption: "Models for: {}",
  UsageSpendProjectsPeriodCaption: "Projects for: {}",
};
vi.mock("../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => EN[key] ?? key }),
}));

import UsageSpendTab from "./UsageSpendTab";
import type { TabProps } from "../settingsTabs";
import type { SpendContract, UsageSpendSummary } from "../../../types/bridge";

function contract(reportingPeriod: string): SpendContract {
  return {
    providerId: "codex",
    historyDays: 30,
    reportingPeriod,
    knownCostUsd: 12,
    knownZero: false,
    provenance: "listPriceEstimate",
    priceCoverage: { priced: 1, unpriced: 0 },
    priceCoverageRatio: 1,
    historyCoverageEstablished: true,
    tokenMix: {},
    conversationCount: 1,
    models: [],
    projects: [],
    conversations: [],
    daily: [],
    hourlyActivity: [],
    projectSourceStatus: null,
    imports: [],
    customPricingActive: false,
  } as unknown as SpendContract;
}

function summary(reportingPeriod: string): UsageSpendSummary {
  return {
    rows: [
      {
        providerId: "codex",
        displayName: "Codex",
        sevenDay: 1,
        thirtyDay: 2,
        periodCost: 9,
        periodTokens: 900,
        currency: "USD",
        source: "local",
        includedInOverview: true,
      },
    ],
    contract: contract(reportingPeriod),
    reportingPeriod,
    reportingDay: "2026-09-29",
    dashboardTimezone: "UTC",
  } as unknown as UsageSpendSummary;
}

const props = {} as TabProps;

function lastScanPeriod(): string | undefined {
  const calls = tauriMocks.getUsageSpendSummary.mock.calls;
  return calls[calls.length - 1]?.[0]?.period;
}

describe("UsageSpendTab History window", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getSettingsSnapshot.mockResolvedValue({
      costReportingPeriod: "month-to-date",
      costSummaryDisplayStyle: "compact",
    });
    tauriMocks.updateSettings.mockResolvedValue({});
    tauriMocks.getUsageSpendSummary.mockImplementation(
      async (options?: { period?: string }) => summary(options?.period ?? "rolling:30"),
    );
  });

  it("scans once with the saved window and shows it selected", async () => {
    render(<UsageSpendTab {...props} />);

    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledTimes(1));
    expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledWith({
      period: "month-to-date",
      forceRefresh: false,
    });
    const select = screen.getByLabelText("CostPeriodHistoryWindow") as HTMLSelectElement;
    expect(select.value).toBe("month-to-date");
    // The period column is labelled with the window and shows its numbers.
    expect(await screen.findByRole("columnheader", { name: "Month to date" })).toBeTruthy();
    expect(screen.getByText(/900/)).toBeTruthy();
    expect(screen.getByText("Models for: Month to date")).toBeTruthy();
  });

  it("migrates a settings payload without a saved window to 30 days", async () => {
    tauriMocks.getSettingsSnapshot.mockResolvedValue({ costSummaryDisplayStyle: "compact" });
    render(<UsageSpendTab {...props} />);

    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalled());
    expect(lastScanPeriod()).toBe("rolling:30");
    expect((screen.getByLabelText("CostPeriodHistoryWindow") as HTMLSelectElement).value).toBe(
      "rolling:30",
    );
  });

  it("falls back to 30 days when the saved window is unreadable", async () => {
    tauriMocks.getSettingsSnapshot.mockResolvedValue({
      costReportingPeriod: "rolling:0",
      costSummaryDisplayStyle: "compact",
    });
    render(<UsageSpendTab {...props} />);

    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalled());
    expect(lastScanPeriod()).toBe("rolling:30");
  });

  it("persists a preset choice and rescans with it", async () => {
    render(<UsageSpendTab {...props} />);
    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByLabelText("CostPeriodHistoryWindow"), {
      target: { value: "rolling:90" },
    });

    await waitFor(() =>
      expect(tauriMocks.updateSettings).toHaveBeenCalledWith({ costReportingPeriod: "rolling:90" }),
    );
    await waitFor(() => expect(lastScanPeriod()).toBe("rolling:90"));
    expect(await screen.findByRole("columnheader", { name: "Last 90 days" })).toBeTruthy();
  });

  it("offers a custom day count and persists one valid value on blur", async () => {
    render(<UsageSpendTab {...props} />);
    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByLabelText("CostPeriodHistoryWindow"), {
      target: { value: "custom" },
    });
    const input = screen.getByLabelText("CostPeriodCustomDays") as HTMLInputElement;

    fireEvent.change(input, { target: { value: "400" } });
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(tauriMocks.updateSettings).not.toHaveBeenCalled();

    fireEvent.change(input, { target: { value: "0" } });
    expect(tauriMocks.updateSettings).not.toHaveBeenCalled();

    // Typing "1" on the way to "14" saves nothing and keeps the input open.
    fireEvent.change(input, { target: { value: "1" } });
    expect(tauriMocks.updateSettings).not.toHaveBeenCalled();
    expect(screen.getByLabelText("CostPeriodCustomDays")).toBe(input);

    fireEvent.change(input, { target: { value: "14" } });
    expect(tauriMocks.updateSettings).not.toHaveBeenCalled();
    fireEvent.blur(input);
    await waitFor(() =>
      expect(tauriMocks.updateSettings).toHaveBeenCalledWith({ costReportingPeriod: "rolling:14" }),
    );
    expect(tauriMocks.updateSettings).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(lastScanPeriod()).toBe("rolling:14"));
    expect(input.getAttribute("aria-invalid")).toBe("false");
    expect(await screen.findByRole("columnheader", { name: "Last 14 days" })).toBeTruthy();
  });

  it("commits a valid custom day count on Enter", async () => {
    render(<UsageSpendTab {...props} />);
    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByLabelText("CostPeriodHistoryWindow"), {
      target: { value: "custom" },
    });
    const input = screen.getByLabelText("CostPeriodCustomDays") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "45" } });
    expect(tauriMocks.updateSettings).not.toHaveBeenCalled();

    fireEvent.keyDown(input, { key: "Enter" });
    await waitFor(() =>
      expect(tauriMocks.updateSettings).toHaveBeenCalledWith({ costReportingPeriod: "rolling:45" }),
    );
    expect(tauriMocks.updateSettings).toHaveBeenCalledTimes(1);
  });

  it("starts in custom mode for a saved count that is not a preset", async () => {
    tauriMocks.getSettingsSnapshot.mockResolvedValue({
      costReportingPeriod: "rolling:14",
      costSummaryDisplayStyle: "compact",
    });
    render(<UsageSpendTab {...props} />);

    const input = (await screen.findByLabelText("CostPeriodCustomDays")) as HTMLInputElement;
    expect(input.value).toBe("14");
    expect((screen.getByLabelText("CostPeriodHistoryWindow") as HTMLSelectElement).value).toBe(
      "custom",
    );
  });

  it("reverts the picker and shows the error when saving fails", async () => {
    tauriMocks.updateSettings.mockRejectedValue(new Error("Invalid cost reporting period: x"));
    render(<UsageSpendTab {...props} />);
    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledTimes(1));

    fireEvent.change(screen.getByLabelText("CostPeriodHistoryWindow"), {
      target: { value: "all" },
    });

    expect(await screen.findByText("Invalid cost reporting period: x")).toBeTruthy();
    await waitFor(() =>
      expect((screen.getByLabelText("CostPeriodHistoryWindow") as HTMLSelectElement).value).toBe(
        "month-to-date",
      ),
    );
  });
});
