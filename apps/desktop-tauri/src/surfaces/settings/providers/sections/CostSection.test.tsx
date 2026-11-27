import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import type { CostSnapshotBridge } from "../../../../types/bridge";
import type { LocaleKey } from "../../../../i18n/keys";
import { CostSection } from "./CostSection";

describe("CostSection", () => {
  it("does not present the zero usage carrier as spend for balance-only providers", () => {
    const cost: CostSnapshotBridge = {
      balanceUpdatedAt: null,
      accountId: null,
      daily: [],
      alwaysVisible: false,
      used: 0,
      limit: null,
      remaining: null,
      currencyCode: "USD",
      period: "Atlas Cloud balance",
      resetsAt: null,
      formattedUsed: "$0.00",
      formattedLimit: null,
      balance: 95.5,
      formattedBalance: "$95.50",
    };

    const { container } = render(
      <CostSection cost={cost} t={(key: LocaleKey) => key} />,
    );

    expect(container.firstChild).toBeNull();
    expect(screen.queryByText("DetailCostUsed")).toBeNull();
    expect(screen.queryByText("$0.00")).toBeNull();
  });
});
