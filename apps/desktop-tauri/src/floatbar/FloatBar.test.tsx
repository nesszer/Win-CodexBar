import { waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { tauriMocks } from "../test/floatBarMocks";
import { snapshot, settings, bootstrap, renderFloatBar, setupFloatBarTests } from "../test/floatBarHarness";

vi.mock("../lib/tauri", async () => (await import("../test/floatBarMocks")).tauriMocks);
vi.mock("@tauri-apps/api/event", async () => (await import("../test/floatBarMocks")).eventMocks);
vi.mock("@tauri-apps/api/window", async () => (await import("../test/floatBarMocks")).windowMocks);
vi.mock("@tauri-apps/api/core", async () => (await import("../test/floatBarMocks")).coreMocks);

describe("FloatBar", () => {
  setupFloatBarTests();

  it("renders a pill per enabled provider, sorted by usage descending", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 20),
      snapshot("codex", "Codex", 75),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ floatBarShowCost: true }),
    );

    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => {
      const pills = container.querySelectorAll(".floatbar__pill");
      expect(pills.length).toBe(2);
    });

    const titles = Array.from(container.querySelectorAll(".floatbar__pill")).map(
      (el) => el.getAttribute("title") ?? "",
    );
    // Highest used (codex, 75%) shows first; display follows showAsUsed.
    expect(titles[0]).toMatch(/Codex: 75% used/);
    expect(titles[1]).toMatch(/Claude: 20% used/);
  });

  it("uses the selected session window when a weekly window is available", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 20, { secondary: { used: 90 } }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ providerMetrics: { claude: "session" } }),
    );

    const { container } = renderFloatBar(
      bootstrap({ providerMetrics: { claude: "session" } }),
    );
    await waitFor(() => {
      expect(container.querySelector(".floatbar__pill")?.getAttribute("title")).toContain(
        "Claude: 20% used",
      );
    });
  });

  it("uses the selected weekly window in the floating bar", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("codex", "Codex", 0, {
        informational: true,
        secondary: { used: 37 },
        selected: { used: 37 },
      }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ providerMetrics: { codex: "weekly" } }),
    );

    const { container } = renderFloatBar(
      bootstrap({ providerMetrics: { codex: "weekly" } }),
    );
    await waitFor(() => {
      expect(container.querySelector(".floatbar__pill")?.getAttribute("title")).toContain(
        "Codex: 37% used",
      );
    });
  });

  it("uses a real secondary window when the primary window is informational", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 10, {
        informational: true,
        secondary: {
          used: 80,
          resetsAt: null,
          resetDescription: "Resets in 2 hours",
        },
      }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ floatBarShowResetInline: true }),
    );

    const { container } = renderFloatBar(bootstrap({ floatBarShowResetInline: true }));
    await waitFor(() => {
      const pill = container.querySelector(".floatbar__pill");
      expect(pill?.getAttribute("title")).toContain("Claude: 80% used\nResets in 2h");
      expect(pill?.classList.contains("floatbar__pill--warn")).toBe(true);
      expect(container.querySelector(".floatbar__reset")?.textContent).toContain("2h");
    });
  });

  it("never shows a detail-backed description as reset text", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 13, {
        resetDescription: "34.07 EUR / 255.00 EUR · 220.93 EUR remaining",
        descriptionIsDetail: true,
      }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ floatBarShowResetInline: true }),
    );

    const { container } = renderFloatBar(bootstrap({ floatBarShowResetInline: true }));
    await waitFor(() => {
      const pill = container.querySelector(".floatbar__pill");
      expect(pill?.getAttribute("title")).toBe(
        "Claude: 13% used\n34.07 EUR / 255.00 EUR · 220.93 EUR remaining",
      );
      expect(container.querySelector(".floatbar__reset")).toBeNull();
    });
  });

  it("keeps an informational primary window when no secondary window is available", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 0, {
        informational: true,
        resetDescription: "No active 5h session",
      }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());

    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => {
      expect(container.querySelector(".floatbar__pill")?.getAttribute("title")).toBe(
        "Claude: No active 5h session",
      );
    });
  });

  it("keeps an informational primary window when the secondary window is informational", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 0, {
        informational: true,
        resetDescription: "No active 5h session",
        secondary: { used: 90, informational: true, resetDescription: "Weekly unavailable" },
      }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());

    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => {
      expect(container.querySelector(".floatbar__pill")?.getAttribute("title")).toBe(
        "Claude: No active 5h session",
      );
    });
  });

  it("shows an informational metric's text instead of a percentage or reset wording", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("litellm", "LiteLLM", 0, {
        informational: true,
        resetDescription: "No budget set",
      }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ enabledProviders: ["litellm"], floatBarShowResetInline: true }),
    );

    const { container } = renderFloatBar(
      bootstrap({ enabledProviders: ["litellm"], floatBarShowResetInline: true }),
    );
    await waitFor(() => {
      expect(container.querySelector(".floatbar__pill")).not.toBeNull();
    });
    const pill = container.querySelector(".floatbar__pill");
    expect(pill?.getAttribute("title")).toBe("LiteLLM: No budget set");
    expect(pill?.querySelector(".floatbar__pct")?.textContent).toBe("No budget set");
    expect(pill?.textContent).not.toMatch(/%|Resets/);
    expect(pill?.querySelector(".floatbar__reset")).toBeNull();
    expect(pill?.classList.contains("floatbar__pill--ok")).toBe(true);
  });

  it("keeps the neutral tone for an informational metric in remaining mode", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("cursor", "Cursor", 100, {
        informational: true,
        resetDescription: "$12.40 API-rate",
      }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ enabledProviders: ["cursor"], showAsUsed: false }),
    );

    const { container } = renderFloatBar(
      bootstrap({ enabledProviders: ["cursor"], showAsUsed: false }),
    );
    await waitFor(() => {
      expect(container.querySelector(".floatbar__pill")).not.toBeNull();
    });
    const pill = container.querySelector(".floatbar__pill");
    expect(pill?.getAttribute("title")).toBe("Cursor: $12.40 API-rate");
    expect(pill?.classList.contains("floatbar__pill--ok")).toBe(true);
  });

  it("uses an em dash for an informational metric without text and keeps a timestamp reset", async () => {
    const resetsAt = new Date(Date.now() + (2 * 60 + 5) * 60_000).toISOString();
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("codex", "Codex", 0, { informational: true, resetsAt }),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());

    const { container } = renderFloatBar(bootstrap());
    await waitFor(() => {
      expect(container.querySelector(".floatbar__pill")?.getAttribute("title")).toMatch(
        /^Codex: —\nResets in 2h \d+m$/,
      );
    });
    expect(container.querySelector(".floatbar__pct")?.textContent).toBe("—");
  });

  it("sorts providers by their selected rate window", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("claude", "Claude", 90, {
        secondary: { used: 20 },
        selected: { used: 20 },
      }),
      snapshot("codex", "Codex", 50),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ providerMetrics: { claude: "weekly" } }),
    );

    const { container } = renderFloatBar(
      bootstrap({ providerMetrics: { claude: "weekly" } }),
    );
    await waitFor(() => {
      const titles = Array.from(container.querySelectorAll(".floatbar__pill")).map(
        (pill) => pill.getAttribute("title"),
      );
      expect(titles).toEqual(["Codex: 50% used", "Claude: 20% used"]);
    });
  });

  it("loads local cost summaries without using the foreground chart endpoint", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("codex", "Codex", 75),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());
    tauriMocks.getProviderLocalUsageSummary.mockResolvedValue({
      todayCost: 1.25,
      thirtyDayCost: 12.5,
      thirtyDayTokens: 1000,
      periodCost: 12.5,
      periodTokens: 1000,
      reportingPeriod: "rolling:30",
      latestTokens: 200,
      topModel: "gpt-5",
      estimateNote: "Estimated from local logs",
      tokenCostUpdatedAtMs: 1234,
    });

    renderFloatBar(bootstrap({ floatBarShowCost: true }));

    await waitFor(() => {
      expect(tauriMocks.getProviderLocalUsageSummary).toHaveBeenCalledWith("codex");
    });
    expect(tauriMocks.getProviderChartData).not.toHaveBeenCalled();
  });

  it("marks displayed local cost as an estimate", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([snapshot("codex", "Codex", 75)]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings({ floatBarShowCost: true }));
    tauriMocks.getProviderLocalUsageSummary.mockResolvedValue({
      todayCost: 1.25,
      thirtyDayCost: 12.5,
      thirtyDayTokens: 1000,
      periodCost: 12.5,
      periodTokens: 1000,
      reportingPeriod: "rolling:30",
      latestTokens: 200,
      topModel: "gpt-5",
      estimateNote: "Estimated from local logs",
      tokenCostUpdatedAtMs: 1234,
    });

    const { container } = renderFloatBar(bootstrap({ floatBarShowCost: true }));

    await waitFor(() => {
      expect(container.querySelector(".floatbar__cost-estimate")?.textContent).toBe(
        "Estimate",
      );
    });
    expect(container.querySelector(".floatbar__cost-pill")?.getAttribute("title")).toContain(
      "(Estimate)",
    );
  });

  it("shows the selected History window cost with its short label", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([snapshot("codex", "Codex", 75)]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(
      settings({ floatBarShowCost: true, costReportingPeriod: "month-to-date" }),
    );
    tauriMocks.getProviderLocalUsageSummary.mockResolvedValue({
      todayCost: 1.25,
      // Fixed 30-day compat field stays different from the selected window.
      thirtyDayCost: 12.5,
      thirtyDayTokens: 1000,
      periodCost: 4.75,
      periodTokens: 300,
      reportingPeriod: "month-to-date",
      latestTokens: 200,
      topModel: "gpt-5",
      estimateNote: "Estimated from local logs",
      tokenCostUpdatedAtMs: 1234,
    });

    const { container } = renderFloatBar(
      bootstrap({ floatBarShowCost: true, costReportingPeriod: "month-to-date" }),
    );

    await waitFor(() => {
      const items = Array.from(container.querySelectorAll(".floatbar__cost-item")).map(
        (item) => item.textContent,
      );
      expect(items).toEqual(["Today$1.25", "MTD$4.75"]);
    });
    expect(container.textContent).not.toContain("$12.50");
  });

  it("does not scan local costs by default", async () => {
    tauriMocks.getCachedProviders.mockResolvedValue([
      snapshot("codex", "Codex", 75),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());

    renderFloatBar(bootstrap());

    await waitFor(() => {
      expect(tauriMocks.getCachedProviders).toHaveBeenCalled();
    });
    expect(tauriMocks.getProviderLocalUsageSummary).not.toHaveBeenCalled();
    expect(document.querySelector(".floatbar__cost-pill")).toBeNull();
  });
});
