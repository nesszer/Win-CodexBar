import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LocaleKey } from "../../../i18n/keys";
import type { CodexSessionDetails, CodexWorkspacesSessionUsage } from "../../../types/bridge";

const tauriMocks = vi.hoisted(() => ({
  getCodexSessionDetails: vi.fn(),
  getCodexSessionToolActivity: vi.fn(),
}));
vi.mock("../../../lib/tauri", () => tauriMocks);

import SessionInspector from "./SessionInspector";

const EN: Partial<Record<LocaleKey, string>> = {
  UsageSpendTurnNumber: "Turn {}",
  UsageSpendRequestNumber: "Request {}",
};
const t = (key: LocaleKey) => EN[key] ?? key;

function session(
  id: string,
  displayTitle: string,
  latestActivity: string | null,
  totalTokens = 100,
): CodexWorkspacesSessionUsage {
  return {
    id,
    projectId: "p",
    displayTitle,
    cwd: null,
    startedAt: null,
    latestActivity,
    totals: { inputTokens: totalTokens, cachedInputTokens: 0, outputTokens: 0, totalTokens },
    costEstimate: { knownUsd: 0.25, unknownTokens: 0 },
    topModel: "gpt-5",
    turnPerformance: null,
  };
}

const SESSIONS = [
  session("s-1", "Older work", "2026-01-01T10:00:00Z", 900),
  session("s-2", "Newest work", "2026-01-03T10:00:00Z", 10),
  session("s-3", "Middle work", "2026-01-02T10:00:00Z", 500),
];

function details(sessionId: string): CodexSessionDetails {
  const request = {
    modelName: "gpt-5",
    timestamp: "2026-01-01T10:00:00Z",
    inputTokens: 60,
    cachedInputTokens: 10,
    outputTokens: 40,
    reasoningTokens: 5,
    totalTokens: 100,
    costUsd: 0.25,
  };
  return {
    sessionId,
    requestCount: 2,
    turns: [
      {
        turnId: "turn-a",
        firstActivity: "2026-01-01T10:00:00Z",
        lastActivity: "2026-01-01T10:00:00Z",
        inputTokens: 60,
        cachedInputTokens: 10,
        outputTokens: 40,
        reasoningTokens: 5,
        totalTokens: 100,
        requestCount: 1,
        costUsd: 0.25,
        requests: [{ id: "request-1", ...request }],
      },
      {
        turnId: null,
        firstActivity: null,
        lastActivity: null,
        inputTokens: 1,
        cachedInputTokens: 0,
        outputTokens: 0,
        reasoningTokens: null,
        totalTokens: 1,
        requestCount: 1,
        costUsd: null,
        requests: [{ id: "request-2", ...request, timestamp: null, costUsd: null }],
      },
    ],
  };
}

function titles() {
  return within(screen.getByRole("list", { name: "UsageSpendViewSessions" }))
    .getAllByRole("listitem")
    .map((item) => item.querySelector("strong")?.textContent);
}

describe("SessionInspector", () => {
  beforeEach(() => {
    tauriMocks.getCodexSessionDetails.mockReset();
    tauriMocks.getCodexSessionToolActivity.mockReset();
    tauriMocks.getCodexSessionToolActivity.mockResolvedValue({
      sessionId: "s-1",
      operations: [],
      ignoredRecordCount: 0,
      isPartial: false,
      redacted: false,
    });
  });

  it("lists sessions newest first and loads no details until one is selected", () => {
    render(<SessionInspector conversations={SESSIONS} historyDays={30} t={t} />);
    expect(titles()).toEqual(["Newest work", "Middle work", "Older work"]);
    expect(screen.getByText("UsageSpendSelectSession")).toBeInTheDocument();
    expect(tauriMocks.getCodexSessionDetails).not.toHaveBeenCalled();
  });

  it("searches, sorts by tokens and flips the direction", () => {
    render(<SessionInspector conversations={SESSIONS} historyDays={30} t={t} />);
    fireEvent.change(screen.getByRole("searchbox", { name: "UsageSpendSearchSessions" }), {
      target: { value: "old" },
    });
    expect(titles()).toEqual(["Older work"]);
    fireEvent.change(screen.getByRole("searchbox", { name: "UsageSpendSearchSessions" }), {
      target: { value: "nothing" },
    });
    expect(screen.getByText("UsageSpendNoMatchingSessions")).toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox", { name: "UsageSpendSearchSessions" }), {
      target: { value: "" },
    });

    fireEvent.change(screen.getByRole("combobox", { name: "UsageSpendSortBy" }), { target: { value: "tokens" } });
    expect(titles()).toEqual(["Older work", "Middle work", "Newest work"]);
    fireEvent.click(screen.getByRole("button", { name: "UsageSpendDescending" }));
    expect(titles()).toEqual(["Newest work", "Middle work", "Older work"]);
    expect(screen.getByRole("button", { name: "UsageSpendAscending" })).toHaveAttribute("aria-pressed", "true");
  });

  it("loads the selected session's turns against its summary checkpoint", async () => {
    tauriMocks.getCodexSessionDetails.mockImplementation(async (id: string) => details(id));
    render(<SessionInspector conversations={SESSIONS} historyDays={30} t={t} />);
    fireEvent.click(screen.getByRole("button", { name: /Older work/ }));

    expect(tauriMocks.getCodexSessionDetails).toHaveBeenCalledWith(
      "s-1",
      { totals: SESSIONS[0].totals, costEstimate: SESSIONS[0].costEstimate },
      30,
    );
    const turns = await screen.findByTestId("session-turns");
    expect(within(turns).getByText("Turn 1")).toBeInTheDocument();
    expect(within(turns).getByText("UsageSpendUnattributedUsage")).toBeInTheDocument();
    expect(within(turns).getAllByText("Request 1")).toHaveLength(2);
  });

  it("shows the limit message and the generic message for other failures", async () => {
    tauriMocks.getCodexSessionDetails.mockImplementation(async (id: string) => {
      throw id === "s-1" ? "limitExceeded" : "sourceChanged";
    });
    render(<SessionInspector conversations={SESSIONS} historyDays={30} t={t} />);
    fireEvent.click(screen.getByRole("button", { name: /Older work/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("UsageSpendRequestDetailsLimit");

    fireEvent.click(screen.getByRole("button", { name: /Newest work/ }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("UsageSpendRequestDetailsUnavailable"),
    );
  });

  it("ignores a stale reply after the selection changed", async () => {
    let resolveFirst: (value: CodexSessionDetails) => void = () => {};
    tauriMocks.getCodexSessionDetails
      .mockImplementationOnce(() => new Promise<CodexSessionDetails>((resolve) => (resolveFirst = resolve)))
      .mockImplementationOnce(async () => {
        throw "unavailable";
      });
    render(<SessionInspector conversations={SESSIONS} historyDays={30} t={t} />);
    fireEvent.click(screen.getByRole("button", { name: /Older work/ }));
    fireEvent.click(screen.getByRole("button", { name: /Newest work/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("UsageSpendRequestDetailsUnavailable");
    resolveFirst(details("s-1"));
    await Promise.resolve();
    expect(screen.queryByTestId("session-turns")).toBeNull();
  });
});
