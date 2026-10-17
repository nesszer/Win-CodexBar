import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ProviderDetail } from "../../../../types/bridge";
import { UsageItemVisibilitySection } from "./UsageItemVisibilitySection";

function provider(): ProviderDetail {
  return {
    id: "codex",
    displayName: "Codex",
    enabled: true,
    autoResumeAfterQuotaReset: false,
    autoResumeSupported: false,
    optionalDetailsSupported: false,
    optionalDetailsEnabled: false,
    email: null,
    plan: null,
    authType: null,
    sourceLabel: "OAuth",
    organization: null,
    lastUpdated: null,
    session: null,
    weekly: null,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [],
    usageItems: [
      { id: "metric:primary", title: "Session", available: true },
      { id: "metric:extra-codex-spark", title: "Codex Spark", available: false },
    ],
    hiddenUsageItemIds: ["metric:extra-codex-spark"],
    cost: null,
    pace: null,
    lastError: null,
    errorState: null,
    dashboardUrl: null,
    statusPageUrl: null,
    buyCreditsUrl: null,
    hasSnapshot: true,
    cookieSource: null,
    region: null,
  };
}

describe("UsageItemVisibilitySection", () => {
  it("persists a stable raw ID and restores the explicit defaults", () => {
    const onChange = vi.fn();
    render(
      <UsageItemVisibilitySection
        provider={provider()}
        disabled={false}
        t={(key) => key}
        onChange={onChange}
      />,
    );

    expect(screen.getByRole("checkbox", { name: "ProviderSessionLabel" })).toBeChecked();
    expect(
      screen.getByRole("checkbox", { name: /Codex Spark/ }),
    ).not.toBeChecked();

    fireEvent.click(screen.getByRole("checkbox", { name: "ProviderSessionLabel" }));
    expect(onChange).toHaveBeenCalledWith({
      providerHiddenUsageItemIds: {
        codex: ["metric:extra-codex-spark", "metric:primary"],
      },
    });

    fireEvent.click(screen.getByRole("button", { name: "ProviderRestoreDefaults" }));
    expect(onChange).toHaveBeenLastCalledWith({
      providerHiddenUsageItemIds: { codex: [] },
    });
  });

  it("explains a cost-gated row only while Cost summary display is Hidden", () => {
    const claude = {
      ...provider(),
      id: "claude",
      usageItems: [
        { id: "section:subscription", title: "Plan renewal / expiry", available: true },
        {
          id: "section:claude-extra-usage",
          title: "Extra usage",
          available: true,
          requiresCostSummary: true,
        },
      ],
      hiddenUsageItemIds: [],
    };
    const onChange = vi.fn();
    const { rerender } = render(
      <UsageItemVisibilitySection provider={claude} disabled={false} t={(key) => key} onChange={onChange} />,
    );
    expect(screen.queryByText("UsageItemHiddenByCostSummary")).not.toBeInTheDocument();

    rerender(
      <UsageItemVisibilitySection
        provider={claude}
        disabled={false}
        t={(key) => key}
        onChange={onChange}
        costSummaryHidden
      />,
    );
    expect(screen.getAllByText("UsageItemHiddenByCostSummary")).toHaveLength(1);
    // The choice stays usable; it does not override the global display style.
    fireEvent.click(screen.getByRole("checkbox", { name: /Extra/ }));
    expect(onChange).toHaveBeenCalledWith({
      providerHiddenUsageItemIds: { claude: ["section:claude-extra-usage"] },
    });
  });
});
