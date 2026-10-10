import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { LocaleProvider } from "../i18n/LocaleProvider";
import { buildBundle } from "../test/localeHarness";
import MenuCard from "./MenuCard";
import { tauriMocks } from "../test/menuCardMocks";
import { rateWindow, provider, renderCard, setupMenuCardTests } from "../test/menuCardHarness";

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  ...(await import("../test/menuCardMocks")).tauriMocks,
}));
vi.mock("@tauri-apps/api/event", async () => (await import("../test/menuCardMocks")).eventMocks);

describe("MenuCard", () => {
  setupMenuCardTests();

  it("keeps Codex sign-in discoverable beside an authentication error with no accounts", async () => {
    tauriMocks.getCodexAccountsState.mockResolvedValueOnce({ accounts: [], accountOrdinals: {}, snapshots: {} });
    const codex = { ...provider("Authentication required"), providerId: "codex", displayName: "Codex", errorState: "needsAuthentication" as const };
    render(<LocaleProvider><MenuCard provider={codex} display={{ hideEmail: false, resetTimeRelative: true, showResetWhenExhausted: true }} /></LocaleProvider>);
    expect(await screen.findByText("Authentication required")).toBeDefined();
    const add = await screen.findByRole("button", { name: "CodexAccountsSignInButton" });
    await waitFor(() => expect(add).toBeEnabled());
  });

  it("keeps Fireworks vendor API spend visible when local cost summaries are hidden", async () => {
    const snapshot = provider(null, 0);
    snapshot.providerId = "fireworks";
    snapshot.displayName = "Fireworks";
    snapshot.cost = {
      used: 12.34,
      limit: null,
      remaining: null,
      currencyCode: "USD",
      currencySymbol: "$",
      period: "30 days",
      resetsAt: null,
      formattedUsed: "$12.34",
      formattedLimit: null,
      balance: null,
      formattedBalance: null,
      daily: [],
      alwaysVisible: true,
    };

    renderCard(snapshot, { costSummaryDisplayStyle: "hidden" });

    expect(await screen.findByText("API spend")).toBeInTheDocument();
    expect(document.querySelector(".menu-card__cost-line")).toHaveTextContent("$12.34");
  });

  it("filters hidden metric and extra rows without changing the snapshot payload", async () => {
    const snapshot = provider(null, 25);
    snapshot.secondary = rateWindow(60);
    snapshot.secondaryLabel = "Weekly";
    snapshot.extraRateWindows = [
      { id: "credits", title: "Credits", window: rateWindow(10) },
    ];
    snapshot.hiddenUsageItemIds = ["metric:primary", "metric:extra-credits"];

    renderCard(snapshot);

    await screen.findByText("ProviderWeeklyLabel");
    expect(screen.queryByText("Session")).not.toBeInTheDocument();
    expect(screen.queryByText("Credits")).not.toBeInTheDocument();
  });
  it("does not mix stale local usage into an error card", async () => {
    const { container } = renderCard(
      provider("OAuth error: Claude OAuth credentials not found."),
    );

    expect(
      await screen.findByText("OAuth error: Claude OAuth credentials not found."),
    ).toBeInTheDocument();
    expect(container.querySelector(".menu-card--header-only")).toBeInTheDocument();
    expect(container.querySelector(".menu-card--with-details")).not.toBeInTheDocument();

    await waitFor(() => {
      expect(tauriMocks.getProviderChartData).toHaveBeenCalled();
    });

    expect(screen.queryByText("30d cost")).not.toBeInTheDocument();
    expect(screen.queryByText("30d tokens")).not.toBeInTheDocument();
    expect(screen.queryByText("Estimated from local logs")).not.toBeInTheDocument();
  });

  it("shows DeepSeek peak/off-peak pricing status", async () => {
    tauriMocks.getDeepSeekPricingStatus.mockResolvedValue({
      period: "offPeak",
      currentLocalTime: "2026-08-17 05:00:00 UTC",
      nextTransitionLocalTime: "2026-08-17 06:00:00 UTC",
      effectiveLocalTime: "2026-08-16 18:00:00 EDT",
    });
    const snapshot = provider(null);
    snapshot.providerId = "deepseek";
    snapshot.displayName = "DeepSeek";

    renderCard(snapshot);

    expect(await screen.findByText("DeepSeek pricing: Off-peak hours")).toBeInTheDocument();
    expect(screen.getByText("Current local time: 2026-08-17 05:00:00 UTC")).toBeInTheDocument();
    expect(screen.getByText("Next transition: 2026-08-17 06:00:00 UTC")).toBeInTheDocument();
  });

  it("can render metric bars as used instead of remaining", async () => {
    renderCard(provider(null, 35), { showAsUsed: true });

    expect(await screen.findByText("35% used")).toBeInTheDocument();
    expect(screen.queryByText("65% left")).not.toBeInTheDocument();

    const fill = document.querySelector<HTMLElement>(".menu-metric__bar-fill");
    expect(fill?.style.width).toBe("35%");
  });

  it("shows an additional balance description below the meter and reset time", async () => {
    const snapshot = provider(null, 25, {
      resetDescription: "750 / 1000 credits left",
    });
    snapshot.primaryLabel = "Credits";
    snapshot.primary.descriptionIsDetail = true;
    snapshot.selectedMetric.descriptionIsDetail = true;
    const resetsAt = new Date(Date.now() + 60 * 60 * 1000).toISOString();
    snapshot.primary.resetsAt = resetsAt;
    snapshot.selectedMetric.resetsAt = resetsAt;

    renderCard(snapshot);

    const description = await screen.findByText("750 / 1000 credits left");
    expect(description).toHaveClass("menu-metric__detail");
    expect(screen.getByText(/Resets in/)).toBeInTheDocument();
  });

  it("does not repeat a reset-phrase description as an extra detail", async () => {
    const snapshot = provider(null, 20, { resetDescription: "Resets in 3h" });
    const resetsAt = new Date(Date.now() + 5 * 60 * 60 * 1000).toISOString();
    snapshot.primary.resetsAt = resetsAt;
    snapshot.selectedMetric.resetsAt = resetsAt;

    renderCard(snapshot);

    await screen.findByText(/Resets in/);
    expect(document.querySelectorAll(".menu-metric__reset")).toHaveLength(1);
    expect(document.querySelectorAll(".menu-metric__detail")).toHaveLength(0);
    expect(screen.queryByText("Resets in 3h")).not.toBeInTheDocument();
  });

  it("displays over-quota usage without overflowing the bar", async () => {
    renderCard(provider(null, 115, { exhausted: true, resetDescription: "115% used" }), {
      showAsUsed: true,
    });

    expect(await screen.findAllByText("115% used")).not.toHaveLength(0);
    const fill = document.querySelector<HTMLElement>(".menu-metric__bar-fill");
    expect(fill?.style.width).toBe("100%");
  });

  it("replaces an exhausted percentage with a future reset countdown", async () => {
    const snapshot = provider(null, 100, { exhausted: true });
    snapshot.primary.resetsAt = new Date(Date.now() + 60 * 60 * 1000).toISOString();

    renderCard(snapshot, { showResetWhenExhausted: true });

    expect(await screen.findByText(/Resets in \d+m/)).toBeInTheDocument();
    expect(screen.queryByText("0% left")).not.toBeInTheDocument();
  });

  it("keeps an exhausted percentage without a concrete future reset", async () => {
    renderCard(provider(null, 100, { exhausted: true, resetDescription: "in 2h" }), {
      showResetWhenExhausted: true,
    });

    expect(await screen.findByText("0% left")).toBeInTheDocument();
  });

  it("renders additional Copilot budget windows", async () => {
    const snapshot = provider(null, 20);
    snapshot.providerId = "copilot";
    snapshot.displayName = "GitHub Copilot";
    snapshot.extraRateWindows = [
      {
        id: "additional_budget",
        title: "Additional Budget",
        window: rateWindow(42),
      },
    ];

    renderCard(snapshot);

    expect(await screen.findByText("Additional Budget")).toBeInTheDocument();
    expect(screen.getByText("58% left")).toBeInTheDocument();
  });

  it("shows every quota row in compact Overview (upstream 0.62.0 #2616)", async () => {
    const snapshot = provider(null, 20, { resetDescription: "Resets in 2h" });
    snapshot.secondary = rateWindow(42, { windowMinutes: 7 * 24 * 60 });
    snapshot.secondaryLabel = "Weekly";
    snapshot.tertiary = rateWindow(63, { windowMinutes: 30 * 24 * 60 });
    snapshot.tertiaryLabel = "Monthly";

    renderCard(snapshot, { compactOverview: true });

    expect(await screen.findByText("ProviderSessionLabel")).toBeInTheDocument();
    expect(screen.getByText("ProviderWeeklyLabel")).toBeInTheDocument();
    expect(screen.getByText("ProviderMonthly")).toBeInTheDocument();
    expect(document.querySelectorAll(".menu-metric")).toHaveLength(3);
  });

  it("shows both Agent Plan lanes next to the informational placeholder in compact Overview", async () => {
    const snapshot = provider(null, 0);
    snapshot.primary = { ...rateWindow(0), isInformational: true, resetDescription: "No active 5h session" };
    snapshot.extraRateWindows = [
      { id: "doubao-agent-session", title: "5-hour", window: rateWindow(42, { windowMinutes: 300 }) },
      { id: "doubao-agent-weekly", title: "Weekly", window: rateWindow(67, { windowMinutes: 10080 }) },
    ];

    renderCard(snapshot, { compactOverview: true });

    // Compact Overview shows every quota row (upstream 0.62.0 #2616), so the
    // placeholder no longer competes with the measured lanes for a row.
    expect(await screen.findByText("58% left")).toBeInTheDocument();
    expect(screen.getByText("33% left")).toBeInTheDocument();
    expect(screen.getByText("No active 5h session")).toBeInTheDocument();
    expect(document.querySelectorAll(".menu-metric")).toHaveLength(3);
  });

  it("localizes Claude scoped weekly extra-window labels", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle({ ClaudeScopedWeeklyLabel: "{} weekly", ProviderLabelModelOnly: "{} only" }));
    const snapshot = provider(null, 20);
    snapshot.extraRateWindows = [
      {
        id: "claude-weekly-scoped-fable",
        title: "Fable only",
        window: rateWindow(42, { windowMinutes: 7 * 24 * 60 }),
      },
      {
        id: "custom",
        title: "Custom only",
        window: rateWindow(10),
      },
    ];

    renderCard(snapshot);

    expect(await screen.findByText("Fable weekly")).toBeInTheDocument();
    expect(screen.getByText("Custom only")).toBeInTheDocument();
    expect(screen.queryByText("Fable only")).not.toBeInTheDocument();

    const otherProvider = provider(null, 20);
    otherProvider.providerId = "synthetic";
    otherProvider.extraRateWindows = [{ ...snapshot.extraRateWindows[0], id: "synthetic-weekly" }];
    renderCard(otherProvider);
    expect(await screen.findByText("Fable only")).toBeInTheDocument();
  });

  it.each([false, true])(
    "shows detail-backed amounts as their own line, never as reset text (resetsAt: %s)",
    async (hasReset) => {
      const resetsAt = hasReset
        ? new Date(Date.now() + 3 * 60 * 60 * 1000 + 30_000).toISOString()
        : null;
      const snapshot = provider(null, 75, {
        resetDescription: "19.17 EUR / 25.50 EUR · 6.33 EUR remaining",
        resetsAt,
      });
      snapshot.providerId = "mistral";
      snapshot.primary.descriptionIsDetail = true;
      snapshot.selectedMetric.descriptionIsDetail = true;
      snapshot.extraRateWindows = [
        {
          id: "mistral-monthly-plan",
          title: "Monthly Plan",
          window: {
            ...rateWindow(13, {
              resetDescription: "34.07 EUR / 255.00 EUR · 220.93 EUR remaining",
              resetsAt,
            }),
            descriptionIsDetail: true,
          },
        },
      ];

      renderCard(snapshot);

      const primaryDetail = await screen.findByText(
        "19.17 EUR / 25.50 EUR · 6.33 EUR remaining",
      );
      const planDetail = screen.getByText("34.07 EUR / 255.00 EUR · 220.93 EUR remaining");
      expect(primaryDetail).toHaveClass("menu-metric__detail");
      expect(planDetail).toHaveClass("menu-metric__detail");
      expect(screen.queryByText(/Resets .*EUR/)).not.toBeInTheDocument();
      expect(document.querySelectorAll(".menu-metric__reset")).toHaveLength(hasReset ? 2 : 0);
    },
  );

  it("renders informational metrics without quota percentages", async () => {
    const snapshot = provider(null, 20);
    snapshot.extraRateWindows = [
      {
        id: "requests",
        title: "Requests",
        window: {
          ...rateWindow(0),
          isInformational: true,
          resetDescription: "7 requests",
        },
      },
    ];

    renderCard(snapshot);

    const title = await screen.findByText("Requests");
    expect(title.parentElement).not.toHaveTextContent("100% left");
    expect(title.parentElement?.querySelector(".menu-metric__bar")).toBeNull();
    expect(screen.getByText("7 requests")).toBeInTheDocument();
  });

  it("shows reset credit count and next expiry without a percent bar", async () => {
    const snapshot = provider(null, 20);
    snapshot.extraRateWindows = [
      {
        id: "reset-credits",
        title: "Reset credits",
        window: {
          ...rateWindow(0, {
            resetsAt: new Date(Date.now() + 6 * 24 * 60 * 60 * 1000 + 21 * 60 * 60 * 1000).toISOString(),
            resetDescription: "2 reset credits available",
          }),
          isInformational: true,
        },
      },
    ];

    renderCard(snapshot);

    const title = await screen.findByText("Reset credits");
    expect(screen.getByText("2 reset credits available")).toBeInTheDocument();
    expect(screen.getByText(/Next expires in/)).toBeInTheDocument();
    expect(title.parentElement?.querySelector(".menu-metric__bar")).toBeNull();
  });

  it("keeps reset credit count in absolute reset-time mode", async () => {
    const snapshot = provider(null, 20);
    snapshot.extraRateWindows = [
      {
        id: "reset-credits",
        title: "Reset credits",
        window: {
          ...rateWindow(0, {
            resetsAt: new Date(Date.now() + 6 * 24 * 60 * 60 * 1000).toISOString(),
            resetDescription: "2 reset credits available",
          }),
          isInformational: true,
        },
      },
    ];

    render(
      <LocaleProvider>
        <MenuCard
          provider={snapshot}
          display={{
            hideEmail: false,
            resetTimeRelative: false,
          }}
        />
      </LocaleProvider>,
    );

    expect(await screen.findByText("2 reset credits available")).toBeInTheDocument();
    expect(screen.queryByText(/Next expires in/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Resets in/)).not.toBeInTheDocument();
  });

  it("renders Wayfinder telemetry without quota or identity rows", async () => {
    const snapshot = provider(null);
    snapshot.providerId = "wayfinder";
    snapshot.displayName = "Wayfinder";
    snapshot.accountEmail = "should-not-render@example.test";
    snapshot.planName = "should-not-render";
    snapshot.wayfinderUsage = {
      gatewayStatus: "ok",
      offline: false,
      dryRun: false,
      missingKeys: [],
      modelCount: 2,
      models: ["model-a", "model-b"],
      requests: 14,
      estimatedRequests: 0,
      tokens: 1028,
      realized: 0.004,
      baseline: 0.01,
      saved: 0.006,
      savedPercent: 60,
      periodDays: 30,
      unit: "usd",
      priced: true,
      routes: [],
    };

    renderCard(snapshot);

    expect(await screen.findByText("ok")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(screen.getByText("1K")).toBeInTheDocument();
    expect(screen.queryByText("should-not-render@example.test")).not.toBeInTheDocument();
    expect(screen.queryByText("should-not-render")).not.toBeInTheDocument();
    expect(screen.queryByText("Session")).not.toBeInTheDocument();
  });

  it("uses explicit hourly quota labels in Simplified Chinese", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle({}, "chinese"));
    const snapshot = provider(null, 31);
    snapshot.primary = rateWindow(31, { windowMinutes: 3 * 60 });
    snapshot.selectedMetric = snapshot.primary;

    renderCard(snapshot);

    expect(await screen.findByText("3 小时")).toBeInTheDocument();
    expect(screen.queryByText("Session")).not.toBeInTheDocument();
  });

  it("maps a Simplified Chinese session-labelled weekly window to Weekly", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({ ProviderWeeklyLabel: "本周" }, "chinese"),
    );
    const snapshot = provider(null, 31);
    snapshot.primary = rateWindow(31, { windowMinutes: 7 * 24 * 60 });
    snapshot.selectedMetric = snapshot.primary;

    renderCard(snapshot);

    expect(await screen.findByText("本周")).toBeInTheDocument();
  });

  it("notifies the tray panel after async local usage data loads", async () => {
    const onLayoutChange = vi.fn();

    renderCard(provider(null), { onLayoutChange });

    await waitFor(() => {
      expect(onLayoutChange).toHaveBeenCalled();
    });
  });

  it("shows the formatted predicted exhaustion time", async () => {
    const snapshot = provider(null, 40);
    snapshot.pace = {
      stage: "far_ahead",
      deltaPercent: 20,
      expectedUsedPercent: 20,
      actualUsedPercent: 40,
      etaSeconds: 90 * 60,
      willLastToReset: false,
    };

    const { container } = renderCard(snapshot);

    await waitFor(() => {
      expect(container.querySelector(".menu-card__pace-eta")).toHaveTextContent(
        "⚠ Runs out in 2h",
      );
    });
  });

  it("hides pace, budgets, and forecast text when Show pace is off", async () => {
    const resetAt = new Date(Date.now() + 6 * 24 * 60 * 60 * 1000);
    const snapshot = provider(null, 31);
    snapshot.pace = {
      stage: "far_ahead",
      deltaPercent: 20,
      expectedUsedPercent: 20,
      actualUsedPercent: 40,
      etaSeconds: 90 * 60,
      willLastToReset: false,
    };
    snapshot.primary = rateWindow(31, {
      windowMinutes: 7 * 24 * 60,
      resetsAt: resetAt.toISOString(),
    });

    const { container } = renderCard(snapshot, { showPace: false });

    expect(await screen.findByText("69% left")).toBeInTheDocument();
    expect(container.querySelector(".menu-card__pace")).not.toBeInTheDocument();
    expect(screen.queryByText("On-pace budget")).not.toBeInTheDocument();
    expect(container.querySelector(".menu-metric__forecast")).not.toBeInTheDocument();
  });

  it("hides derived pace advice for local OpenCode Go estimates", async () => {
    const resetAt = new Date(Date.now() + 3 * 24 * 60 * 60 * 1000);
    const snapshot = provider(null, 12);
    snapshot.providerId = "opencodego";
    snapshot.displayName = "OpenCode Go";
    snapshot.sourceLabel = "local estimate";
    snapshot.primary = rateWindow(12, {
      windowMinutes: 5 * 60,
      resetsAt: new Date(Date.now() + 2 * 60 * 60 * 1000).toISOString(),
    });
    snapshot.secondary = rateWindow(23, {
      windowMinutes: 7 * 24 * 60,
      resetsAt: resetAt.toISOString(),
      reservePercent: 34,
      reserveWillLastToReset: true,
    });
    snapshot.pace = {
      stage: "far_ahead",
      deltaPercent: 20,
      expectedUsedPercent: 20,
      actualUsedPercent: 40,
      etaSeconds: 90 * 60,
      willLastToReset: false,
    };

    const { container } = renderCard(snapshot);

    expect(await screen.findByText("88% left")).toBeInTheDocument();
    expect(screen.getByText("77% left")).toBeInTheDocument();
    expect(container.querySelector(".menu-card__pace")).not.toBeInTheDocument();
    expect(screen.queryByText("On-pace budget")).not.toBeInTheDocument();
    expect(screen.queryByText(/in reserve/)).not.toBeInTheDocument();
    expect(container.querySelector(".menu-metric__forecast")).not.toBeInTheDocument();
  });
});
