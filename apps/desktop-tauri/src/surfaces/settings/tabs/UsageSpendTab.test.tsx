import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getUsageSpendSummary: vi.fn(),
  writeUsageSpendExport: vi.fn(),
  getCodexSessionToolActivity: vi.fn(),
  getCodexSessionDetails: vi.fn(),
}));

vi.mock("../../../lib/tauri", () => tauriMocks);
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => import("../../../test/mocks/event"));

const EN: Record<string, string> = {
  CostPeriodMonthToDate: "Month to date",
  CostPeriodAll: "All available history",
  CostPeriodToday: "Today",
  CostPeriodLastDays: "Last {} days",
  CostPeriodShortAll: "All",
  CostPeriodShortDays: "{}d",
  UsageSpendModelsPeriodCaption: "Models for: {}",
  UsageSpendProjectsPeriodCaption: "Projects for: {}",
  UsageSpendEstimatedSpend: "Estimated spend",
  UsageSpendTrackedTokens: "Tracked tokens",
  UsageSpendSources: "Sources",
  UsageSpendTrackCosts: "Track costs",
  UsageSpendTrackingOffTitle: "Cost tracking is off",
  UsageSpendCopyJson: "Copy JSON",
  UsageSpendPeriodLabel: "Time range",
  UsageSpendIndependentChats: "Independent chats",
  UsageSpendIndependentChat: "Independent chat",
  UsageSpendConversations: "conversations",
};
vi.mock("../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => EN[key] ?? key }),
}));

import UsageSpendTab from "./UsageSpendTab";
import type { TabProps } from "../settingsTabs";
import type {
  CodexWorkspacesProjectUsage,
  CodexWorkspacesSessionUsage,
  SettingsSnapshot,
  SpendContract,
  TokenActivitySource,
  UsageSpendRow,
  UsageSpendSummary,
} from "../../../types/bridge";

function contract(reportingPeriod: string): SpendContract {
  return {
    providerId: "codex",
    historyDays: 30,
    reportingPeriod,
    knownCostUsd: 12,
    knownZero: false,
    provenance: "listPriceEstimate",
    priceCoverage: { priced: 1, unpriced: 0, unmetered: 0, estimated: 0 },
    priceCoverageRatio: 1,
    historyCoverageEstablished: true,
    tokenMix: { inputTokens: 700, outputTokens: 200, cacheReadTokens: null, cacheCreationTokens: null, reasoningTokens: null },
    conversationCount: 1,
    models: [{ model: "gpt-5", costUsd: 9, inputTokens: 700, outputTokens: 200, cacheReadTokens: 0, totalTokens: 900, customPricing: false }],
    projects: [],
    conversations: [],
    daily: [],
    hourlyActivity: [],
    projectSourceStatus: null,
    imports: [],
    customPricingActive: false,
  } as SpendContract;
}

function row(providerId: string, displayName: string, periodCost: number | null, periodTokens: number | null, currency = "USD"): UsageSpendRow {
  return {
    providerId,
    displayName,
    sevenDay: 1,
    thirtyDay: 2,
    periodCost,
    periodTokens,
    currency,
    source: "local",
    includedInOverview: true,
  };
}

function summary(reportingPeriod: string, costTrackingEnabled = true): UsageSpendSummary {
  return {
    rows: costTrackingEnabled
      ? [row("codex", "Codex", 9, 900), row("claude", "Claude", 3.5, 2_100), row("zai", "z.ai", 4, 50, "CNY")]
      : [],
    contract: contract(reportingPeriod),
    reportingPeriod,
    reportingDay: "2026-10-11",
    dashboardTimezone: "UTC",
    costTrackingEnabled,
  };
}

function renderTab(settings: Partial<SettingsSnapshot> = {}) {
  const set = vi.fn();
  const props = {
    settings: { costReportingPeriod: "rolling:7", spendDashboardPeriod: "month-to-date", ...settings } as SettingsSnapshot,
    set,
    saving: false,
  } satisfies TabProps;
  render(<UsageSpendTab {...props} />);
  return set;
}

