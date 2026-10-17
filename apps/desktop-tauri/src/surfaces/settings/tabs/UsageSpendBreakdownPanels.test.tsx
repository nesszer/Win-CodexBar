import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { SpendContract, SpendModelRow } from "../../../types/bridge";
import type { LocaleKey } from "../../../i18n/keys";
import { buildSpendProviderGroups } from "../../../lib/usageSpendBreakdown";
import { UsageSpendProviderGroups } from "./UsageSpendProviderGroups";

const t = (key: LocaleKey) => key;

const models: SpendModelRow[] = Array.from({ length: 8 }, (_, index) => ({
  model: `model-${index + 1}`,
  costUsd: 8 - index,
  inputTokens: 10,
  outputTokens: 0,
  cacheReadTokens: 0,
  totalTokens: 10,
  customPricing: false,
}));

const contract = {
  providerId: "codex",
  historyDays: 30,
  knownCostUsd: 36,
  historyCoverageEstablished: false,
  priceCoverage: { priced: 8, unpriced: 0, unmetered: 0, estimated: 0 },
  models,
  daily: [
    { day: "2026-09-18", costUsd: 4, totalTokens: 40 },
    { day: "2026-09-19", costUsd: 2, totalTokens: 20 },
  ],
  hourlyActivity: [{ weekday: 0, hour: 9, conversations: 3 }],
} as unknown as SpendContract;

describe("UsageSpendProviderGroups", () => {
  const groups = buildSpendProviderGroups(contract, "Codex");

  it("shows six models per provider, expands, and collapses the group", () => {
    render(<UsageSpendProviderGroups groups={groups} caption="Models for: Last 30 days" t={t} />);
    expect(screen.getByText("Models for: Last 30 days")).toBeTruthy();
    expect(screen.getByText("Codex")).toBeTruthy();
    expect(screen.getByText("UsageSpendPartialModelBreakdown")).toBeTruthy();
    expect(screen.queryByText("model-7")).toBeNull();
    expect(screen.getByText("model-6")).toBeTruthy();

    fireEvent.click(screen.getByText(/UsageSpendShowAll/));
    expect(screen.getByText("model-8")).toBeTruthy();
    fireEvent.click(screen.getByText("UsageSpendShowLess"));
    expect(screen.queryByText("model-8")).toBeNull();

    const header = screen.getByRole("button", { expanded: true });
    fireEvent.click(header);
    expect(screen.queryByText("model-1")).toBeNull();
    expect(header.getAttribute("aria-expanded")).toBe("false");
  });

  it("shows the empty state without groups", () => {
    render(<UsageSpendProviderGroups groups={[]} caption="Models for: Last 30 days" t={t} />);
    expect(screen.getByText("UsageSpendNoModels")).toBeTruthy();
  });
});
