import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ClaudeSwapAccount, ClaudeSwapAccountsState } from "../../../../../types/bridge";

const mocks = vi.hoisted(() => ({
  claudeSwapAccountsList: vi.fn(),
  claudeSwapAccountSwitch: vi.fn(),
  claudeSwapAccountReauthenticate: vi.fn(),
  claudeReconciliationState: vi.fn(),
  getSettingsSnapshot: vi.fn(),
  getCurrencyRates: vi.fn(),
  updateSettings: vi.fn(),
}));
const events = vi.hoisted(() => ({
  listen: vi.fn<(event: string, listener: () => void) => Promise<() => void>>(),
}));
vi.mock("../../../../../lib/tauri", () => mocks);
vi.mock("@tauri-apps/api/event", () => events);
import { ClaudeSwapAccountsSection } from "./ClaudeSwapAccountsSection";
import { CurrencyProvider } from "../../../../../hooks/CurrencyProvider";

const t = (key: string) => key;

const active: ClaudeSwapAccount = {
  id: "claude-swap:1",
  slot: 1,
  label: "work@example.com",
  email: "work@example.com",
  organization: null,
  alias: null,
  isActive: true,
  action: null,
  isDisabled: false,
  status: "ok",
  error: null,
  fiveHour: { usedPercent: 12, resetsAt: null },
  sevenDay: null,
  scoped: [],
  spend: null,
  historicalUsage: null,
};

const switchable: ClaudeSwapAccount = {
  ...active,
  id: "claude-swap:2",
  slot: 2,
  label: "personal@example.com",
  email: "personal@example.com",
  isActive: false,
  action: "switch",
};

const blocked: ClaudeSwapAccount = {
  ...switchable,
  id: "claude-swap:3",
  slot: 3,
  label: "Backup",
  action: null,
  status: "token_expired",
  error: "Token expired. Switch to this account in claude-swap to refresh it.",
};

function enabledState(accounts: ClaudeSwapAccount[]): ClaudeSwapAccountsState {
  return { enabled: true, executableConfigured: true, accounts, error: null };
}

