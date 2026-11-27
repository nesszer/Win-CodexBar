import { describe, expect, it } from "vitest";
import type { ProviderUsageSnapshot, RateWindowSnapshot } from "../types/bridge";
import { accountMenuAction } from "./accountMenuRow";

const window: RateWindowSnapshot = {
  descriptionIsDetail: false,
  reserveWillLastToReset: false,
  reserveEtaSeconds: null,
  pace: null,
  monthlyLimitBlock: null,
  usedPercent: 25,
  remainingPercent: 75,
  windowMinutes: 300,
  resetsAt: null,
  resetDescription: null,
  isExhausted: false,
  reservePercent: null,
  reserveDescription: null,
  isInformational: false,
};

function snapshot(overrides: Partial<ProviderUsageSnapshot>): ProviderUsageSnapshot {
  return {
    providerId: "codex",
    displayName: "Codex",
    primary: window,
    selectedMetric: window,
    primaryLabel: null,
    secondaryLabel: null,
    tertiaryLabel: null,
    accountLabel: null,
    subscription: null,
    hasSuccessfulClaudeCliQuota: false,
    fetchDurationMs: null,
    wayfinderUsage: null,
    secondary: null,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [],
    cost: null,
    planName: null,
    accountEmail: null,
    sourceLabel: "auto",
    updatedAt: "2026-10-11T00:00:00Z",
    error: null,
    errorState: "ready",
    pace: null,
    accountOrganization: null,
    trayStatusLabel: null,
    ...overrides,
  };
}

describe("accountMenuAction", () => {
  it("keeps Add Account... for a signed-in Codex, as upstream loginMenuAction does", () => {
    expect(
      accountMenuAction(snapshot({ providerId: "codex", accountEmail: "parity.user@example.com" })),
    ).toEqual({ labelKey: "AddAccountEllipsis", settingsTab: "providers:codex" });
  });

  it.each(["claude", "grok"])("offers Switch Account... for a signed-in %s", (providerId) => {
    expect(accountMenuAction(snapshot({ providerId, accountEmail: "a@example.com" }))).toEqual({
      labelKey: "TrayMenuSwitchAccount",
      settingsTab: `providers:${providerId}`,
    });
  });

  it.each(["codex", "claude", "grok"])("offers Add Account... when %s has no account", (providerId) => {
    expect(accountMenuAction(snapshot({ providerId }))).toEqual({
      labelKey: "AddAccountEllipsis",
      settingsTab: `providers:${providerId}`,
    });
  });

  it("treats a live Claude CLI quota as a signed-in account without an email", () => {
    expect(
      accountMenuAction(
        snapshot({ providerId: "claude", sourceLabel: "Claude CLI", hasSuccessfulClaudeCliQuota: true }),
      )?.labelKey,
    ).toBe("TrayMenuSwitchAccount");
  });

  it("treats a live Claude identity with quota windows as signed in, like upstream", () => {
    const oauth = snapshot({ providerId: "claude", sourceLabel: "oauth", planName: "Claude (OAuth)" });
    expect(accountMenuAction(oauth)?.labelKey).toBe("TrayMenuSwitchAccount");
    // No identity, no quota window, or a failed refresh does not prove an account.
    expect(accountMenuAction({ ...oauth, planName: " " })?.labelKey).toBe("AddAccountEllipsis");
    const informational = { ...window, isInformational: true };
    expect(
      accountMenuAction({ ...oauth, primary: informational, selectedMetric: informational })
        ?.labelKey,
    ).toBe("AddAccountEllipsis");
    expect(accountMenuAction({ ...oauth, error: "expired" })?.labelKey).toBe("AddAccountEllipsis");
    // The identity rule is Claude's; other providers still need an email.
    expect(
      accountMenuAction(snapshot({ providerId: "codex", planName: "ChatGPT Pro" }))?.labelKey,
    ).toBe("AddAccountEllipsis");
  });

  it("ignores a blank email", () => {
    expect(accountMenuAction(snapshot({ accountEmail: "  " }))?.labelKey).toBe("AddAccountEllipsis");
  });

  it("offers nothing for providers without an account manager", () => {
    expect(accountMenuAction(snapshot({ providerId: "cursor", accountEmail: "a@example.com" }))).toBeNull();
    expect(accountMenuAction(null)).toBeNull();
  });
});
