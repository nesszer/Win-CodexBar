import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ClaudeAccount } from "../types/bridge";

const mocks = vi.hoisted(() => ({ claudeAccountsList: vi.fn(), claudeAccountSwitch: vi.fn(), refreshProviders: vi.fn() }));
vi.mock("../lib/tauri", () => mocks);
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../hooks/useLocale", () => ({ useLocale: () => ({ t: (key: string) => key }) }));
import ClaudeAccountsMenu from "./ClaudeAccountsMenu";

const first: ClaudeAccount = { id: "first:org", email: "first@example.com", organization: "Personal", plan: "max", isActive: true, isSaved: true };
const second: ClaudeAccount = { ...first, id: "second:org", email: "second@example.com", organization: "Work", isActive: false };

describe("ClaudeAccountsMenu", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.claudeAccountsList.mockResolvedValue([first, second]);
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

  it("uses stable opaque account labels and redacts tooltips when hideEmail is enabled", async () => {
    mocks.claudeAccountsList.mockResolvedValue([first, { ...second, organization: `${second.email}'s Organization` }]);
    const { container } = render(<ClaudeAccountsMenu hideEmail />);
    await screen.findByText("ClaudeAccountsTitle");
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
    await screen.findByText("ClaudeAccountsTitle");
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
