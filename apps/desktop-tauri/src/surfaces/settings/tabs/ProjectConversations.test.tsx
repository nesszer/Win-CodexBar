import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import ProjectConversations, {
  CONVERSATION_INITIAL_ROWS,
  CONVERSATION_MAX_ROWS,
} from "./ProjectConversations";
import type {
  CodexWorkspacesSessionUsage,
  CodexWorkspacesTurnPerformance,
} from "../../../types/bridge";
import type { LocaleKey } from "../../../i18n/keys";

const t = (key: LocaleKey) => key;

function session(
  index: number,
  unknownTokens = 0,
  turnPerformance: CodexWorkspacesTurnPerformance | null = null,
): CodexWorkspacesSessionUsage {
  return {
    id: `session-${index}`,
    projectId: "project-a",
    displayTitle: `Thread ${index}`,
    cwd: null,
    startedAt: null,
    latestActivity: null,
    totals: { inputTokens: 10, cachedInputTokens: 0, outputTokens: 0, totalTokens: 10 },
    costEstimate: { knownUsd: 100 - index, unknownTokens },
    topModel: null,
    turnPerformance,
  };
}

function performance(overrides: Partial<CodexWorkspacesTurnPerformance> = {}): CodexWorkspacesTurnPerformance {
  return {
    sampleCount: 9,
    outputTokensPerSecond: 45.4,
    medianDurationMilliseconds: 8100,
    medianFirstTokenMilliseconds: 1200,
    firstTokenSampleCount: 9,
    details: {
      p95DurationMilliseconds: null,
      p95FirstTokenMilliseconds: null,
      outputRateLowerQuartile: null,
      outputRateUpperQuartile: null,
      cachedInputFraction: null,
      cacheSampleCount: 0,
      groups: [],
    },
    ...overrides,
  };
}

describe("ProjectConversations", () => {
  it("shows ranked rows in the given order with their rank", () => {
    render(<ProjectConversations conversations={[session(1), session(2, 5)]} t={t} />);

    expect(screen.getByText("#1")).toBeTruthy();
    expect(screen.getByText("#2")).toBeTruthy();
    expect(screen.getByText("Thread 1").getAttribute("title")).toBe("Thread 1");
    expect(screen.getByText(/\$99\.00/)).toBeTruthy();
    expect(screen.getByText(/~\$98\.00/)).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("shows the top rows first and expands up to the display limit", () => {
    const conversations = Array.from({ length: CONVERSATION_MAX_ROWS + 5 }, (_, i) => session(i + 1));
    render(<ProjectConversations conversations={conversations} t={t} />);

    expect(screen.getAllByText(/^#\d+$/)).toHaveLength(CONVERSATION_INITIAL_ROWS);
    fireEvent.click(screen.getByRole("button", { name: `UsageSpendShowAll (${CONVERSATION_MAX_ROWS})` }));
    expect(screen.getAllByText(/^#\d+$/)).toHaveLength(CONVERSATION_MAX_ROWS);
    expect(screen.queryByText(`Thread ${CONVERSATION_MAX_ROWS + 1}`)).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "UsageSpendShowLess" }));
    expect(screen.getAllByText(/^#\d+$/)).toHaveLength(CONVERSATION_INITIAL_ROWS);
  });

  it("shows no performance line for a conversation without timed turns", () => {
    render(<ProjectConversations conversations={[session(1)]} t={t} />);

    expect(screen.queryByText(/UsageSpendTurnOutput/)).toBeNull();
    expect(screen.queryByText("UsageSpendPerformanceDetails")).toBeNull();
  });

  it("shows turn output, first token and duration with their upstream labels", () => {
    render(<ProjectConversations conversations={[session(1, 0, performance())]} t={t} />);

    expect(screen.getByText(/UsageSpendTurnOutput: 45\.4 UsageSpendTokensPerSecond/)).toBeTruthy();
    expect(screen.getByText(/UsageSpendFirstToken: 1\.2 UsageSpendSeconds/)).toBeTruthy();
    expect(screen.getByText(/UsageSpendTurnDuration: 8\.1 UsageSpendSeconds/)).toBeTruthy();
    expect(screen.getByTitle("UsageSpendTurnOutputHelp")).toBeTruthy();
    expect(screen.getByText("UsageSpendPerformanceDetails")).toBeTruthy();
  });

  it("shows spread, cache and per-model details when the sample minimums are met", () => {
    const rich = performance({
      details: {
        p95DurationMilliseconds: 19000,
        p95FirstTokenMilliseconds: 2500,
        outputRateLowerQuartile: 250,
        outputRateUpperQuartile: 500,
        cachedInputFraction: 0.5,
        cacheSampleCount: 4,
        groups: [
          {
            model: "gpt-5.5",
            reasoningEffort: "high",
            sampleCount: 5,
            firstTokenSampleCount: 5,
            medianFirstTokenMilliseconds: 1200,
            medianDurationMilliseconds: 8100,
            outputTokensPerSecond: 40,
          },
          {
            model: null,
            reasoningEffort: null,
            sampleCount: 4,
            firstTokenSampleCount: 0,
            medianFirstTokenMilliseconds: null,
            medianDurationMilliseconds: 9000,
            outputTokensPerSecond: 30,
          },
        ],
      },
    });
    render(<ProjectConversations conversations={[session(1, 0, rich)]} t={t} />);

    expect(screen.getByText(/UsageSpendP95Duration: 19\.0 UsageSpendSeconds/)).toBeTruthy();
    expect(screen.getByText(/UsageSpendP95FirstToken: 2\.5 UsageSpendSeconds/)).toBeTruthy();
    expect(screen.getByText(/UsageSpendMiddleHalfRate: 250\.0–500\.0 UsageSpendTokensPerSecond/)).toBeTruthy();
    expect(screen.getByText("UsageSpendCachedInput: 50%")).toBeTruthy();
    expect(screen.getByText(/gpt-5\.5 \/ high: 40\.0 UsageSpendTokensPerSecond/)).toBeTruthy();
    expect(screen.getByText(/UsageSpendUnknownModel \/ UsageSpendUnknownEffort: 30\.0/)).toBeTruthy();
  });

  it("explains missing spread and cache evidence instead of showing a number", () => {
    render(<ProjectConversations conversations={[session(1, 0, performance())]} t={t} />);

    expect(screen.getAllByText(/UsageSpendNeedsTwentyTurns/)).toHaveLength(2);
    expect(screen.getByText(/UsageSpendNeedsFourTurns/)).toBeTruthy();
    expect(screen.getByText("UsageSpendCachedInput: UsageSpendCacheUnavailable")).toBeTruthy();
  });
});
