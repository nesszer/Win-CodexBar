import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ClaudeAccount } from "../types/bridge";

const mocks = vi.hoisted(() => {
  const listeners = new Map<string, (event: { payload: unknown }) => void>();
  return {
    claudeAccountsList: vi.fn(),
    claudeAccountAdd: vi.fn(),
    claudeAccountReauthenticate: vi.fn(),
    claudeAccountCancelLogin: vi.fn(),
    claudeAccountSwitch: vi.fn(),
    claudeReconciliationState: vi.fn(),
    refreshProviders: vi.fn(),
    listeners,
    listen: vi.fn((event: string, callback: (event: { payload: unknown }) => void) => {
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
const reconciliation = (generation: number, status: "pending" | "succeeded" | "failed", detail: string = status) => ({
  generation, status, detail, providerRefreshGeneration: null,
});

describe("ClaudeAccountsMenu", () => {
  it("shows Refresh login only for the expired account and removes it after recovery", async () => {
    mocks.claudeAccountsList.mockResolvedValue([first, { ...second, needsAuthentication: true, usageError: "Sign in again." }]);
    mocks.claudeAccountReauthenticate.mockResolvedValue(undefined);
    render(<ClaudeAccountsMenu hideEmail={false} />);
    const refresh = await screen.findByRole("button", { name: `CodexAccountsReauthenticateButton: ${second.email}` });
    expect(screen.getAllByRole("button", { name: /CodexAccountsReauthenticateButton/ })).toHaveLength(1);
    mocks.claudeAccountsList.mockResolvedValue([first, second]);
    await act(async () => fireEvent.click(refresh));
    expect(mocks.claudeAccountReauthenticate).toHaveBeenCalledWith(second.id);
    expect(mocks.claudeAccountAdd).not.toHaveBeenCalled();
    expect(mocks.claudeAccountSwitch).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: /CodexAccountsReauthenticateButton/ })).toBeNull();
  });

  it("does not suggest login for a temporary usage error", async () => {
    mocks.claudeAccountsList.mockResolvedValue([{ ...first, usageError: "Will retry automatically.", needsAuthentication: false }]);
    render(<ClaudeAccountsMenu hideEmail={false} />);
    await screen.findByText(first.email);
    expect(screen.queryByRole("button", { name: /CodexAccountsReauthenticateButton/ })).toBeNull();
  });
  it("renders per-account usage and reloads it after background checks", async () => {
    const usage = (used: number) => ({ fiveHour: { usedPercent: used, resetsAt: null }, sevenDay: null, updatedAt: "2026-10-03T12:00:00Z" });
    mocks.claudeAccountsList.mockResolvedValue([{ ...first, usage: usage(11) }, { ...second, usage: usage(63) }]);
    render(<ClaudeAccountsMenu hideEmail={false} />);
    await screen.findByText("89% PanelLeftSuffix");
    expect(screen.getByText("37% PanelLeftSuffix")).toBeInTheDocument();
    mocks.claudeAccountsList.mockResolvedValue([{ ...first, usage: usage(20) }, { ...second, usage: usage(70) }]);
    await act(async () => mocks.listeners.get("claude-accounts-updated")?.({ payload: null }));
    await screen.findByText("80% PanelLeftSuffix");
    expect(screen.getByText("30% PanelLeftSuffix")).toBeInTheDocument();
    expect(mocks.claudeAccountSwitch).not.toHaveBeenCalled();
  });
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.listeners.clear();
    mocks.claudeAccountsList.mockResolvedValue([first, second]);
    mocks.claudeReconciliationState.mockResolvedValue(null);
    mocks.claudeAccountSwitch.mockResolvedValue(reconciliation(1, "succeeded"));
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

  it("shows one active account and adds from the footer with cancellable sign-in", async () => {
    mocks.claudeAccountsList.mockResolvedValue([first]);
    let finishLogin: (() => void) | undefined;
    mocks.claudeAccountAdd.mockImplementation(() => new Promise<void>(resolve => { finishLogin = resolve; }));
    mocks.claudeAccountCancelLogin.mockResolvedValue(undefined);
    const { container } = render(<ClaudeAccountsMenu hideEmail={false} />);
    const account = await screen.findByText(first.email);
    expect(container.querySelector("details")?.open).toBe(true);
    const add = screen.getByRole("button", { name: "CodexAccountsAddButton" });
    expect(account.compareDocumentPosition(add) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await act(async () => fireEvent.click(add));
    expect(mocks.claudeAccountAdd).toHaveBeenCalledOnce();
    expect(add).toBeDisabled();
    expect(screen.getByText("ClaudeAccountsSigningIn")).toBeInTheDocument();
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "ClaudeAccountsCancelLogin" })));
    expect(mocks.claudeAccountCancelLogin).toHaveBeenCalledOnce();
    mocks.claudeAccountsList.mockResolvedValue([first, second]);
    await act(async () => finishLogin?.());
    expect(await screen.findByText(second.email)).toBeInTheDocument();
    expect(add).toBeEnabled();
    expect(mocks.claudeAccountSwitch).not.toHaveBeenCalled();
  });

  it("keeps the add action available when no accounts are saved and shows sign-in failures", async () => {
    mocks.claudeAccountsList.mockResolvedValue([]);
    mocks.claudeAccountAdd.mockRejectedValue("Login cancelled.");
    render(<ClaudeAccountsMenu hideEmail={false} />);
    const add = screen.getByRole("button", { name: "CodexAccountsAddButton" });
    await waitFor(() => expect(add).toBeEnabled());
    await act(async () => fireEvent.click(add));
    expect(screen.getByRole("alert")).toHaveTextContent("Login cancelled.");
    expect(add).toBeEnabled();
  });

  it("keeps the menu in activating and reconciling phases until the switch settles", async () => {
    let resolveSwitch: ((value: ReturnType<typeof reconciliation>) => void) | undefined;
    mocks.claudeAccountSwitch.mockImplementation(() => new Promise(resolve => {
      resolveSwitch = resolve;
    }));
    render(<ClaudeAccountsMenu hideEmail={false} />);
    await screen.findByText(first.email);
    const details = () => document.querySelector("details[data-claude-account-phase]") as HTMLDetailsElement;
    const row = screen.getByText(second.email).closest("li") as HTMLElement;
    const button = within(row).getByRole("button");

    await act(async () => fireEvent.click(button));
    expect(details().dataset.claudeAccountPhase).toBe("activating");
    expect(details()).toHaveAttribute("aria-busy", "true");

    await act(async () => {
      mocks.listeners.get("claude-reconciliation-changed")?.({ payload: reconciliation(1, "pending") });
    });
    expect(details().dataset.claudeAccountPhase).toBe("reconciling");

    await act(async () => {
      resolveSwitch?.(reconciliation(1, "pending"));
    });
    // Settling is event-driven; the resolving switch promise alone stays in
    // the reconciling phase until the backend emits the terminal event.
    await waitFor(() => expect(details().dataset.claudeAccountPhase).toBe("reconciling"));
    await act(async () => {
      mocks.listeners.get("claude-reconciliation-changed")?.({ payload: reconciliation(1, "succeeded") });
    });
    await waitFor(() => expect(details().dataset.claudeAccountPhase).toBe("settled"));
    expect(details()).toHaveAttribute("aria-busy", "false");
  });

  it("settles from a reconciled event fired with no local switch in flight", async () => {
    render(<ClaudeAccountsMenu hideEmail={false} />);
    await screen.findByText(first.email);
    const details = () => document.querySelector("details[data-claude-account-phase]") as HTMLDetailsElement;
    const row = screen.getByText(second.email).closest("li") as HTMLElement;
    const button = within(row).getByRole("button");

    await act(async () => {
      mocks.listeners.get("claude-reconciliation-changed")?.({ payload: reconciliation(2, "pending") });
    });
    expect(details().dataset.claudeAccountPhase).toBe("reconciling");
    expect(details()).toHaveAttribute("aria-busy", "true");
    expect(button).toBeDisabled();

    await act(async () => {
      mocks.listeners.get("claude-reconciliation-changed")?.({ payload: reconciliation(2, "succeeded") });
    });
    expect(details().dataset.claudeAccountPhase).toBe("settled");
    expect(details()).toHaveAttribute("aria-busy", "false");
    expect(button).not.toBeDisabled();
  });

  it("waits for the reconciled event before settling a local switch", async () => {
    let resolveSwitch: ((value: ReturnType<typeof reconciliation>) => void) | undefined;
    mocks.claudeAccountSwitch.mockImplementation(() => new Promise(resolve => {
      resolveSwitch = resolve;
    }));
    render(<ClaudeAccountsMenu hideEmail={false} />);
    await screen.findByText(first.email);
    const details = () => document.querySelector("details[data-claude-account-phase]") as HTMLDetailsElement;

    const row = screen.getByText(second.email).closest("li") as HTMLElement;
    const button = within(row).getByRole("button");
    await act(async () => fireEvent.click(button));
    await act(async () => {
      mocks.listeners.get("claude-reconciliation-changed")?.({ payload: reconciliation(3, "pending") });
    });
    await act(async () => {
      resolveSwitch?.(reconciliation(3, "pending"));
    });
    // The switch promise resolving is not enough: settling is event-driven.
    await waitFor(() => expect(details().dataset.claudeAccountPhase).toBe("reconciling"));
    await act(async () => {
      mocks.listeners.get("claude-reconciliation-changed")?.({ payload: reconciliation(3, "succeeded") });
    });
    await waitFor(() => expect(details().dataset.claudeAccountPhase).toBe("settled"));
  });

  it("surfaces a late failure for the matching pending generation", async () => {
    mocks.claudeAccountSwitch.mockResolvedValue(reconciliation(8, "pending"));
    render(<ClaudeAccountsMenu hideEmail={false} />);
    await screen.findByText(second.email);
    const row = screen.getByText(second.email).closest("li") as HTMLElement;
    await act(async () => fireEvent.click(within(row).getByRole("button")));
    await waitFor(() => {
      expect(document.querySelector("details")?.dataset.claudeAccountPhase).toBe("reconciling");
    });

    await act(async () => {
      mocks.listeners.get("claude-reconciliation-changed")?.({
        payload: reconciliation(8, "failed", "late refresh failed"),
      });
    });
    await waitFor(() => {
      expect(screen.getByRole("alert").textContent).toContain("late refresh failed");
    });
    expect(screen.queryByText("ClaudeAccountsSwitched")).toBeNull();
  });

  it("uses stable opaque account labels and redacts tooltips when hideEmail is enabled", async () => {
    mocks.claudeAccountsList.mockResolvedValue([first, { ...second, organization: `${second.email}'s Organization` }]);
    const { container } = render(<ClaudeAccountsMenu hideEmail />);
    await screen.findByText("Account 1");
    const labels = container.querySelectorAll(".codex-menu-accounts__email");
    expect(labels[0].firstChild?.textContent).toBe("Account 1");
    expect(labels[0].getAttribute("title")).toBe("Account 1");
    expect(labels[1].firstChild?.textContent).toBe("Account 2");
    expect(labels[1].getAttribute("title")).toBe("Account 2");
    expect(container.textContent).not.toContain(first.email);
    expect(container.innerHTML).not.toContain(second.email);
    expect(container.textContent).not.toContain("Personal");
    expect(container.textContent).not.toContain("Organization");
  });

  it("keeps opaque labels stable when the source reorders accounts", async () => {
    const { container } = render(<ClaudeAccountsMenu hideEmail />);
    await screen.findByText("Account 1");
    mocks.claudeAccountsList.mockResolvedValue([second, first]);
    await act(async () => window.dispatchEvent(new Event("focus")));
    await waitFor(() => {
      const labels = container.querySelectorAll(".codex-menu-accounts__email");
      expect(labels[0].firstChild?.textContent).toBe("Account 2");
      expect(labels[1].firstChild?.textContent).toBe("Account 1");
    });
  });

  it("shows switch failures and leaves the current account marked active", async () => {
    mocks.claudeAccountSwitch.mockRejectedValue("Close Claude Code first.");
    render(<ClaudeAccountsMenu hideEmail={false} />);
    await screen.findByText(first.email);
    const row = screen.getByText(second.email).closest("li") as HTMLElement;
    await act(async () => fireEvent.click(within(row).getByRole("button")));
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