function lastScanPeriod(): string | undefined {
  const calls = tauriMocks.getUsageSpendSummary.mock.calls;
  return calls[calls.length - 1]?.[0]?.period;
}

function stat(label: string, scope: HTMLElement = document.body): string | null | undefined {
  const term = within(scope).getAllByText(label, { selector: "dt" })[0];
  return term.nextElementSibling?.textContent;
}

describe("UsageSpendTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getUsageSpendSummary.mockImplementation(
      async (options?: { period?: string }) => summary(options?.period ?? "rolling:30"),
    );
    Object.assign(navigator, { clipboard: { writeText: vi.fn().mockResolvedValue(undefined) } });
  });

  it("scans the dashboard period, not the menu History window, and saves a new segment", async () => {
    const set = renderTab();

    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledTimes(1));
    expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledWith({ period: "month-to-date", forceRefresh: false });
    const range = screen.getByRole("radiogroup", { name: "Time range" });
    expect(within(range).getByRole("radio", { name: "Month to date" }).getAttribute("aria-checked")).toBe("true");

    fireEvent.click(within(range).getByRole("radio", { name: "90d" }));
    expect(set).toHaveBeenCalledWith({ spendDashboardPeriod: "rolling:90" });
  });

  it("copies the menu History window when no dashboard period is saved", async () => {
    renderTab({ spendDashboardPeriod: undefined, costReportingPeriod: "all" });
    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalled());
    expect(lastScanPeriod()).toBe("all");
  });

  it("keeps a legacy custom window as its own active segment", async () => {
    renderTab({ spendDashboardPeriod: "rolling:14" });
    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalled());
    expect(lastScanPeriod()).toBe("rolling:14");
    const range = screen.getByRole("radiogroup", { name: "Time range" });
    expect(within(range).getAllByRole("radio")).toHaveLength(6);
    expect(within(range).getByRole("radio", { name: "14d" }).getAttribute("aria-checked")).toBe("true");
  });

  it("sums the stat strip across providers per currency", async () => {
    renderTab();
    await screen.findByText("USD");
    const usd = screen.getByText("USD").closest(".usage-spend__currency") as HTMLElement;
    const cny = screen.getByText("CNY").closest(".usage-spend__currency") as HTMLElement;

    expect(stat("Estimated spend", usd)).toBe("$12.50");
    expect(stat("Tracked tokens", usd)).toBe("3K");
    expect(stat("Sources", usd)).toBe("2");
    expect(stat("Sources", cny)).toBe("1");
  });

  it("leaves hidden sources out of the strip and out of Copy JSON", async () => {
    const set = renderTab({ spendDashboardHiddenSourceIds: ["claude"] });
    await screen.findByText("USD");
    const usd = screen.getByText("USD").closest(".usage-spend__currency") as HTMLElement;
    expect(stat("Estimated spend", usd)).toBe("$9.00");
    expect(stat("Sources", usd)).toBe("1");

    // The hidden source stays in the filter, unchecked, so it can be shown again.
    const claude = screen.getByRole("checkbox", { name: "Claude" }) as HTMLInputElement;
    expect(claude.checked).toBe(false);
    fireEvent.click(claude);
    expect(set).toHaveBeenCalledWith({ spendDashboardHiddenSourceIds: [] });

    fireEvent.click(screen.getByRole("button", { name: "Copy JSON" }));
    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalledTimes(1));
    const copied = JSON.parse(vi.mocked(navigator.clipboard.writeText).mock.calls[0][0]) as UsageSpendSummary;
    expect(copied.rows.map((entry) => entry.providerId)).toEqual(["codex", "zai"]);
  });

  it("draws the trend explorer from the published trend without hidden sources", async () => {
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) => ({
      ...summary(options?.period ?? "rolling:30"),
      trend: {
        startDay: "2026-10-05",
        endDay: "2026-10-11",
        timeZone: "UTC",
        sources: [
          { id: "claude", providerId: "claude", displayName: "Claude", knownZero: false },
          { id: "codex", providerId: "codex", displayName: "Codex", knownZero: false },
        ],
        daily: [
          { sourceId: "claude", day: "2026-10-07", costUsd: 3.5 },
          { sourceId: "codex", day: "2026-10-08", costUsd: 9 },
        ],
        hourly: [],
        hourlyDays: [],
      },
    }));
    renderTab({ spendDashboardHiddenSourceIds: ["claude"] });
    const panel = await screen.findByTestId("spend-trend");
    expect(within(panel).getByTestId("spend-trend-source-codex")).toBeTruthy();
    expect(within(panel).queryByTestId("spend-trend-source-claude")).toBeNull();
    expect(within(panel).getByTestId("spend-trend-summary").textContent).toContain("$9.00");
  });

  it("blanks the Codex detail when Codex is hidden", async () => {
    renderTab({ spendDashboardHiddenSourceIds: ["codex"] });
    await screen.findByText("USD");
    expect(screen.queryByText("UsageSpendTokenInput")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Copy JSON" }));
    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalled());
    const copied = JSON.parse(vi.mocked(navigator.clipboard.writeText).mock.calls[0][0]) as UsageSpendSummary;
    expect(copied.rows.map((entry) => entry.providerId)).toEqual(["claude", "zai"]);
    expect(copied.contract.models).toEqual([]);
    expect(copied.contract.knownCostUsd).toBeNull();
  });

  it("shows Cost tracking is off and keeps the switch to turn it back on", async () => {
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) =>
      summary(options?.period ?? "rolling:30", false),
    );
    const set = renderTab({ costUsageEnabled: false });

    expect(await screen.findByText("Cost tracking is off")).toBeTruthy();
    expect(screen.queryByText("Estimated spend")).toBeNull();
    expect(screen.queryByRole("radiogroup", { name: "Time range" })).toBeNull();

    const toggle = screen.getByRole("checkbox", { name: "Track costs" }) as HTMLInputElement;
    expect(toggle.checked).toBe(false);
    fireEvent.click(toggle);
    expect(set).toHaveBeenCalledWith({ costUsageEnabled: true });
  });

  it("hides the OpenCodex switches and the Sources filter while Track costs is off (#825)", async () => {
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) =>
      summary(options?.period ?? "rolling:30", false),
    );
    renderTab({ costUsageEnabled: false, openCodexUsageLogsEnabled: true });
    expect(await screen.findByText("Cost tracking is off")).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: "Track costs" })).not.toBeChecked();
    expect(screen.queryByText("UsageSpendOpenCodexImport")).toBeNull();
    expect(screen.queryByText("UsageSpendHideNativeCodex")).toBeNull();
    expect(screen.queryByRole("group", { name: "Sources" })).toBeNull();
    expect(screen.queryByRole("checkbox", { name: "Codex" })).toBeNull();
  });

  it("shows the OpenCodex switches and the Sources filter while Track costs is on", async () => {
    renderTab({ costUsageEnabled: true, openCodexUsageLogsEnabled: true });
    await screen.findByText("USD");
    expect(screen.getByText("UsageSpendOpenCodexImport")).toBeInTheDocument();
    expect(screen.getByText("UsageSpendHideNativeCodex")).toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Sources" })).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Codex" })).toBeChecked();
  });

  it("leaves the History window and Cost summary pickers to Settings > Menu", async () => {
    renderTab();
    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledTimes(1));

    expect(screen.queryByLabelText("CostPeriodHistoryWindow")).toBeNull();
    expect(screen.queryByText("CostSummaryDisplayStyle")).toBeNull();
  });

  it("lists independent chats in their own region, apart from the project rows", async () => {
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) =>
      withContract(summary(options?.period ?? "rolling:30"), {
        projects: [usageProject("repo", "Win-CodexBar", 3), usageProject("chat-1", "Repair the local service", 2, true)],
      }),
    );
    renderTab();
    await screen.findByText("USD");
    fireEvent.click(screen.getByRole("tab", { name: "UsageSpendProjects" }));

    const chats = await screen.findByRole("region", { name: "Independent chats" });
    expect(within(chats).getByText("Repair the local service")).toBeInTheDocument();
    expect(within(chats).queryByText("Win-CodexBar")).toBeNull();
    expect(screen.getByText("Win-CodexBar").closest('[role="region"]')).toBeNull();
  });

  it("labels each independent chat row with its conversation count", async () => {
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) =>
      withContract(summary(options?.period ?? "rolling:30"), {
        projects: [usageProject("chat-1", "Repair the local service", 2, true)],
      }),
    );
    renderTab();
    await screen.findByText("USD");
    fireEvent.click(screen.getByRole("tab", { name: "UsageSpendProjects" }));

    const chats = await screen.findByRole("region", { name: "Independent chats" });
    expect(within(chats).getByText(/^Independent chat · 2 conversations/)).toBeInTheDocument();
  });

  it("expands an independent chat to its own conversations", async () => {
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) =>
      withContract(summary(options?.period ?? "rolling:30"), {
        projects: [usageProject("chat-1", "Repair the local service", 1, true)],
        conversations: [chatSession("s-1", "chat-1", "Restart the daemon"), chatSession("s-2", "other", "Elsewhere")],
      }),
    );
    renderTab();
    await screen.findByText("USD");
    fireEvent.click(screen.getByRole("tab", { name: "UsageSpendProjects" }));

    const chats = await screen.findByRole("region", { name: "Independent chats" });
    expect(within(chats).queryByText("Restart the daemon")).toBeNull();
    fireEvent.click(within(chats).getByRole("button", { name: /Repair the local service/ }));
    expect(within(chats).getByText("Restart the daemon")).toBeInTheDocument();
    expect(within(chats).queryByText("Elsewhere")).toBeNull();
  });

  it("reads a conversation's tool activity over the contract's history window", async () => {
    tauriMocks.getCodexSessionDetails.mockResolvedValue({ sessionId: "s-1", requestCount: 0, turns: [] });
    tauriMocks.getCodexSessionToolActivity.mockResolvedValue({
      sessionId: "s-1",
      operations: [],
      ignoredRecordCount: 0,
      isPartial: false,
      redacted: false,
    });
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) =>
      withContract(summary(options?.period ?? "rolling:30"), {
        conversations: [chatSession("s-1", "chat-1", "Restart the daemon")],
      }),
    );
    renderTab();
    await screen.findByText("USD");
    fireEvent.click(screen.getByRole("tab", { name: "UsageSpendViewSessions" }));

    expect(tauriMocks.getCodexSessionToolActivity).not.toHaveBeenCalled();
    fireEvent.click(await screen.findByRole("button", { name: /Restart the daemon/ }));
    fireEvent.click(await screen.findByRole("button", { name: "UsageSpendToolsTitle" }));
    await waitFor(() => expect(tauriMocks.getCodexSessionToolActivity).toHaveBeenCalledWith("s-1", 30));
    expect(await screen.findByText("UsageSpendToolsEmpty")).toBeInTheDocument();
  });

  it("captions a project with its path only when another project shares its name", async () => {
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) =>
      withContract(summary(options?.period ?? "rolling:30"), {
        projects: [
          usageProject("p1", "app", 3, false, "C:/work/app"),
          usageProject("p2", "app", 2, false, "D:/personal/app"),
          usageProject("p3", "Win-CodexBar", 1, false, "C:/code/Win-CodexBar"),
        ],
      }),
    );
    renderTab();
    await screen.findByText("USD");
    fireEvent.click(screen.getByRole("tab", { name: "UsageSpendProjects" }));

    expect(await screen.findByText("C:/work/app")).toBeInTheDocument();
    expect(screen.getByText("D:/personal/app")).toBeInTheDocument();
    expect(screen.queryByText("C:/code/Win-CodexBar")).toBeNull();
  });

  it("hides project paths under Hide personal information", async () => {
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) =>
      withContract(summary(options?.period ?? "rolling:30"), {
        projects: [
          usageProject("p1", "app", 3, false, "C:/work/app"),
          usageProject("p2", "app", 2, false, "D:/personal/app"),
        ],
      }),
    );
    renderTab({ hidePersonalInfo: true });
    await screen.findByText("USD");
    fireEvent.click(screen.getByRole("tab", { name: "UsageSpendProjects" }));

    expect(await screen.findAllByText("app")).toHaveLength(2);
    expect(screen.queryByText("C:/work/app")).toBeNull();
    expect(screen.queryByText("D:/personal/app")).toBeNull();
    expect(document.querySelector('[title*="C:/work"]')).toBeNull();
  });

  it("shows the numbered privacy labels without paths", async () => {
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) =>
      withContract(summary(options?.period ?? "rolling:30"), {
        projects: [usageProject("p1", "Workspace #1", 3), usageProject("p2", "Workspace #2", 2)],
      }),
    );
    renderTab({ hidePersonalInfo: true });
    await screen.findByText("USD");
    fireEvent.click(screen.getByRole("tab", { name: "UsageSpendProjects" }));

    expect(await screen.findByText("Workspace #1")).toBeInTheDocument();
    expect(screen.getByText("Workspace #2")).toBeInTheDocument();
    expect(document.querySelectorAll(".usage-spend__project-path")).toHaveLength(0);
  });

  it("leaves the chats region out when no independent chat was found", async () => {
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) =>
      withContract(summary(options?.period ?? "rolling:30"), { projects: [usageProject("repo", "Win-CodexBar", 3)] }),
    );
    renderTab();
    await screen.findByText("USD");
    fireEvent.click(screen.getByRole("tab", { name: "UsageSpendProjects" }));

    expect(await screen.findByText("Win-CodexBar")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Independent chats" })).toBeNull();
  });

  it("draws the token activity year from visible sources only and keeps it out of Copy JSON", async () => {
    const activity = (providerId: string, tokens: number): TokenActivitySource => ({
      providerId,
      days: [
        { day: "2026-10-10", totalTokens: tokens, isScanned: true },
        { day: "2026-10-11", totalTokens: tokens, isScanned: true },
      ],
    });
    tauriMocks.getUsageSpendSummary.mockImplementation(async (options?: { period?: string }) => ({
      ...summary(options?.period ?? "rolling:30"),
      tokenActivity: [activity("codex", 1_000), activity("claude", 50_000)],
    }));
    renderTab({ spendDashboardHiddenSourceIds: ["claude"] });

    const grid = await screen.findByTestId("token-activity");
    expect(within(grid).getByRole("heading", { name: "TokenActivityTitle" })).toBeInTheDocument();
    const cell = grid.querySelector("[data-day='2026-10-11'] title");
    expect(cell?.textContent).toMatch(/: 1K$/);

    fireEvent.click(screen.getByRole("button", { name: "Copy JSON" }));
    await waitFor(() => expect(navigator.clipboard.writeText).toHaveBeenCalledTimes(1));
    const copied = JSON.parse(vi.mocked(navigator.clipboard.writeText).mock.calls[0][0]) as UsageSpendSummary;
    expect(copied.tokenActivity).toBeUndefined();
  });

  it("leaves the token activity grid out without activity", async () => {
    renderTab();
    await screen.findByText("USD");
    expect(screen.queryByTestId("token-activity")).toBeNull();
  });
});

function withContract(base: UsageSpendSummary, overrides: Partial<SpendContract>): UsageSpendSummary {
  return { ...base, contract: { ...base.contract, ...overrides } };
}

function usageProject(
  id: string,
  displayName: string,
  sessionCount: number,
  isProjectless = false,
  path: string | null = null,
): CodexWorkspacesProjectUsage {
  return {
    id,
    displayName,
    path,
    totals: { inputTokens: 10, cachedInputTokens: 0, outputTokens: 5, totalTokens: 15 },
    costEstimate: { knownUsd: 1.5, unknownTokens: 0 },
    sessionCount,
    latestActivity: null,
    topModel: "gpt-5",
    topSessions: [],
    isProjectless,
  };
}

function chatSession(id: string, projectId: string, displayTitle: string): CodexWorkspacesSessionUsage {
  return {
    id,
    projectId,
    displayTitle,
    cwd: null,
    startedAt: null,
    latestActivity: null,
    totals: { inputTokens: 10, cachedInputTokens: 0, outputTokens: 5, totalTokens: 15 },
    costEstimate: { knownUsd: 0.5, unknownTokens: 0 },
    topModel: "gpt-5",
    turnPerformance: null,
  };
}