describe("ClaudeSwapAccountsSection", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    events.listen.mockResolvedValue(() => {});
    mocks.claudeReconciliationState.mockResolvedValue(null);
    mocks.getSettingsSnapshot.mockResolvedValue({
      claudeSwapEnabled: false,
      claudeSwapExecutablePath: "",
    });
    mocks.getCurrencyRates.mockResolvedValue({ rates: { USD: 1, TRY: 48.5 } });
    mocks.claudeSwapAccountsList.mockResolvedValue({
      enabled: false,
      executableConfigured: false,
      accounts: [],
      error: null,
    });
  });

  it("shows the disabled status and persists an explicit opt-in", async () => {
    render(<ClaudeSwapAccountsSection t={t} />);
    await screen.findByText("ClaudeSwapTitle");
    expect((screen.getByRole("checkbox") as HTMLInputElement).checked).toBe(false);
    mocks.updateSettings.mockResolvedValue(undefined);
    mocks.claudeSwapAccountsList.mockResolvedValue(
      enabledState([active, switchable]),
    );
    await act(async () => fireEvent.click(screen.getByRole("checkbox")));
    expect(mocks.updateSettings).toHaveBeenCalledWith({ claudeSwapEnabled: true });
  });

  it("switches only actionable inactive accounts and reports success", async () => {
    mocks.getSettingsSnapshot.mockResolvedValue({
      claudeSwapEnabled: true,
      claudeSwapExecutablePath: "~/bin/cswap",
    });
    mocks.claudeSwapAccountsList.mockResolvedValue(
      enabledState([active, switchable, blocked]),
    );
    mocks.claudeSwapAccountSwitch.mockResolvedValue({
      generation: 1, status: "succeeded", providerRefreshGeneration: 1, detail: "published",
    });
    render(<ClaudeSwapAccountsSection t={t} />);
    await screen.findByText("work@example.com");

    expect(screen.getByText("TokenAccountActive")).toBeTruthy();
    expect(screen.queryByText("ClaudeSwapSwitchButton")).toBeTruthy();
    // Exactly one actionable card (the other inactive card is not actionable).
    expect(screen.getAllByText("ClaudeSwapSwitchButton")).toHaveLength(1);

    await act(async () =>
      fireEvent.click(screen.getByText("ClaudeSwapSwitchButton")),
    );
    expect(mocks.claudeSwapAccountSwitch).toHaveBeenCalledWith(2);
    expect(screen.getByRole("status").textContent).toBe("ClaudeSwapSwitched");
  });

  it("surfaces adapter errors without offering a switch", async () => {
    mocks.getSettingsSnapshot.mockResolvedValue({
      claudeSwapEnabled: true,
      claudeSwapExecutablePath: "~/bin/cswap",
    });
    mocks.claudeSwapAccountsList.mockResolvedValue({
      enabled: true,
      executableConfigured: true,
      accounts: [],
      error: "claude-swap did not respond within 30 seconds.",
    });
    render(<ClaudeSwapAccountsSection t={t} />);
    await waitFor(() =>
      expect(screen.getByRole("alert").textContent).toContain("did not respond"),
    );
    expect(screen.queryByText("ClaudeSwapSwitchButton")).toBeNull();
  });

  it("allows retrying the same executable path after a failed save", async () => {
    render(<ClaudeSwapAccountsSection t={t} />);
    await act(async () => {});
    const input = screen.getByRole("textbox");
    mocks.updateSettings.mockRejectedValueOnce("Save failed.");
    await act(async () => {
      fireEvent.change(input, { target: { value: "C:/tools/cswap.exe" } });
      fireEvent.blur(input);
    });
    expect(screen.getByRole("alert").textContent).toContain("Save failed.");
    mocks.updateSettings.mockResolvedValueOnce(undefined);
    await act(async () => fireEvent.blur(input));
    expect(mocks.updateSettings).toHaveBeenCalledTimes(2);
    expect(mocks.updateSettings).toHaveBeenLastCalledWith({
      claudeSwapExecutablePath: "C:/tools/cswap.exe",
    });
  });

  it("shows a switch failure without claiming success", async () => {
    mocks.getSettingsSnapshot.mockResolvedValue({
      claudeSwapEnabled: true,
      claudeSwapExecutablePath: "~/bin/cswap",
    });
    mocks.claudeSwapAccountsList.mockResolvedValue(
      enabledState([active, switchable]),
    );
    mocks.claudeSwapAccountSwitch.mockRejectedValue("cswap failed.");
    render(<ClaudeSwapAccountsSection t={t} />);
    await screen.findByText("personal@example.com");
    await act(async () =>
      fireEvent.click(screen.getByText("ClaudeSwapSwitchButton")),
    );
    expect(screen.getByRole("alert").textContent).toContain("cswap failed.");
    expect(screen.queryByText("ClaudeSwapSwitched")).toBeNull();
  });

  it("re-authenticates an active foreign credential and reports the distinct result", async () => {
    const foreign: ClaudeSwapAccount = {
      ...active,
      status: "foreign_credential",
      action: "reauthenticate",
      error: "claude-swap reports the live credential belongs to a different account.",
    };
    mocks.getSettingsSnapshot.mockResolvedValue({
      claudeSwapEnabled: true,
      claudeSwapExecutablePath: "~/bin/cswap",
    });
    mocks.claudeSwapAccountsList.mockResolvedValue(enabledState([foreign]));
    mocks.claudeSwapAccountReauthenticate.mockResolvedValue({
      generation: 2, status: "succeeded", providerRefreshGeneration: 2, detail: "published",
    });
    render(<ClaudeSwapAccountsSection t={t} />);
    await screen.findByText("work@example.com");
    await act(async () =>
      fireEvent.click(screen.getByText("ClaudeSwapReauthenticateButton")),
    );
    expect(mocks.claudeSwapAccountReauthenticate).toHaveBeenCalledWith(1);
    expect(screen.getByRole("status").textContent).toBe("ClaudeSwapReauthenticated");
    expect(mocks.claudeSwapAccountSwitch).not.toHaveBeenCalled();
  });

  it("renders disabled, spend, and display-only historical usage details", async () => {
    const account: ClaudeSwapAccount = {
      ...switchable,
      isDisabled: true,
      spend: {
        used: 2,
        limit: 20,
        usedPercent: 10,
        currencyCode: "USD",
        resetsAt: null,
      },
      historicalUsage: {
        fiveHour: { usedPercent: 44, resetsAt: null },
        sevenDay: null,
        scoped: [],
        spend: {
          used: 1,
          limit: 10,
          usedPercent: 10,
          currencyCode: "USD",
          resetsAt: null,
        },
        fetchedAt: "2026-09-12T00:45:00Z",
        provenance: "source_reported_last_good",
      },
    };
    mocks.getSettingsSnapshot.mockResolvedValue({
      claudeSwapEnabled: true,
      claudeSwapExecutablePath: "~/bin/cswap",
      preferredCurrencyCode: "TRY",
    });
    mocks.claudeSwapAccountsList.mockResolvedValue(enabledState([account]));
    render(
      <CurrencyProvider>
        <ClaudeSwapAccountsSection t={t} />
      </CurrencyProvider>,
    );
    await screen.findByText("personal@example.com");
    await screen.findByText(/ClaudeSwapSpend .*97[.,]00.*970[.,]00.*10%/);
    expect(screen.getByText(/ClaudeSwapHistoricalUsage.*48[.,]50.*485[.,]00/)).toBeTruthy();
    expect(screen.queryByText(/USD/)).toBeNull();
    expect(screen.getByText("ClaudeSwapDisabled")).toBeTruthy();
  });
});
