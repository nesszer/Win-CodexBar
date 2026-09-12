import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ClaudeSwapAccount, ClaudeSwapAccountsState } from "../../../../../types/bridge";

const mocks = vi.hoisted(() => ({
  claudeSwapAccountsList: vi.fn(),
  claudeSwapAccountSwitch: vi.fn(),
  getSettingsSnapshot: vi.fn(),
  updateSettings: vi.fn(),
}));
const events = vi.hoisted(() => ({
  listen: vi.fn<(event: string, listener: () => void) => Promise<() => void>>(),
}));
vi.mock("../../../../../lib/tauri", () => mocks);
vi.mock("@tauri-apps/api/event", () => events);
import { ClaudeSwapAccountsSection } from "./ClaudeSwapAccountsSection";

const t = (key: string) => key;

const active: ClaudeSwapAccount = {
  id: "claude-swap:1",
  slot: 1,
  label: "work@example.com",
  email: "work@example.com",
  organization: null,
  alias: null,
  isActive: true,
  canActivate: false,
  status: "ok",
  error: null,
  fiveHour: { usedPercent: 12, resetsAt: null },
  sevenDay: null,
  scoped: [],
};

const switchable: ClaudeSwapAccount = {
  ...active,
  id: "claude-swap:2",
  slot: 2,
  label: "personal@example.com",
  email: "personal@example.com",
  isActive: false,
  canActivate: true,
};

const blocked: ClaudeSwapAccount = {
  ...switchable,
  id: "claude-swap:3",
  slot: 3,
  label: "Backup",
  canActivate: false,
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
    mocks.getSettingsSnapshot.mockResolvedValue({
      claudeSwapEnabled: false,
      claudeSwapExecutablePath: "",
    });
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
    mocks.claudeSwapAccountSwitch.mockResolvedValue(undefined);
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
});
