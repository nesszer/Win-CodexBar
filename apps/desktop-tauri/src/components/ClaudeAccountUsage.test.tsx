import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ClaudeAccount } from "../types/bridge";
import ClaudeAccountUsage from "./ClaudeAccountUsage";

vi.mock("../hooks/useLocale", () => ({ useLocale: () => ({ t: (key: string) => key }) }));
const t = (key: string) => key;
const account: ClaudeAccount = {
  id: "one:org", email: "one@example.test", organization: null, plan: "max", isActive: false, isSaved: true,
  usage: { fiveHour: { usedPercent: 23, resetsAt: null }, sevenDay: { usedPercent: 81, resetsAt: null }, updatedAt: "2026-10-03T12:00:00Z" },
};

describe("ClaudeAccountUsage", () => {
  it("shows independent session and weekly remaining bars with optional used values", () => {
    const { container } = render(<ClaudeAccountUsage account={account} t={t} showAsUsed />);
    expect(screen.getByText("77% PanelLeftSuffix")).toBeInTheDocument();
    expect(screen.getByText("19% PanelLeftSuffix")).toBeInTheDocument();
    expect(screen.getByText("23% PanelUsedSuffix")).toBeInTheDocument();
    const bars = container.querySelectorAll<HTMLElement>(".codex-menu-accounts__bar-fill");
    expect(Array.from(bars, bar => bar.style.width)).toEqual(["77%", "19%"]);
  });

  it("keeps last known limits but marks failed checks visibly", () => {
    const { container } = render(<ClaudeAccountUsage account={{ ...account, usageError: "Sign in again" }} t={t} />);
    expect(screen.getByRole("status")).toHaveTextContent("Sign in again");
    expect(screen.getByText("77% PanelLeftSuffix")).toBeInTheDocument();
    expect(container.querySelector('[data-stale="true"]')).toBeInTheDocument();
  });

  it("does not invent quota when an account has never returned usage", () => {
    const { container } = render(<ClaudeAccountUsage account={{ ...account, usage: null }} t={t} />);
    expect(screen.getByText("CodexAccountsUsageUnavailable")).toBeInTheDocument();
    expect(container.querySelector(".codex-menu-accounts__bar-fill")).toBeNull();
  });
});
