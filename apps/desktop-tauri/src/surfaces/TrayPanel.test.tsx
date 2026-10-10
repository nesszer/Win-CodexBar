import { fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import TrayPanel from "./TrayPanel";
import { LocaleProvider } from "../i18n/LocaleProvider";
import { TEST_PROVIDER_CATALOG } from "../test/providerCatalog";
import { buildBundle } from "../test/localeHarness";
import type { ProviderCatalogEntry } from "../types/bridge";
import { makeSettings } from "../test/fixtures";
import { tauriMocks } from "../test/trayPanelMocks";
import { provider, providerWithThreeQuotaWindows, bootstrap, renderTrayPanel, setupTrayPanelTests } from "../test/trayPanelHarness";

vi.mock("../lib/tauri", async () => (await import("../test/trayPanelMocks")).tauriMocks);
vi.mock("@tauri-apps/api/event", async () => (await import("../test/trayPanelMocks")).eventMocks);
vi.mock("@tauri-apps/api/window", async () => (await import("../test/trayPanelMocks")).windowMocks);

describe("TrayPanel provider grid", () => {
  setupTrayPanelTests();

  it("reveals regardless of the shared surface-mode snapshot (TrayPanel now runs in its own dedicated window)", async () => {
    // TrayPanel is now hosted exclusively in the dedicated `flyout` OS
    // window (see App.tsx's isFlyoutWindow() routing), so it must not depend
    // on `main`'s surface-mode machine to know it's "open" — that machine
    // can report something other than "trayPanel". Overriding the snapshot
    // mock to another mode confirms the fixed-size restore + reveal gate
    // (isFlyoutOpen, hardcoded true in TrayPanel.tsx) is no longer wired to
    // useSurfaceMode() at all.
    tauriMocks.getCurrentSurfaceState.mockResolvedValue({
      mode: "settings",
      target: { kind: "settings", tab: "general" },
    });

    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
  });

  it("shows only included overview spend rows without an export button", async () => {
    tauriMocks.getUsageSpendSummary.mockResolvedValue({
      contract: {},
      reportingPeriod: "rolling:30",
      reportingDay: "2026-09-19",
      dashboardTimezone: "UTC",
      rows: [
        {
          providerId: "codex",
          displayName: "Codex",
          sevenDay: 1,
          thirtyDay: 2,
          periodCost: 2,
          periodTokens: null,
          currency: "USD",
          source: "local",
          includedInOverview: true,
        },
        {
          providerId: "claude",
          displayName: "Claude",
          sevenDay: 3,
          thirtyDay: 4,
          periodCost: 4,
          periodTokens: null,
          currency: "USD",
          source: "hidden",
          includedInOverview: false,
        },
        // A known subtotal is a partial estimate: counted for coverage, not in the total.
        {
          providerId: "antigravity",
          displayName: "Antigravity",
          sevenDay: null,
          thirtyDay: null,
          periodCost: null,
          periodTokens: null,
          thirtyDayEstimate: {
            knownSubtotalUsd: 9,
            coverage: { priced: 0, unpriced: 1, unmetered: 0, estimated: 1 },
          },
          currency: "USD",
          source: "known subtotal",
          includedInOverview: true,
        },
      ],
    });

    renderTrayPanel([provider("codex", "Codex", 35)]);

    const expectedTotal = `~${new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: "USD",
      maximumFractionDigits: 2,
    }).format(2)}`;
    expect(await screen.findByText((_, element) =>
      element?.tagName === "STRONG" && element.textContent === expectedTotal,
    )).toBeInTheDocument();
    expect(screen.getByText(/1 of 2 providers have spend/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "UsageSpendShare" })).not.toBeInTheDocument();
  });

  it("dismisses the tray panel on unmodified Escape", async () => {
    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });

    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => {
      expect(tauriMocks.dismissTrayPanel).toHaveBeenCalledTimes(1);
    });
  });

  it("does not dismiss the tray panel on modified Escape", async () => {
    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });

    fireEvent.keyDown(window, { key: "Escape", ctrlKey: true });
    fireEvent.keyDown(window, { key: "Escape", shiftKey: true });
    fireEvent.keyDown(window, { key: "Escape", altKey: true });
    fireEvent.keyDown(window, { key: "Escape", metaKey: true });

    expect(tauriMocks.dismissTrayPanel).not.toHaveBeenCalled();
  });

  it("keeps the existing Ctrl+R tray shortcut", async () => {
    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
    tauriMocks.refreshProviders.mockClear();

    fireEvent.keyDown(window, { key: "r", ctrlKey: true });

    await waitFor(() => {
      expect(tauriMocks.refreshProviders).toHaveBeenCalledTimes(1);
    });
  });

  it("scopes the status-page action to the selected provider", async () => {
    const { container } = renderTrayPanel([
      provider("claude", "Claude", 35),
      provider("codex", "Codex", 45),
    ]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });

    fireEvent.click(screen.getByRole("button", { name: /^Claude$/ }));
    fireEvent.click(await screen.findByRole("button", { name: /^Usage Dashboard$/ }));
    expect(tauriMocks.openProviderDashboard).toHaveBeenLastCalledWith("claude");
    fireEvent.click(await screen.findByRole("button", { name: /^Status Page$/ }));
    expect(tauriMocks.openProviderStatusPage).toHaveBeenLastCalledWith("claude");

    fireEvent.click(screen.getByRole("button", { name: /^Codex$/ }));
    fireEvent.click(await screen.findByRole("button", { name: /^Usage Dashboard$/ }));
    expect(tauriMocks.openProviderDashboard).toHaveBeenLastCalledWith("codex");
    fireEvent.click(await screen.findByRole("button", { name: /^Status Page$/ }));
    expect(tauriMocks.openProviderStatusPage).toHaveBeenLastCalledWith("codex");
  });

  it("localizes static tray panel labels in Japanese", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle(
        {
          ActionRefresh: "更新",
          MenuAbout: "CodexBar について",
          MenuQuit: "終了",
          MenuSettings: "設定...",
          PanelAllProviders: "すべてのプロバイダー",
          PanelAllProvidersShort: "すべて",
          PanelLatestTokens: "最新トークン",
          CostPeriodShortDays: "{}日",
          PanelPeriodCost: "{}間のコスト",
          PanelPeriodTokens: "{}間のトークン",
          PanelTopModelPrefix: "トップモデル",
          PanelEstimatedFromLocalLogs: "ローカルログから推定",
          PanelZoom: "ズーム",
          UpdatedDaysAgo: "{}日前",
        },
        "japanese",
      ),
    );
    tauriMocks.getProviderChartData.mockResolvedValue({
      providerId: "codex",
      costHistory: [{ date: "2026-05-24", value: 1.23 }],
      creditsHistory: [],
      usageBreakdown: [],
      localUsage: {
        todayCost: null,
        thirtyDayCost: 1.23,
        thirtyDayTokens: 584_000,
        periodCost: 1.23,
        periodTokens: 584_000,
        reportingPeriod: "rolling:30",
        latestTokens: 1200,
        topModel: "gpt-5.5",
        estimateNote: "Estimated from local logs",
        tokenCostUpdatedAtMs: 1234,
      },
    });

    const { container } = renderTrayPanel([provider("codex", "Codex", 35)]);

    await waitFor(() => {
      expect(
        container.querySelector('.provider-grid__item[aria-label="すべてのプロバイダー"]'),
      ).not.toBeNull();
    });
    expect(container.querySelector(".provider-grid__item")?.textContent).toContain("すべて");
    expect(screen.getByText("ズーム")).toBeInTheDocument();
    expect(screen.getByLabelText("ズーム")).toBeInTheDocument();
    expect(screen.getByText("更新")).toBeInTheDocument();
    expect(screen.getByText("設定...")).toBeInTheDocument();
    expect(screen.getByText("CodexBar について")).toBeInTheDocument();
    expect(screen.getByText("終了")).toBeInTheDocument();
    expect(await screen.findByText("30日間のコスト")).toBeInTheDocument();
    expect(container.querySelector(".menu-card__subtitle")?.textContent).toContain("日前");
    expect(screen.getByText("最新トークン")).toBeInTheDocument();
    expect(screen.getByText("トップモデル: gpt-5.5")).toBeInTheDocument();
    expect(screen.getByText("ローカルログから推定")).toBeInTheDocument();
  });

  it("localizes the expanded dense grid collapse label in Japanese", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle(
        {
          PanelAllProviders: "すべてのプロバイダー",
          PanelAllProvidersShort: "すべて",
          PanelShowAllProviders: "すべてのプロバイダーを表示",
          PanelShowFewerProviders: "表示を減らす",
        },
        "japanese",
      ),
    );
    const providers = TEST_PROVIDER_CATALOG.map(([id, displayName], index) =>
      provider(id, displayName, (index * 7) % 100),
    );

    const { container } = renderTrayPanel(providers);

    await waitFor(() => {
      expect(container.querySelector(".provider-grid--compact")).not.toBeNull();
    });

    fireEvent.click(
      container.querySelector<HTMLButtonElement>(
        '.provider-grid__item--more[aria-label="すべてのプロバイダーを表示"]',
      )!,
    );

    expect(await screen.findByText("表示を減らす")).toBeInTheDocument();
  });

  it.each([
    [1, true],
    [2, true],
    [5, true],
    [6, false],
    [12, false],
  ])("uses expected density for %i providers plus overview", async (providerCount, shouldBeSparse) => {
      const providers = [
        provider("codex", "Codex"),
        provider("claude", "Claude"),
        provider("copilot", "GitHub Copilot"),
        provider("cursor", "Cursor"),
        provider("gemini", "Gemini"),
        provider("kiro", "Kiro"),
        provider("zai", "z.ai"),
        provider("minimax", "MiniMax"),
        provider("vertexai", "Vertex AI"),
        provider("augment", "Augment"),
        provider("opencode", "OpenCode"),
        provider("kimi", "Kimi"),
      ].slice(0, providerCount);

      const { container } = renderTrayPanel(providers);

      await waitFor(() => {
        expect(container.querySelector(".provider-grid")).not.toBeNull();
      });

      const grid = container.querySelector(".provider-grid");
      expect(grid?.classList.contains("provider-grid--sparse")).toBe(
        shouldBeSparse,
      );
    },
  );

  it("only requests chart data for providers that can render charts", async () => {
    renderTrayPanel([
      provider("codex", "Codex"),
      provider("claude", "Claude"),
      provider("copilot", "GitHub Copilot"),
      provider("cursor", "Cursor"),
      provider("deepseek", "DeepSeek"),
    ]);

    await waitFor(() => {
      expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(2);
    });

    expect(tauriMocks.getProviderChartData).toHaveBeenCalledWith("codex", undefined);
    expect(tauriMocks.getProviderChartData).toHaveBeenCalledWith("claude", undefined);
  });

  it("renders providers in settings catalog order instead of fetch completion order", async () => {
    const catalog: ProviderCatalogEntry[] = [
      { id: "codex", displayName: "Codex", cookieDomain: null },
      { id: "claude", displayName: "Claude", cookieDomain: null },
      { id: "cursor", displayName: "Cursor", cookieDomain: null },
      { id: "factory", displayName: "Factory", cookieDomain: null },
      { id: "gemini", displayName: "Gemini", cookieDomain: null },
    ];
    const providers = [
      provider("gemini", "Gemini", 10),
      provider("cursor", "Cursor", 20),
      { ...provider("codex", "Codex", 80), error: "Authentication required" },
      provider("factory", "Factory", 30),
      { ...provider("claude", "Claude", 40), error: "Claude sign-in missing" },
    ];

    const { container } = renderTrayPanel(
      providers,
      { enabledProviders: catalog.map((entry) => entry.id) },
      catalog,
    );

    await waitFor(() => {
      expect(container.querySelectorAll(".provider-grid__item")).toHaveLength(6);
    });

    const labels = Array.from(container.querySelectorAll(".provider-grid__item"))
      .map((node) => node.getAttribute("aria-label"));
    expect(labels).toEqual([
      "All providers",
      "Codex",
      "Claude",
      "Cursor",
      "Factory",
      "Gemini",
    ]);
    expect(
      Array.from(container.querySelectorAll(".menu-card__name")).map(
        (node) => node.textContent,
      ),
    ).toEqual(["Codex", "Claude", "Cursor", "Factory", "Gemini"]);
  });

  it("shows all quota rows in compact Overview when explicitly selected (0.62.0 #2616)", async () => {
    const { container } = renderTrayPanel(
      [providerWithThreeQuotaWindows("codex", "Codex")],
      { overviewLayout: "compact" },
    );

    await waitFor(() => {
      expect(container.querySelector(".menu-stack__item")).not.toBeNull();
    });

    expect(container.querySelectorAll(".menu-metric")).toHaveLength(3);
  });

  it("uses independent columns for a wide user-sized overview", async () => {
    tauriMocks.flyoutStoredSize.mockResolvedValue([700, 700]);
    const providers = [
      provider("codex", "Codex"),
      provider("claude", "Claude"),
      provider("antigravity", "Antigravity"),
      provider("copilot", "GitHub Copilot"),
    ];

    const { container } = renderTrayPanel(providers, {
      enabledProviders: providers.map((snapshot) => snapshot.providerId),
    });

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--usersized")).not.toBeNull();
    });

    expect(
      Array.from(container.querySelectorAll(".menu-stack__column")).map((column) =>
        Array.from(column.querySelectorAll(".menu-stack__item")).map(
          (item) => item.id,
        ),
      ),
    ).toEqual([
      ["card-codex", "card-antigravity"],
      ["card-claude", "card-copilot"],
    ]);
    expect(container.querySelector(".menu-stack__sep")).toBeNull();
  });

  it("keeps the stacked layout when the saved flyout width is narrow", async () => {
    vi.spyOn(window, "innerWidth", "get").mockReturnValue(700);
    tauriMocks.flyoutStoredSize.mockResolvedValue([500, 700]);
    const providers = [
      provider("codex", "Codex"),
      provider("claude", "Claude"),
      provider("antigravity", "Antigravity"),
      provider("copilot", "GitHub Copilot"),
    ];

    const { container } = renderTrayPanel(providers, {
      enabledProviders: providers.map((snapshot) => snapshot.providerId),
    });

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--usersized")).not.toBeNull();
    });

    expect(container.querySelector(".menu-stack__column")).toBeNull();
    expect(container.querySelectorAll(".menu-stack__sep")).toHaveLength(3);
  });

  it("collapses and expands the full provider catalog in the dense tray grid", async () => {
    const providers = TEST_PROVIDER_CATALOG.map(([id, displayName], index) =>
      provider(id, displayName, (index * 7) % 100),
    );

    const { container } = renderTrayPanel(providers);

    await waitFor(() => {
      expect(container.querySelectorAll(".provider-grid__item")).toHaveLength(
        20,
      );
    });

    const grid = container.querySelector(".provider-grid");
    expect(grid?.classList.contains("provider-grid--sparse")).toBe(false);
    expect(grid?.classList.contains("provider-grid--compact")).toBe(true);
    expect(grid?.getAttribute("data-expanded")).toBe("false");
    expect(grid?.getAttribute("data-provider-count")).toBe(
      String(providers.length + 1),
    );
    expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(4);

    const expand = container.querySelector<HTMLButtonElement>(
      '.provider-grid__item--more[aria-label="Show all providers"]',
    );
    expect(expand).not.toBeNull();
    expect(expand?.textContent).toContain(`+${providers.length - 18}`);

    fireEvent.click(expand!);

    await waitFor(() => {
      expect(container.querySelectorAll(".provider-grid__item")).toHaveLength(
        providers.length + 2,
      );
    });
    expect(grid?.getAttribute("data-expanded")).toBe("true");
    expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(
      providers.length,
    );
    for (const [id, displayName] of TEST_PROVIDER_CATALOG) {
      expect(
        container.querySelector(`.provider-grid__item[aria-label="${displayName}"]`),
        id,
      ).not.toBeNull();
    }
  });

  it("uses compact provider labels for huge catalogs without losing full accessible labels", async () => {
    const providers = TEST_PROVIDER_CATALOG.slice(0, 36).map(
      ([id, displayName], index) => provider(id, displayName, (index * 7) % 100),
    );

    const { container } = renderTrayPanel(providers);

    await waitFor(() => {
      expect(container.querySelector(".provider-grid--compact")).not.toBeNull();
    });

    const expand = container.querySelector<HTMLButtonElement>(
      '.provider-grid__item--more[aria-label="Show all providers"]',
    );
    expect(expand).not.toBeNull();

    fireEvent.click(expand!);

    await waitFor(() => {
      expect(
        container.querySelector('.provider-grid__item[aria-label="Copilot"]'),
      ).not.toBeNull();
    });

    const copilot = container.querySelector(
      '.provider-grid__item[aria-label="Copilot"]',
    );
    expect(copilot).not.toBeNull();
    expect(copilot?.getAttribute("aria-label")).toBe("Copilot");
    expect(copilot?.querySelector(".provider-grid__label")?.textContent).toBe(
      "Copi",
    );
  });

  it("provider grid indicator follows the show-as-used setting", async () => {
    const { container, rerender } = renderTrayPanel(
      [provider("claude", "Claude", 35)],
      { showAsUsed: true },
    );

    await waitFor(() => {
      const track = container.querySelector<HTMLElement>(
        ".provider-grid__weekly-track",
      );
      expect(track?.style.getPropertyValue("--weekly-pct")).toBe("35%");
    });

    tauriMocks.getCachedProviders.mockResolvedValue([
      provider("claude", "Claude", 35),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(makeSettings({ showAsUsed: false }));
    rerender(
      <LocaleProvider>
        <TrayPanel state={bootstrap({ showAsUsed: false })} />
      </LocaleProvider>,
    );

    await waitFor(() => {
      const track = container.querySelector<HTMLElement>(
        ".provider-grid__weekly-track",
      );
      expect(track?.style.getPropertyValue("--weekly-pct")).toBe("65%");
    });
  });

  it("hides provider grid icons when the display setting is disabled", async () => {
    const { container } = renderTrayPanel(
      [provider("codex", "Codex"), provider("claude", "Claude")],
      { switcherShowsIcons: false },
    );

    await waitFor(() => {
      expect(container.querySelector(".provider-grid")).not.toBeNull();
    });

    const grid = container.querySelector(".provider-grid");
    expect(grid?.getAttribute("data-show-icons")).toBe("false");
    expect(grid?.classList.contains("provider-grid--no-icons")).toBe(true);
    expect(container.querySelector(".provider-icon")).toBeNull();
    expect(container.querySelector(".provider-grid__icon-overview")).toBeNull();
  });
});
