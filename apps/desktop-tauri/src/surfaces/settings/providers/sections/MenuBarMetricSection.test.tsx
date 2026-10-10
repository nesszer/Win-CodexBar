import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ProviderDetail } from "../../../../types/bridge";
import { MenuBarMetricSection } from "./MenuBarMetricSection";
import { makeRateWindow } from "../../../../test/fixtures";

function provider(extra = true): ProviderDetail {
  return {
    id: "copilot",
    displayName: "GitHub Copilot",
    enabled: true,
    autoResumeAfterQuotaReset: false,
    autoResumeSupported: false,
    optionalDetailsSupported: false,
    optionalDetailsEnabled: false,
    email: null,
    plan: null,
    authType: null,
    sourceLabel: null,
    organization: null,
    lastUpdated: null,
    session: null,
    weekly: null,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: extra
      ? [{ id: "additional_budget", title: "Additional Budget", window: makeRateWindow(42) }]
      : [],
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

function mistral(observed = true): ProviderDetail {
  return {
    ...provider(false),
    id: "mistral",
    displayName: "Mistral",
    primaryMetricLabel: "Included API",
    monthlyPlanWindowId: "mistral-monthly-plan",
    session: observed ? makeRateWindow(2) : null,
    extraRateWindows: observed
      ? [{ id: "mistral-monthly-plan", title: "Monthly Plan", window: makeRateWindow(42) }]
      : [],
    hasSnapshot: observed,
  };
}

function optionNames() {
  return screen.getAllByRole("option").map((option) => option.textContent);
}

describe("MenuBarMetricSection", () => {
  it("renders the provider-declared tertiary label key before observation", () => {
    const base = provider(false);
    base.id = "opencodego";
    base.displayName = "OpenCode Go";
    base.tertiary = null;
    base.tertiaryLabelKey = "ProviderMonthly";
    const onChange = vi.fn();
    const { rerender } = render(
      <MenuBarMetricSection
        provider={base}
        providerMetrics={{}}
        disabled={false}
        t={(key) => key}
        onChange={onChange}
      />,
    );

    expect(screen.getByRole("option", { name: "ProviderMonthly" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "tertiary" } });
    expect(onChange).toHaveBeenCalledWith({
      providerMetrics: { opencodego: "tertiary" },
    });

    const observed = { ...base, tertiary: makeRateWindow(37) };
    rerender(
      <MenuBarMetricSection
        provider={observed}
        providerMetrics={{}}
        disabled={false}
        t={(key) => key}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByRole("option", { name: "ProviderMonthly" })).toBeInTheDocument();
  });

  it("keeps the generic tertiary label when no provider key is declared", () => {
    const base = provider(false);
    base.tertiary = makeRateWindow(37);

    render(
      <MenuBarMetricSection
        provider={base}
        providerMetrics={{}}
        disabled={false}
        t={(key) => key}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByRole("option", { name: "DetailWindowTertiary" })).toBeInTheDocument();
  });

  it("offers extra usage when a provider has extra rate windows", () => {
    const onChange = vi.fn();
    render(
      <MenuBarMetricSection
        provider={provider()}
        providerMetrics={{}}
        disabled={false}
        t={(key) => key}
        onChange={onChange}
      />,
    );

    fireEvent.change(screen.getByRole("combobox"), { target: { value: "extraUsage" } });

    expect(screen.getByRole("option", { name: "ExtraUsage" })).toBeInTheDocument();
    expect(onChange).toHaveBeenCalledWith({
      providerMetrics: { copilot: "extraUsage" },
    });
    expect(screen.queryByRole("option", { name: "MetricMonthlyPlan" })).toBeNull();
  });

  it("offers Included API and Monthly Plan for a provider plan window", () => {
    const onChange = vi.fn();
    render(
      <MenuBarMetricSection
        provider={mistral()}
        providerMetrics={{}}
        disabled={false}
        t={(key) => key}
        onChange={onChange}
      />,
    );

    // Upstream 0.70.0: Automatic, Included API and Monthly Plan only. The
    // plan window does not add a separate Extra usage choice.
    expect(optionNames()).toEqual(["Automatic", "Included API", "MetricMonthlyPlan"]);
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "monthlyPlan" } });
    expect(onChange).toHaveBeenCalledWith({
      providerMetrics: { mistral: "monthlyPlan" },
    });
  });

  it("offers Monthly Plan before the first Mistral snapshot", () => {
    render(
      <MenuBarMetricSection
        provider={mistral(false)}
        providerMetrics={{ mistral: "monthlyPlan" }}
        disabled={false}
        t={(key) => key}
        onChange={vi.fn()}
      />,
    );

    expect(optionNames()).toEqual(["Automatic", "Included API", "MetricMonthlyPlan"]);
    expect(screen.getByRole("combobox")).toHaveValue("monthlyPlan");
  });

  it("keeps a saved choice the provider no longer offers readable", () => {
    render(
      <MenuBarMetricSection
        provider={mistral()}
        providerMetrics={{ mistral: "extraUsage" }}
        disabled={false}
        t={(key) => key}
        onChange={vi.fn()}
      />,
    );

    expect(optionNames()).toEqual([
      "Automatic",
      "Included API",
      "MetricMonthlyPlan",
      "ExtraUsage",
    ]);
    expect(screen.getByRole("combobox")).toHaveValue("extraUsage");
  });

  it("offers only Automatic for Aixy, even with extra budget windows", () => {
    const aixy = provider();
    aixy.id = "aixy";
    aixy.displayName = "Aixy";
    aixy.weekly = makeRateWindow(40);
    render(
      <MenuBarMetricSection
        provider={aixy}
        providerMetrics={{ aixy: "weekly" }}
        disabled={false}
        t={(key) => key}
        onChange={vi.fn()}
      />,
    );

    const options = screen.getAllByRole("option").map((option) => option.textContent);
    expect(options).toEqual(["Automatic"]);
    expect(screen.getByRole("combobox")).toHaveValue("automatic");
  });
it("offers the provider-declared lane labels in the metric picker", () => {
    const base = provider(false);
    base.id = "litellm";
    base.displayName = "LiteLLM";
    base.weekly = makeRateWindow(30);
    base.primaryLabel = "Fuel Pack";
    base.secondaryLabel = "Gemini Pro";

    render(
      <MenuBarMetricSection
        provider={base}
        providerMetrics={{}}
        disabled={false}
        t={(key) => key}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByRole("option", { name: "Fuel Pack" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "Gemini Pro" })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "ProviderSessionLabel" })).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "ProviderWeeklyLabel" })).not.toBeInTheDocument();
  });

it("keeps the generic metric labels when the provider declares none", () => {
    const base = provider(false);
    base.weekly = makeRateWindow(30);

    render(
      <MenuBarMetricSection
        provider={base}
        providerMetrics={{}}
        disabled={false}
        t={(key) => key}
        onChange={vi.fn()}
      />,
    );

    expect(screen.getByRole("option", { name: "Automatic" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "ProviderSessionLabel" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "ProviderWeeklyLabel" })).toBeInTheDocument();
  });

});
