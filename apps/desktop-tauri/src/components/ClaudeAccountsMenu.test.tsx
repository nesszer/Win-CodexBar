import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ClaudeAccount } from "../types/bridge";

const mocks = vi.hoisted(() => {
  const listeners = new Map<string, () => void>();
  return {
    claudeAccountsList: vi.fn(),
    claudeAccountSwitch: vi.fn(),
    refreshProviders: vi.fn(),
    listeners,
    listen: vi.fn((event: string, callback: () => void) => {
      listeners.set(event, callback);
      return Promise.resolve(() => listeners.delete(event));
    }),
  };
});
vi.mock("../lib/tauri", () => mocks);
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
vi.mock("../hooks/useLocale", () => ({ useLocale: () => ({ t: (key: string) => key }) }));
import ClaudeAccountsMenu from "./ClaudeAccountsMenu";

const first: ClaudeAccount = { id: "first:org", email: "first@example.com", organization: "Personal", plan: "max", isActive: true, isSaved: true };
const second: ClaudeAccount = { ...first, id: "second:org", email: "second@example.com", organization: "Work", isActive: false };

describe("ClaudeAccountsMenu", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.listeners.clear();
    mocks.claudeAccountsList.mockResolvedValue([first, second]);
    mocks.claudeAccountSwitch.mockResolvedValue(undefined);
    mocks.refreshProviders.mockResolvedValue(undefined);
  });

  it("marks the current account and switches the selected saved account", async () => {
    render(<ClaudeAccountsMenu hideEmail={false} />);
    await screen.findByText(first.email);
    const buttons = screen.getAllByText("CodexAccountsSwitchButton") as HTMLButtonElement[];
    expect(buttons[0].disabled).toBe(true);
    expect(buttons[1].disabled).toBe(false);
    await act(async () => fireEvent.click(buttons[1]));
    expect(mocks.claudeAccountSwitch).toHaveBeenCalledWith(second.id);
    expect(screen.getByRole("status").textContent).toBe("ClaudeAccountsSwitched");
  });

  it("shows a single inactive saved account so the first login can be activated", async () => {
    mocks.claudeAccountsList.mockResolvedValue([second]);
    render(<ClaudeAccountsMenu hideEmail={false} />);
    await screen.findByText(second.email);
    const button = screen.getByText("CodexAccountsSwitchButton");
    expect(button).not.toBeDisabled();
    await act(async () => fireEvent.click(button));
    expect(mocks.claudeAccountSwitch).toHaveBeenCalledWith(second.id);
  });

  it("keeps the menu in activating and reconciling phases until the switch settles", async () => {
    let resolveSwitch: (() => void) | undefined;
    mocks.claudeAccountSwitch.mockImplementation(() => new Promise<void>(resolve => {
      resolveSwitch = resolve;
    }));
    render(<ClaudeAccountsMenu hideEmail={false} />);
    await screen.findByText(first.email);
    const details = () => document.querySelector("details[data-claude-account-phase]") as HTMLDetailsElement;
    const button = screen.getAllByText("CodexAccountsSwitchButton")[1];

    await act(async () => fireEvent.click(button));
    expect(details().dataset.claudeAccountPhase).toBe("activating");
    expect(details()).toHaveAttribute("aria-busy", "true");

    await act(async () => {
      mocks.listeners.get("claude-accounts-reconciling")?.();
    });
    expect(details().dataset.claudeAccountPhase).toBe("reconciling");

    await act(async () => {
      resolveSwitch?.();
    });
    await waitFor(() => expect(details().dataset.claudeAccountPhase).toBe("settled"));
    expect(details()).toHaveAttribute("aria-busy", "false");
  });

  it("masks emails, including tooltips, when hideEmail is enabled", async () => {
    mocks.claudeAccountsList.mockResolvedValue([first, { ...second, organization: `${second.email}'s Organization` }]);
    const { container } = render(<ClaudeAccountsMenu hideEmail />);
    await screen.findByText("ClaudeAccountsTitle");
    expect(container.textContent).not.toContain(first.email);
    expect(container.innerHTML).not.toContain(second.email);
  });

  it("shows switch failures and leaves the current account marked active", async () => {
    mocks.claudeAccountSwitch.mockRejectedValue("Close Claude Code first.");
    render(<ClaudeAccountsMenu hideEmail={false} />);
    await screen.findByText(first.email);
    await act(async () => fireEvent.click(screen.getAllByText("CodexAccountsSwitchButton")[1]));
    expect(screen.getByRole("alert").textContent).toContain("Close Claude Code first.");
    expect(screen.queryByRole("status")).toBeNull();
    expect(mocks.refreshProviders).not.toHaveBeenCalled();
  });

  it("keeps account loading failures discoverable and retries when the window gains focus", async () => {
    mocks.claudeAccountsList.mockRejectedValueOnce("Account storage unavailable.");
    const onLayoutChange = vi.fn();
    render(<ClaudeAccountsMenu hideEmail={false} onLayoutChange={onLayoutChange} />);
    await screen.findByText("Account storage unavailable.");
    expect(screen.getByText("ClaudeAccountsTitle")).toBeInTheDocument();
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(await screen.findByText(second.email)).toBeInTheDocument();
    expect(screen.queryByText("Account storage unavailable.")).toBeNull();
    expect(onLayoutChange).toHaveBeenCalled();
  });
});
