import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GrokAccount, ProviderUsageSnapshot } from "../types/bridge";

const mocks = vi.hoisted(() => ({
  grokAccountsList: vi.fn(),
  grokAccountSwitch: vi.fn(),
  grokAccountFetch: vi.fn(),
  grokAccountReauthenticate: vi.fn(),
  grokAccountCancelLogin: vi.fn(),
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
}));
vi.mock("../lib/tauri", () => mocks);
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, callback: (event: { payload: unknown }) => void) => {
    mocks.listeners.set(event, callback);
    return Promise.resolve(() => mocks.listeners.delete(event));
  },
}));
vi.mock("../hooks/useLocale", () => ({ useLocale: () => ({ t: (key: string) => key }) }));
import GrokAccountsMenu from "./GrokAccountsMenu";

const first: GrokAccount = {
  id: "user-one",
  email: "one@example.com",
  organization: null,
  plan: "SuperGrok",
  isActive: true,
  isSaved: true,
};
const second: GrokAccount = {
  ...first,
  id: "user-two",
  email: "two@example.com",
  isActive: false,
};

function cardSnapshot(): Pick<ProviderUsageSnapshot,
  "accountEmail" | "accountOrganization" | "primary" | "planName" | "error"
> {
  return {
    accountEmail: first.email,
    accountOrganization: null,
    planName: "SuperGrok",
    error: null,
    primary: {
      usedPercent: 47,
      remainingPercent: 53,
      windowMinutes: 10080,
      resetsAt: null,
      resetDescription: null,
      isExhausted: false,
      reservePercent: null,
      reserveDescription: null,
    },
  };
}

