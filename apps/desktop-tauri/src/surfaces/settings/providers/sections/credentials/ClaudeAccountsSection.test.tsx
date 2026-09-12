import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ClaudeAccount } from "../../../../../types/bridge";

const mocks = vi.hoisted(() => ({
  claudeAccountsList: vi.fn(), claudeAccountAdd: vi.fn(), claudeAccountCancelLogin: vi.fn(),
  claudeAccountSaveCurrent: vi.fn(), claudeAccountRemove: vi.fn(), claudeAccountSwitch: vi.fn(),
  claudeSwapAccountsList: vi.fn(), claudeSwapAccountSwitch: vi.fn(),
  getSettingsSnapshot: vi.fn(), updateSettings: vi.fn(),
}));
const events = vi.hoisted(() => ({ listen: vi.fn<(event: string, listener: () => void) => Promise<() => void>>() }));
vi.mock("../../../../../lib/tauri", () => mocks);
vi.mock("@tauri-apps/api/event", () => events);
import { ClaudeAccountsSection } from "./ClaudeAccountsSection";

const t = (key: string) => key;
const current: ClaudeAccount = { id: "one:org", email: "one@example.com", organization: "Work", plan: "max", isActive: true, isSaved: false };
const other: ClaudeAccount = { ...current, id: "two:org", email: "two@example.com", isActive: false, isSaved: true };

describe("ClaudeAccountsSection", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    events.listen.mockResolvedValue(() => {});
    mocks.claudeAccountsList.mockResolvedValue([current, other]);
    mocks.claudeSwapAccountsList.mockResolvedValue({
      enabled: false,
      executableConfigured: false,
      accounts: [],
      error: null,
    });
    mocks.getSettingsSnapshot.mockResolvedValue({
      claudeSwapEnabled: false,
      claudeSwapExecutablePath: "",
    });
  });

  it("offers to save the discovered account and switches only saved inactive accounts", async () => {
    render(<ClaudeAccountsSection t={t} />);
    await screen.findByText(current.email);
    expect(screen.getAllByText("CodexAccountsSwitchButton")).toHaveLength(1);
    await act(async () => fireEvent.click(screen.getByText("ClaudeAccountsSaveCurrent")));
    expect(mocks.claudeAccountSaveCurrent).toHaveBeenCalledOnce();
    await act(async () => fireEvent.click(screen.getByText("CodexAccountsSwitchButton")));
    expect(mocks.claudeAccountSwitch).toHaveBeenCalledWith(other.id);
    expect(screen.getByRole("status").textContent).toBe("ClaudeAccountsSwitched");
  });

  it("keeps mutations disabled across an account update during browser login and supports cancel", async () => {
    let finish!: () => void;
    mocks.claudeAccountAdd.mockImplementation(() => new Promise<void>(resolve => { finish = resolve; }));
    mocks.claudeAccountCancelLogin.mockResolvedValue(undefined);
    render(<ClaudeAccountsSection t={t} />);
    await screen.findByText(current.email);
    fireEvent.click(screen.getByText("CodexAccountsAddButton"));
    const eventCallback = events.listen.mock.calls[0][1] as unknown as () => void;
    await act(async () => eventCallback());
    expect((screen.getByText("CodexAccountsSwitchButton") as HTMLButtonElement).disabled).toBe(true);
    await act(async () => fireEvent.click(screen.getByText("ClaudeAccountsCancelLogin")));
    expect(mocks.claudeAccountCancelLogin).toHaveBeenCalledOnce();
    await act(async () => finish());
    expect(screen.queryByText("ClaudeAccountsCancelLogin")).toBeNull();
    expect((screen.getByText("CodexAccountsAddButton") as HTMLButtonElement).disabled).toBe(false);
  });

  it("shows a switch error without claiming success and allows retry", async () => {
    mocks.claudeAccountSwitch.mockRejectedValue("Close Claude Code first.");
    render(<ClaudeAccountsSection t={t} />);
    await screen.findByText(other.email);
    await act(async () => fireEvent.click(screen.getByText("CodexAccountsSwitchButton")));
    expect(screen.getByRole("alert").textContent).toContain("Close Claude Code first.");
    expect(screen.queryByText("ClaudeAccountsSwitched")).toBeNull();
    expect((screen.getByText("CodexAccountsSwitchButton") as HTMLButtonElement).disabled).toBe(false);
  });

  it("shows initial loading errors while keeping sign-in accessible", async () => {
    mocks.claudeAccountsList.mockRejectedValue("Could not read saved accounts.");
    render(<ClaudeAccountsSection t={t} />);
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("Could not read"));
    expect((screen.getByText("CodexAccountsAddButton") as HTMLButtonElement).disabled).toBe(false);
  });
});
