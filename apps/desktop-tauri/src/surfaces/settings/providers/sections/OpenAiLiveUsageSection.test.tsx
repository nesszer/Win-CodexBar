import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LocaleKey } from "../../../../i18n/keys";
import type {
  OrgRateLimitsSnapshot,
  ProviderThroughputSnapshot,
} from "../../../../types/bridge";

const tauriMocks = vi.hoisted(() => ({
  getOpenAiLiveUsage: vi.fn(),
  getOpenAiOrgRateLimits: vi.fn(),
  getProviderWorkspaceId: vi.fn(),
}));

vi.mock("../../../../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../lib/tauri")>()),
  ...tauriMocks,
}));

import { PROVIDER_WORKSPACE_ID_SAVED_EVENT } from "./credentials/OpenAiExtras";
import { OpenAiLiveUsageSection } from "./OpenAiLiveUsageSection";

const t = (key: LocaleKey) => key as string;

const START = 1_788_220_800;

function liveUsage(): ProviderThroughputSnapshot {
  return {
    minutes: [
      { start: START, totalTokens: 1_500, requests: 1, isPending: false },
      { start: START + 60, totalTokens: 0, requests: 0, isPending: true },
    ],
    frontier: START,
    latestTokensPerMinute: 1_500,
    peakTokensPerMinute: 1_500,
    peakAt: START,
    fiveMinuteAverageTokensPerMinute: 1_500,
    models: [],
    windowMinutes: 60,
    fetchedAt: START + 60,
    projectId: null,
  };
}

function orgCaps(): OrgRateLimitsSnapshot {
  return {
    byModel: [{ model: "gpt-5", tokensPerMinute: 90_000, requestsPerMinute: 500 }],
    projectsSampled: 2,
    truncated: false,
    fetchedAt: START,
  };
}

function renderSection(onSettingsChange = vi.fn()) {
  const view = render(
    <OpenAiLiveUsageSection
      settings={{ openAiLiveUsageEnabled: true, openAiModelRateLimits: [] }}
      disabled={false}
      t={t}
      onSettingsChange={onSettingsChange}
    />,
  );
  return { ...view, onSettingsChange };
}

describe("OpenAiLiveUsageSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getProviderWorkspaceId.mockResolvedValue(null);
    tauriMocks.getOpenAiLiveUsage.mockResolvedValue(liveUsage());
    tauriMocks.getOpenAiOrgRateLimits.mockResolvedValue(orgCaps());
  });

  it("writes the live-usage switch through the settings patch", () => {
    const { container, onSettingsChange } = renderSection();
    const toggle = container.querySelector("input.toggle") as HTMLInputElement;
    expect(toggle.checked).toBe(true);
    fireEvent.click(toggle);
    expect(onSettingsChange).toHaveBeenCalledWith({ openAiLiveUsageEnabled: false });
  });

  it("loads the selected window and renders the Tokens/min chart", async () => {
    const { container } = renderSection();
    fireEvent.click(screen.getByText("OpenAiLiveUsageLoad"));
    await waitFor(() => expect(container.querySelector(".openai-tpm")).not.toBeNull());
    expect(tauriMocks.getOpenAiLiveUsage).toHaveBeenCalledWith(60);
  });

  it("hides organization caps and shows the scoped note when a project id is saved", async () => {
    tauriMocks.getProviderWorkspaceId.mockResolvedValue("proj_abc");
    renderSection();
    fireEvent.click(screen.getByText("OpenAiOrgLimitsLoad"));
    await waitFor(() => expect(screen.getByText("OpenAiOrgLimitsScoped")).toBeTruthy());
    expect(tauriMocks.getOpenAiOrgRateLimits).not.toHaveBeenCalled();
    expect(screen.queryByText("gpt-5")).toBeNull();
  });

  it("lists organization caps when no project id is saved", async () => {
    renderSection();
    fireEvent.click(screen.getByText("OpenAiOrgLimitsLoad"));
    await waitFor(() => expect(screen.getByText("gpt-5")).toBeTruthy());
    expect(tauriMocks.getOpenAiOrgRateLimits).toHaveBeenCalledTimes(1);
  });

  it("clears loaded caps when an OpenAI project id is saved", async () => {
    renderSection();
    fireEvent.click(screen.getByText("OpenAiOrgLimitsLoad"));
    await waitFor(() => expect(screen.getByText("gpt-5")).toBeTruthy());

    act(() => {
      window.dispatchEvent(
        new CustomEvent(PROVIDER_WORKSPACE_ID_SAVED_EVENT, { detail: "litellm" }),
      );
    });
    expect(screen.getByText("gpt-5")).toBeTruthy();

    act(() => {
      window.dispatchEvent(
        new CustomEvent(PROVIDER_WORKSPACE_ID_SAVED_EVENT, { detail: "openaiapi" }),
      );
    });
    expect(screen.queryByText("gpt-5")).toBeNull();
  });
});