describe("GrokAccountsMenu", () => {
  it("allows switching a saved account while its usage request is pending", async () => {
    mocks.grokAccountFetch.mockImplementation(() => new Promise(() => {}));
    mocks.grokAccountSwitch.mockResolvedValue(undefined);
    render(<GrokAccountsMenu hideEmail={false} resetTimeRelative />);
    await screen.findByText(second.email);
    const buttons = screen.getAllByRole("button", { name: "CodexAccountsSwitchButton" });
    expect(buttons[1]).toBeEnabled();
    fireEvent.click(buttons[1]);
    await waitFor(() => expect(mocks.grokAccountSwitch).toHaveBeenCalledWith(second.id));
  });

  it("shows the other account's reading while one account still loads", async () => {
    mocks.grokAccountsList.mockResolvedValue([first, second]);
    mocks.grokAccountFetch.mockImplementation((id: string) => id === first.id
      ? new Promise(() => {})
      : Promise.resolve({ status: "ready", usageAvailable: true, usedPercent: 23, resetsAt: null, windowMinutes: 10080, plan: null }));
    render(<GrokAccountsMenu hideEmail={false} resetTimeRelative />);
    expect(await screen.findByText("23% PanelUsedSuffix")).toBeInTheDocument();
    expect(screen.getByText("GrokUsageLoading")).toBeInTheDocument();
    expect(screen.getByText("GrokUsageResetUnavailable")).toBeInTheDocument();
  });

  it("reauthenticates the selected expired account without calling Switch", async () => {
    mocks.grokAccountFetch.mockImplementation(async (id: string) => ({
      status: id === second.id ? "signInRequired" : "ready",
      usageAvailable: id !== second.id, usedPercent: id === second.id ? null : 0,
      resetsAt: null, windowMinutes: null, plan: null,
    }));
    mocks.grokAccountReauthenticate.mockResolvedValue(undefined);
    render(<GrokAccountsMenu hideEmail={false} resetTimeRelative />);
    fireEvent.click(await screen.findByText("GrokAccountsSignInAgain"));
    await waitFor(() => expect(mocks.grokAccountReauthenticate).toHaveBeenCalledWith(second.id));
    expect(mocks.grokAccountSwitch).not.toHaveBeenCalled();
    expect(await screen.findByText("0% PanelUsedSuffix")).toBeInTheDocument();
    expect(screen.queryByText("GrokAccountsSwitched")).toBeNull();
  });

  beforeEach(() => {
    vi.resetAllMocks();
    mocks.listeners.clear();
    mocks.grokAccountsList.mockResolvedValue([first, second]);
    mocks.grokAccountFetch.mockImplementation(async (id: string) => ({
      usageAvailable: true,
      usedPercent: id === first.id ? 38 : 100,
      plan: "SuperGrok",
      windowMinutes: 10080,
      resetsAt: "2026-09-25T05:55:45Z",
    }));
  });

  it("shows Codex-style usage bars for each Grok account", async () => {
    const { container } = render(
      <GrokAccountsMenu hideEmail={false} resetTimeRelative />,
    );
    await screen.findByText(first.email);
    expect(screen.getAllByText("7d").length).toBeGreaterThan(0);
    expect(screen.getByText("38% PanelUsedSuffix")).toBeInTheDocument();
    expect(screen.getByText("100% PanelUsedSuffix")).toBeInTheDocument();
    const bars = container.querySelectorAll(".codex-menu-accounts__bar-fill");
    expect(bars).toHaveLength(2);
    expect((bars[0] as HTMLElement).style.width).toBe("38%");
    expect((bars[1] as HTMLElement).style.width).toBe("100%");
  });

  it("omits percentage and bar when usage is unavailable", async () => {
    mocks.grokAccountFetch.mockResolvedValue({
      usageAvailable: false,
      usedPercent: null,
      plan: "SuperGrok",
      windowMinutes: 10080,
      resetsAt: null,
    });
    const { container } = render(
      <GrokAccountsMenu hideEmail={false} resetTimeRelative />,
    );

    await screen.findByText(first.email);
    expect(screen.queryByText(/PanelUsedSuffix/)).toBeNull();
    expect(container.querySelectorAll(".codex-menu-accounts__bar-fill")).toHaveLength(0);
  });

  it("refreshes both account rows on Grok provider updates without a button click", async () => {
    const { unmount } = render(<GrokAccountsMenu hideEmail={false} resetTimeRelative />);
    await screen.findByText("38% PanelUsedSuffix");
    expect(mocks.grokAccountFetch).toHaveBeenCalledTimes(2);

    await act(async () => {
      mocks.listeners.get("provider-updated")?.({ payload: { providerId: "codex" } });
    });
    expect(mocks.grokAccountFetch).toHaveBeenCalledTimes(2);

    mocks.grokAccountFetch.mockImplementation(async (id: string) => ({
      status: "ready", usageAvailable: true,
      usedPercent: id === first.id ? 42 : 56,
      plan: "SuperGrok", windowMinutes: 10080,
      resetsAt: "2026-10-05T05:55:45Z",
    }));
    await act(async () => {
      mocks.listeners.get("provider-updated")?.({ payload: { providerId: "grok" } });
    });
    expect(await screen.findByText("42% PanelUsedSuffix")).toBeInTheDocument();
    expect(screen.getByText("56% PanelUsedSuffix")).toBeInTheDocument();
    expect(mocks.grokAccountFetch).toHaveBeenCalledTimes(4);

    unmount();
    await act(async () => {});
    expect(mocks.listeners.has("provider-updated")).toBe(false);
    expect(mocks.listeners.has("grok-accounts-updated")).toBe(false);
  });

  it("uses matching card usage only for the active account when a fetch fails", async () => {
    mocks.grokAccountFetch.mockRejectedValue(new Error("Unavailable"));
    const { container } = render(
      <GrokAccountsMenu hideEmail={false} resetTimeRelative cardSnapshot={cardSnapshot()} />,
    );
    await screen.findByText(first.email);
    expect(screen.getByText("47% PanelUsedSuffix")).toBeInTheDocument();
    expect(container.querySelectorAll(".codex-menu-accounts__bar-fill")).toHaveLength(1);
  });

  it("preserves account-specific usage when a matching card has a different value", async () => {
    render(<GrokAccountsMenu hideEmail={false} resetTimeRelative cardSnapshot={cardSnapshot()} />);
    await screen.findByText(first.email);
    expect(screen.getByText("38% PanelUsedSuffix")).toBeInTheDocument();
    expect(screen.queryByText("47% PanelUsedSuffix")).toBeNull();
  });

  it.each(["different account", "unknown identity", "different organization", "informational", "error"])(
    "does not borrow %s card usage", async (scenario) => {
      mocks.grokAccountFetch.mockRejectedValue(new Error("Unavailable"));
      const card = cardSnapshot();
      if (scenario === "different account") card.accountEmail = second.email;
      if (scenario === "unknown identity") card.accountEmail = null;
      if (scenario === "different organization") card.accountOrganization = "other-team";
      if (scenario === "informational") card.primary.isInformational = true;
      if (scenario === "error") card.error = "Expired credentials";
      const { container } = render(
        <GrokAccountsMenu hideEmail={false} resetTimeRelative cardSnapshot={card} />,
      );
      await screen.findByText(first.email);
      expect(screen.queryByText(/PanelUsedSuffix/)).toBeNull();
      expect(container.querySelectorAll(".codex-menu-accounts__bar-fill")).toHaveLength(0);
    },
  );
});
