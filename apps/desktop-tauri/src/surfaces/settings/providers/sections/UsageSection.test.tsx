import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { LocaleProvider } from "../../../../i18n/LocaleProvider";
import { buildBundle } from "../../../../test/localeHarness";
import type { ProviderDetail } from "../../../../types/bridge";
import { UsageSection } from "./UsageSection";

const tauriMocks = vi.hoisted(() => ({
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
}));

vi.mock("../../../../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../lib/tauri")>()),
  ...tauriMocks,
}));
vi.mock("@tauri-apps/api/event", () => eventMocks);

function rateWindow(usedPercent: number) {
  return {
    usedPercent,
    remainingPercent: 100 - usedPercent,
    windowMinutes: null,
    resetsAt: null,
    resetDescription: null,
    isExhausted: false,
    reservePercent: null,
    reserveDescription: null,
  };
}

function provider(): ProviderDetail {
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
    session: rateWindow(20),
    weekly: null,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [
      { id: "additional_budget", title: "Additional Budget", window: rateWindow(42) },
    ],
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

describe("UsageSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({
        InventoryAvailableCount: "{} available",
        PanelLeftSuffix: "left",
        PanelUsedSuffix: "used",
        UsageBarQuotaWarnings: "Quota warnings",
        ProviderLabelLimitResetCredits: "Limit reset credits",
        ProviderTextTokens: "{} tokens",
      }),
    );
    eventMocks.listen.mockResolvedValue(() => {});
  });

  it("renders extra Copilot budget windows in settings", async () => {
    render(
      <LocaleProvider>
        <UsageSection provider={provider()} resetTimeRelative={true} t={(key) => key} />
      </LocaleProvider>,
    );

    expect(await screen.findByText("Additional Budget")).toBeInTheDocument();
    // Show usage as used is off by default, so rows read "% left" like the card.
    expect(screen.getByText("58% left")).toBeInTheDocument();
  });

  it("omits card-hidden windows, as the tray card does", async () => {
    const detail = provider();
    detail.session = { ...rateWindow(0), hiddenOnCard: true };
    detail.primaryLabel = "Spend";

    render(
      <LocaleProvider>
        <UsageSection provider={detail} resetTimeRelative={true} t={(key) => key} />
      </LocaleProvider>,
    );

    expect(await screen.findByText("Additional Budget")).toBeInTheDocument();
    expect(screen.queryByText("Spend")).not.toBeInTheDocument();
    expect(screen.queryByText("100% left")).not.toBeInTheDocument();
  });

  it.each([false, true])(
    "renders detail-backed amounts as a detail line, not reset text (resetsAt: %s)",
    async (hasReset) => {
      const detail = provider();
      const resetsAt = hasReset
        ? new Date(Date.now() + 3 * 60 * 60 * 1000 + 30_000).toISOString()
        : null;
      detail.session = {
        ...rateWindow(75),
        resetsAt,
        resetDescription: "19.17 EUR / 25.50 EUR · 6.33 EUR remaining",
        descriptionIsDetail: true,
      };
      detail.extraRateWindows = [
        {
          id: "mistral-monthly-plan",
          title: "Monthly Plan",
          window: {
            ...rateWindow(13),
            resetsAt,
            resetDescription: "34.07 EUR / 255.00 EUR · 220.93 EUR remaining",
            descriptionIsDetail: true,
          },
        },
      ];

      const { container } = render(
        <LocaleProvider>
          <UsageSection provider={detail} resetTimeRelative={true} t={(key) => key} />
        </LocaleProvider>,
      );

      const detailLines = await screen.findAllByText(/EUR remaining/);
      expect(detailLines.map((line) => line.textContent)).toEqual([
        "19.17 EUR / 25.50 EUR · 6.33 EUR remaining",
        "34.07 EUR / 255.00 EUR · 220.93 EUR remaining",
      ]);
      detailLines.forEach((line) => expect(line).toHaveClass("menu-metric__detail"));
      expect(screen.queryByText(/Resets .*EUR/)).not.toBeInTheDocument();
      expect(container.querySelectorAll(".menu-metric__reset")).toHaveLength(
        hasReset ? 2 : 0,
      );
    },
  );

  it("filters only hidden metric and extra rows", async () => {
    const detail = provider();
    detail.weekly = rateWindow(30);
    detail.hiddenUsageItemIds = [
      "metric:primary",
      "metric:extra-additional_budget",
    ];

    render(
      <LocaleProvider>
        <UsageSection provider={detail} resetTimeRelative={true} t={(key) => key} />
      </LocaleProvider>,
    );

    await screen.findByText("ProviderUsage");
    expect(screen.queryByText("ProviderSessionLabel")).not.toBeInTheDocument();
    expect(screen.queryByText("Additional Budget")).not.toBeInTheDocument();
  });

  it("marks an unavailable session without rendering a quota bar", async () => {
    const detail = provider();
    detail.session = {
      ...rateWindow(0),
      isInformational: true,
      resetDescription: "No active 5h session",
    };

    render(
      <LocaleProvider>
        <UsageSection provider={detail} resetTimeRelative={true} t={(key) => key} />
      </LocaleProvider>,
    );

    const label = await screen.findByText("ProviderSessionLabel");
    const row = label.closest(".menu-metric");
    expect(row).toHaveTextContent("ProviderTextNoActiveSession");
    expect(row?.querySelector(".menu-metric__progress")).toBeNull();
  });

  function kimiDetailBlockedUntil(blockResetsAt: string) {
    const detail = provider();
    detail.id = "kimi";
    detail.displayName = "Kimi";
    detail.session = {
      ...rateWindow(0),
      resetsAt: new Date(Date.now() + 3 * 60 * 60 * 1000).toISOString(),
      monthlyLimitBlock: { resetsAt: blockResetsAt },
    };
    detail.extraRateWindows = [
      {
        id: "kimi-monthly",
        title: "Total usage",
        window: { ...rateWindow(100), isExhausted: true, resetsAt: blockResetsAt },
      },
    ];
    return detail;
  }

  it("shows a window blocked by the monthly pool as title and status only", async () => {
    const poolReset = new Date(Date.now() + 20 * 24 * 60 * 60 * 1000).toISOString();

    render(
      <LocaleProvider>
        <UsageSection
          provider={kimiDetailBlockedUntil(poolReset)}
          resetTimeRelative={true}
          t={(key) => key}
        />
      </LocaleProvider>,
    );

    const label = await screen.findByText("ProviderSessionLabel");
    const row = label.closest(".menu-metric");
    expect(row).toHaveTextContent(/^ProviderSessionLabelPanelBlockedByMonthlyLimit$/);
    expect(row?.querySelector(".menu-metric__progress")).toBeNull();
    expect(row?.querySelector(".menu-metric__reset")).toBeNull();
    // The pool's own row keeps its bar, exhausted state and reset.
    const pool = screen.getByText("Total usage").closest(".menu-metric");
    expect(pool?.querySelector(".menu-metric__progress")).not.toBeNull();
    expect(pool).toHaveTextContent("DetailWindowExhausted");
    expect(pool?.querySelector(".menu-metric__reset")).not.toBeNull();
  });

  it("shows the raw window again once the monthly pool reset has passed", async () => {
    const past = new Date(Date.now() - 60 * 1000).toISOString();

    render(
      <LocaleProvider>
        <UsageSection
          provider={kimiDetailBlockedUntil(past)}
          resetTimeRelative={true}
          t={(key) => key}
        />
      </LocaleProvider>,
    );

    const label = await screen.findByText("ProviderSessionLabel");
    const row = label.closest(".menu-metric");
    expect(row).toHaveTextContent("100% left");
    expect(row).not.toHaveTextContent("PanelBlockedByMonthlyLimit");
    expect(row?.querySelector(".menu-metric__progress")).not.toBeNull();
  });

  it("renders discrete inventory without turning it into a quota bar", async () => {
    const detail = provider();
    detail.session = null;
    detail.extraRateWindows = [];
    detail.inventory = [
      {
        id: "reset-credits",
        title: "Limit Reset Credits",
        availableCount: 2,
        nextExpiresAt: "2099-01-01T00:00:00Z",
      },
    ];

    const { container } = render(
      <LocaleProvider>
        <UsageSection provider={detail} resetTimeRelative={true} t={(key) => key} />
      </LocaleProvider>,
    );

    expect(await screen.findByText(/Limit Reset Credits: 2 available/)).toBeInTheDocument();
    expect(container.querySelector(".menu-metric__progress")).toBeNull();
  });

  it("shows provider-declared lane labels in settings bars", async () => {
    const detail = provider();
    detail.weekly = rateWindow(30);
    detail.primaryLabel = "Fuel Pack";
    detail.secondaryLabel = "Gemini Pro";

    render(
      <LocaleProvider>
        <UsageSection provider={detail} resetTimeRelative={true} t={(key) => key} />
      </LocaleProvider>,
    );

    expect(await screen.findByText("Fuel Pack")).toBeInTheDocument();
    expect(screen.getByText("Gemini Pro")).toBeInTheDocument();
    expect(screen.queryByText("ProviderSessionLabel")).not.toBeInTheDocument();
    expect(screen.queryByText("ProviderWeeklyLabel")).not.toBeInTheDocument();
  });

  it("keeps the generic labels when the provider declares none", async () => {
    const detail = provider();
    detail.weekly = rateWindow(30);

    render(
      <LocaleProvider>
        <UsageSection provider={detail} resetTimeRelative={true} t={(key) => key} />
      </LocaleProvider>,
    );

    expect(await screen.findByText("ProviderSessionLabel")).toBeInTheDocument();
    expect(screen.getByText("ProviderWeeklyLabel")).toBeInTheDocument();
  });

  it("prints one heading per display detail section", async () => {
    const detail = provider();
    detail.session = null;
    detail.extraRateWindows = [];
    detail.displayDetails = [
      {
        id: "plain",
        sectionTitle: null,
        title: "Plain",
        value: "1",
        secondaryValue: null,
        progress: null,
      },
      {
        id: "a",
        sectionTitle: "Model activity",
        title: "fixture-alpha",
        value: "60 tokens",
        secondaryValue: null,
        progress: null,
      },
      {
        id: "b",
        sectionTitle: "Model activity",
        title: "fixture-beta",
        value: "30 tokens",
        secondaryValue: null,
        progress: null,
      },
    ];

    render(
      <LocaleProvider>
        <UsageSection provider={detail} resetTimeRelative={true} t={(key) => key} />
      </LocaleProvider>,
    );

    expect(await screen.findByText(/fixture-alpha: 60 tokens/)).toBeInTheDocument();
    expect(screen.getAllByRole("heading", { name: "Model activity" })).toHaveLength(1);
    expect(screen.getByText(/Plain: 1/)).toBeInTheDocument();
  });

  it("draws Custom quota-warning markers at % left and follows show-as-used", async () => {
    const detail = provider();
    detail.id = "codex";
    detail.session = rateWindow(42);
    detail.weekly = rateWindow(10);
    detail.extraRateWindows = [];
    const thresholds = {
      highUsageThreshold: 70,
      criticalUsageThreshold: 90,
      providerQuotaWarnings: {
        codex: { session: { high: 30, critical: 50 }, weekly: { enabled: false } },
      },
    };
    const markerX = (row: Element | null) =>
      Array.from(row?.querySelectorAll(".menu-metric__progress-warning") ?? []).map((marker) =>
        marker.getAttribute("x"),
      );

    const { container, rerender } = render(
      <LocaleProvider>
        <UsageSection
          provider={detail}
          resetTimeRelative={true}
          display={{ usageThresholds: thresholds }}
          accentColor="#10a37f"
          t={(key) => key}
        />
      </LocaleProvider>,
    );

    const session = (await screen.findByText("ProviderSessionLabel")).closest(".menu-metric");
    expect(session).toHaveTextContent("58% left");
    // Custom 30/50 % used sits at 70% and 50% of a "% left" bar.
    expect(markerX(session)).toEqual(["50%", "70%"]);
    // Weekly is Off for Codex: no markers.
    const weekly = screen.getByText("ProviderWeeklyLabel").closest(".menu-metric");
    expect(markerX(weekly)).toEqual([]);
    const section = container.querySelector<HTMLElement>(".provider-usage-main");
    expect(section?.style.getPropertyValue("--provider-accent")).toBe("#10a37f");

    rerender(
      <LocaleProvider>
        <UsageSection
          provider={detail}
          resetTimeRelative={true}
          display={{ usageThresholds: thresholds, showAsUsed: true }}
          t={(key) => key}
        />
      </LocaleProvider>,
    );

    const usedSession = screen.getByText("ProviderSessionLabel").closest(".menu-metric");
    expect(usedSession).toHaveTextContent("42% used");
    expect(markerX(usedSession)).toEqual(["30%", "50%"]);
  });

  it("puts the window pace on the meta line under its bar", async () => {
    const detail = provider();
    detail.extraRateWindows = [];
    detail.session = null;
    detail.weekly = {
      ...rateWindow(20),
      windowMinutes: 7 * 24 * 60,
      pace: {
        stage: "far_behind",
        deltaPercent: -14,
        expectedUsedPercent: 34,
        actualUsedPercent: 20,
        etaSeconds: null,
        willLastToReset: true,
      },
    };

    render(
      <LocaleProvider>
        <UsageSection provider={detail} resetTimeRelative={true} t={(key) => key} />
      </LocaleProvider>,
    );

    const weekly = (await screen.findByText("ProviderWeeklyLabel")).closest(".menu-metric");
    expect(weekly?.querySelector(".menu-metric__meta")).not.toBeNull();
    expect(weekly?.querySelector(".menu-metric__progress-pace")).not.toBeNull();
  });
});
