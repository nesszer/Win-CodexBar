import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GrokAccount } from "../../../../../types/bridge";

const mocks = vi.hoisted(() => ({
  grokAccountsList: vi.fn(),
  grokAccountAdd: vi.fn(),
  grokAccountCancelLogin: vi.fn(),
  grokAccountSaveCurrent: vi.fn(),
  grokAccountRemove: vi.fn(),
  grokAccountSwitch: vi.fn(),
  grokAccountFetch: vi.fn(),
  grokAccountReauthenticate: vi.fn(),
}));
const events = vi.hoisted(() => ({
  listen: vi.fn<(event: string, listener: () => void) => Promise<() => void>>(),
}));
vi.mock("../../../../../lib/tauri", () => mocks);
vi.mock("@tauri-apps/api/event", () => events);
vi.mock("../../../../../hooks/useLocale", () => ({ useLocale: () => ({ t: (key: string) => key }) }));
import { GrokAccountsSection } from "./GrokAccountsSection";

const t = (key: string) => key;
const current: GrokAccount = {
  id: "user-one",
  email: "one@example.com",
  organization: "team",
  plan: "SuperGrok",
  isActive: true,
  isSaved: false,
};
const other: GrokAccount = {
  ...current,
  id: "user-two",
  email: "two@example.com",
  isActive: false,
  isSaved: true,
};

describe("GrokAccountsSection", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    events.listen.mockResolvedValue(() => {});
    mocks.grokAccountsList.mockResolvedValue([current, other]);
    mocks.grokAccountFetch.mockResolvedValue({
      usageAvailable: true,
      usedPercent: 10,
      plan: "SuperGrok",
      windowMinutes: 10080,
      resetsAt: "2026-09-25T05:55:45Z",
    });
  });

  it("shows Add account and switches only saved inactive accounts", async () => {
    render(<GrokAccountsSection t={t} />);
    await screen.findByText(current.email);
    expect(screen.getByText("CodexAccountsAddButton")).toBeInTheDocument();
    expect(screen.getAllByText("CodexAccountsSwitchButton")).toHaveLength(1);
    await act(async () => fireEvent.click(screen.getByText("GrokAccountsSaveCurrent")));
    expect(mocks.grokAccountSaveCurrent).toHaveBeenCalledOnce();
    await act(async () => fireEvent.click(screen.getByText("CodexAccountsSwitchButton")));
    expect(mocks.grokAccountSwitch).toHaveBeenCalledWith(other.id);
    expect(screen.getByRole("status").textContent).toBe("GrokAccountsSwitched");
  });

  it("keeps mutations disabled during browser login and supports cancel", async () => {
    let finish!: () => void;
    mocks.grokAccountAdd.mockImplementation(
      () => new Promise<void>((resolve) => { finish = resolve; }),
    );
    mocks.grokAccountCancelLogin.mockResolvedValue(undefined);
    render(<GrokAccountsSection t={t} />);
    await screen.findByText(current.email);
    fireEvent.click(screen.getByText("CodexAccountsAddButton"));
    expect(screen.getByText("GrokAccountsSigningIn")).toBeInTheDocument();
    await act(async () => fireEvent.click(screen.getByText("GrokAccountsCancelLogin")));
    expect(mocks.grokAccountCancelLogin).toHaveBeenCalledOnce();
    await act(async () => finish());
    expect(screen.queryByText("GrokAccountsCancelLogin")).toBeNull();
    expect((screen.getByText("CodexAccountsAddButton") as HTMLButtonElement).disabled).toBe(false);
  });

  it("shows a switch error without claiming success", async () => {
    mocks.grokAccountSwitch.mockRejectedValue("Grok sign-in did not complete.");
    render(<GrokAccountsSection t={t} />);
    await screen.findByText(other.email);
    await act(async () => fireEvent.click(screen.getByText("CodexAccountsSwitchButton")));
    expect(screen.getByRole("alert").textContent).toContain("Grok sign-in did not complete.");
    expect(screen.queryByText("GrokAccountsSwitched")).toBeNull();
  });

  it("shows initial loading errors while keeping Add account available", async () => {
    mocks.grokAccountsList.mockRejectedValue("Could not read saved accounts.");
    render(<GrokAccountsSection t={t} />);
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("Could not read"));
    expect((screen.getByText("CodexAccountsAddButton") as HTMLButtonElement).disabled).toBe(false);
  });
});
