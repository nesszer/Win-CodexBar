import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { SpendProviderGroup } from "../../../lib/usageSpendBreakdown";
import type { SpendModelRow } from "../../../types/bridge";
import { UsageSpendProviderGroups } from "./UsageSpendProviderGroups";

const t = (key: string) => key;

function model(name: string): SpendModelRow {
  return {
    model: name,
    costUsd: 1,
    inputTokens: 10,
    outputTokens: 5,
    cacheReadTokens: 0,
    totalTokens: 15,
    customPricing: false,
  };
}

function group(providerId: string, displayName: string, models: SpendModelRow[], extra: Partial<SpendProviderGroup> = {}): SpendProviderGroup {
  return {
    providerId,
    displayName,
    costUsd: 1,
    totalTokens: 15,
    costIsPartial: false,
    hasPartialModelHistory: false,
    models,
    ...extra,
  };
}

describe("UsageSpendProviderGroups brand rows", () => {
  it("shows the brand artwork in the provider heading", () => {
    const { container } = render(
      <UsageSpendProviderGroups groups={[group("claude", "Claude", [model("opus")])]} caption="caption" t={t} showHeading={false} />,
    );
    const header = container.querySelector("button");
    expect(header?.querySelector(".spend-provider-icon--brand")).not.toBeNull();
  });

  it("keeps model child rows monochrome", () => {
    const { container } = render(
      <UsageSpendProviderGroups
        groups={[group("claude", "Claude", [model("opus"), model("sonnet")])]}
        caption="caption"
        t={t}
        showHeading={false}
      />,
    );
    expect(container.querySelector(".spend-provider-icon--brand")).not.toBeNull();
    const childIcons = container.querySelectorAll(".usage-spend-provider__model .spend-provider-icon--monochrome");
    expect(childIcons).toHaveLength(2);
    expect(container.querySelectorAll(".usage-spend-provider__model .spend-provider-icon--brand")).toHaveLength(0);
  });

  it("hides the Models label for a single-model provider", () => {
    render(
      <UsageSpendProviderGroups groups={[group("claude", "Claude", [model("opus")])]} caption="caption" t={t} showHeading={false} />,
    );
    expect(screen.queryByText("UsageSpendModels")).toBeNull();
  });

  it("shows the Models label when a provider has several models", () => {
    render(
      <UsageSpendProviderGroups
        groups={[group("claude", "Claude", [model("opus"), model("sonnet")])]}
        caption="caption"
        t={t}
        showHeading={false}
      />,
    );
    expect(screen.getByText("UsageSpendModels")).not.toBeNull();
  });

  it("keeps the Models label for partial single-model history", () => {
    render(
      <UsageSpendProviderGroups
        groups={[group("claude", "Claude", [model("opus")], { hasPartialModelHistory: true })]}
        caption="caption"
        t={t}
        showHeading={false}
      />,
    );
    expect(screen.getByText("UsageSpendModels")).not.toBeNull();
  });
});
