import { describe, expect, it } from "vitest";
import type { CostSnapshotBridge } from "../types/bridge";
import type { LocaleKey } from "../i18n/keys";
import { cardCostSection } from "./cardCost";

const STRINGS: Partial<Record<LocaleKey, string>> = {
  ExtraUsage: "Extra usage",
  QuotaUsageTitle: "Quota usage",
  ApiSpendTitle: "API spend",
  DetailCostBalance: "Balance",
  DetailCostResets: "Resets",
  UsedPercent: '{:.0}% used',
  CostPeriodLastDays: "Last {} days",
  CostPeriodThisMonth: "This month",
  PanelToday: "Today",
  ProviderLabelMonthly: "Monthly",
  WindowLabelMonthlyCredits: "Monthly credits",
  CreditsTitle: "Credits",
  CostTitleClawRouterMonthlyBudget: "Monthly budget",
  CostTitleClawRouterSpend: "ClawRouter spend",
};
const t = (key: LocaleKey) => STRINGS[key] ?? key;

function cost(overrides: Partial<CostSnapshotBridge>): CostSnapshotBridge {
  return {
    used: 0,
    limit: null,
    remaining: null,
    currencyCode: "USD",
    period: "",
    resetsAt: null,
    formattedUsed: "",
    formattedLimit: null,
    ...overrides,
  };
}

describe("cardCostSection", () => {
  it("returns nothing without a cost", () => {
    expect(cardCostSection(null, "detailed", t)).toBeNull();
  });

  it("hides the block when the summary style is hidden", () => {
    expect(cardCostSection(cost({ used: 3, limit: 10 }), "hidden", t)).toBeNull();
  });

  it("keeps always-visible API spend under the hidden style", () => {
    const section = cardCostSection(
      cost({ used: 4, period: "Last 30 days", alwaysVisible: true }),
      "hidden",
      t,
    );
    expect(section).toEqual({
      title: "API spend",
      spendLine: "Last 30 days: $4.00",
      percentUsed: null,
      percentLine: null,
      balanceLine: null,
      inline: false,
    });
  });

  it("uses Last 30 days as the API spend default period", () => {
    const section = cardCostSection(
      cost({ used: 1.5, alwaysVisible: true, formattedUsed: "$1.50" }),
      "detailed",
      t,
    );
    expect(section?.spendLine).toBe("Last 30 days: $1.50");
  });

  it("renders a capped spend as Extra usage with a percent", () => {
    const section = cardCostSection(
      cost({
        used: 12.5,
        limit: 50,
        period: "Monthly cap",
        formattedUsed: "$12.50",
        formattedLimit: "$50.00",
      }),
      "detailed",
      t,
    );
    expect(section).toEqual({
      title: "Extra usage",
      spendLine: "Monthly cap: $12.50 / $50.00",
      percentUsed: 25,
      percentLine: "25% used",
      balanceLine: null,
      inline: false,
    });
  });

  it("defaults a capped spend period to This month and adds the balance", () => {
    const section = cardCostSection(
      cost({ used: 60, limit: 50, balance: 8 }),
      "detailed",
      t,
    );
    expect(section?.spendLine).toBe("This month: $60.00 / $50.00");
    expect(section?.percentUsed).toBe(100);
    expect(section?.percentLine).toBe("100% used");
    expect(section?.balanceLine).toBe("Balance: $8.00");
  });

  it("keeps a folded capped spend as a caption without a second bar", () => {
    const section = cardCostSection(
      cost({
        used: 12.5,
        limit: 50,
        period: "Monthly",
        formattedUsed: "$12.50",
        formattedLimit: "$50.00",
        balance: 8,
        foldedIntoPrimary: true,
      }),
      "detailed",
      t,
    );
    expect(section).toEqual({
      title: "Extra usage",
      spendLine: "Monthly: $12.50 / $50.00",
      percentUsed: null,
      percentLine: null,
      balanceLine: "Balance: $8.00",
      inline: false,
    });
  });

  it("drops the balance line in the compact summary style", () => {
    const section = cardCostSection(cost({ used: 1, limit: 4, balance: 8 }), "compact", t);
    expect(section?.balanceLine).toBeNull();
  });

  it("titles a Quota currency as Quota usage with whole numbers", () => {
    const section = cardCostSection(
      cost({ used: 120.4, limit: 500, currencyCode: "Quota", period: "This month" }),
      "detailed",
      t,
    );
    expect(section?.title).toBe("Quota usage");
    expect(section?.spendLine).toBe("This month: 120 / 500");
    expect(section?.percentLine).toBe("24% used");
  });

  it("shows a balance-only cost inline", () => {
    const section = cardCostSection(
      cost({ used: 0, balance: 31.58, formattedBalance: "$31.58" }),
      "detailed",
      t,
    );
    expect(section).toEqual({
      title: "Extra usage",
      spendLine: "Balance: $31.58",
      percentUsed: null,
      percentLine: null,
      balanceLine: null,
      inline: true,
    });
  });

  it("reads a credits-labelled cost without a cap as a balance", () => {
    const section = cardCostSection(
      cost({ used: 125.5, period: "Credits" }),
      "detailed",
      t,
    );
    expect(section?.title).toBe("Extra usage");
    expect(section?.spendLine).toBe("Balance: $125.50");
    expect(section?.inline).toBe(true);
  });

  it("prints Codex credits unitless, as upstream creditsUsageSection does", () => {
    const balance = cardCostSection(
      cost({ used: 125.5, period: "Credits", formattedUsed: "$125.50" }),
      "detailed",
      t,
      "codex",
    );
    expect(balance?.spendLine).toBe("Balance: 125.5");
    expect(balance?.inline).toBe(true);

    const capped = cardCostSection(
      cost({ used: 1234.567, limit: 2000, period: "Monthly credits", formattedUsed: "$1,234.57" }),
      "detailed",
      t,
      "codex",
    );
    expect(capped?.spendLine).toBe("Monthly credits: 1,234.57 / 2,000");
  });

  it("shows an uncapped spend with its period", () => {
    const section = cardCostSection(
      cost({ used: 2, currencyCode: "EUR", period: "Monthly", currencySymbol: "€" }),
      "detailed",
      t,
    );
    expect(section?.title).toBe("Extra usage");
    expect(section?.spendLine).toBe("Monthly: €2.00");
    expect(section?.inline).toBe(true);
  });

  // Upstream `ProviderCostMenuCardStyle`: the provider names its block.
  it("uses the provider's card title for capped, inline and balance blocks", () => {
    const budget = cardCostSection(
      cost({ used: 18.42, limit: 50, period: "This month", titleKey: "CostTitleClawRouterMonthlyBudget" }),
      "detailed",
      t,
      "clawrouter",
    );
    expect(budget?.title).toBe("Monthly budget");
    expect(budget?.spendLine).toBe("This month: $18.42 / $50.00");

    const spend = cardCostSection(
      cost({ used: 18.42, period: "This month", titleKey: "CostTitleClawRouterSpend" }),
      "detailed",
      t,
      "clawrouter",
    );
    expect(spend?.title).toBe("ClawRouter spend");
    expect(spend?.spendLine).toBe("This month: $18.42");

    const credits = cardCostSection(
      cost({ used: 0, balance: 42.37, period: "Prepaid credits", titleKey: "CreditsTitle" }),
      "detailed",
      t,
      "lithosai",
    );
    expect(credits?.title).toBe("Credits");
    expect(credits?.spendLine).toBe("Balance: $42.37");
  });
});
