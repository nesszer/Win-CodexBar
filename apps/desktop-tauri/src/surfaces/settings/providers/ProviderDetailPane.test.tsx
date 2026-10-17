import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProviderDetail, SettingsSnapshot } from "../../../types/bridge";

const tauri = vi.hoisted(() => ({
  getCredentialStorageStatus: vi.fn(),
  getProviderCookieSourceOptions: vi.fn(),
  getProviderDetail: vi.fn(),
  getProviderGatewayUrl: vi.fn(),
  getProviderRegionOptions: vi.fn(),
  getTokenAccountProviders: vi.fn(),
  openProviderDashboard: vi.fn(),
  openProviderStatusPage: vi.fn(),
  refreshProviders: vi.fn(),
  revokeProviderCredentials: vi.fn(),
  setProviderGatewayUrl: vi.fn(),
  triggerProviderLogin: vi.fn(),
  setProviderAutoResumeAfterQuotaReset: vi.fn(),
  setProviderOptionalDetails: vi.fn(),
}));
vi.mock("../../../lib/tauri", () => tauri);
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }));
vi.mock("../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => key, language: "english" }),
}));

// Sections that load their own data through the bridge are stubbed; the test
// covers where the pane places them, not what they load.
const { stub } = vi.hoisted(() => ({
  stub: (name: string) => () => <div data-testid={name} />,
}));
vi.mock("./sections/charts/ChartsSection", () => ({ ChartsSection: stub("charts") }));
vi.mock("./sections/UsageDetailsSection", () => ({ UsageDetailsSection: () => null }));
vi.mock("./sections/UsageSourceSection", () => ({ UsageSourceSection: stub("usage-source") }));
vi.mock("./sections/CookieSourceSection", () => ({ CookieSourceSection: () => null }));
vi.mock("./sections/RegionSection", () => ({ RegionSection: () => null }));
vi.mock("./sections/CredentialsDispatcher", () => ({ CredentialsDispatcher: () => null }));
vi.mock("./ApiKeySection", () => ({ ApiKeySection: stub("api-key") }));
vi.mock("./CookieSection", () => ({ CookieSection: () => null }));
vi.mock("../tokens/TokenAccountsPanel", () => ({ TokenAccountsPanel: () => null }));
vi.mock("./sections/credentials/CodexAccountsSection", () => ({
  CodexAccountsSection: stub("codex-accounts"),
}));
vi.mock("./sections/credentials/GrokAccountsSection", () => ({ GrokAccountsSection: () => null }));
vi.mock("./sections/credentials/ClaudeAccountsSection", () => ({
  ClaudeAccountsSection: ({
    claudeSwapEnabled,
    claudeSwapExecutablePath,
  }: {
    claudeSwapEnabled?: boolean;
    claudeSwapExecutablePath?: string;
  }) => (
    <div
      data-testid="claude-accounts"
      data-swap={String(claudeSwapEnabled)}
      data-path={claudeSwapExecutablePath}
    />
  ),
}));

import { ProviderDetailPane } from "./ProviderDetailPane";

function rateWindow(usedPercent: number) {
  return {
    usedPercent,
    remainingPercent: 100 - usedPercent,
    windowMinutes: 300,
    resetsAt: null,
    resetDescription: null,
    isExhausted: false,
    reservePercent: null,
    reserveDescription: null,
  };
}

function detail(id: string, extra: Partial<ProviderDetail> = {}): ProviderDetail {
  return {
    id,
    displayName: id === "codex" ? "Codex" : "Claude",
    enabled: true,
    autoResumeAfterQuotaReset: false,
    autoResumeSupported: false,
    optionalDetailsSupported: false,
    optionalDetailsEnabled: false,
    email: null,
    plan: "Pro",
    authType: null,
    sourceLabel: "oauth",
    organization: null,
    lastUpdated: null,
    session: rateWindow(30),
    weekly: rateWindow(50),
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [],
    cost: null,
    pace: null,
    lastError: null,
    errorState: null,
    dashboardUrl: "https://example.invalid/usage",
    statusPageUrl: null,
    buyCreditsUrl: null,
    hasSnapshot: true,
    cookieSource: null,
    region: null,
    ...extra,
  };
}

const baseSettings = {
  enabledProviders: ["codex", "claude"],
  resetTimeRelative: true,
  showAsUsed: false,
  providerMetrics: {},
  providerAccentColors: {},
  copilotSeatCreditEntitlement: null,
  hidePersonalInfo: false,
  highUsageThreshold: 80,
  criticalUsageThreshold: 95,
  providerQuotaWarnings: {},
  showNotifications: true,
  quotaWarningMarkersVisible: true,
  claudeAvoidKeychainPrompts: false,
  claudeAllowReadingClaudeCodeCredentials: false,
  claudeSwapEnabled: false,
  claudeSwapExecutablePath: "",
  disableKeychainAccess: false,
} as unknown as SettingsSnapshot;

const catalog = [{ id: "codex" }, { id: "claude" }, { id: "gemini" }];

async function renderPane(
  providerId: string,
  settings: SettingsSnapshot = baseSettings,
  onSettingsChange = vi.fn(),
) {
  const view = render(
    <ProviderDetailPane
      providerId={providerId}
      settings={settings}
      catalog={catalog}
      resetTimeRelative
      providerMetrics={settings.providerMetrics}
      copilotSeatCreditEntitlement={settings.copilotSeatCreditEntitlement}
      providerAccentColors={settings.providerAccentColors}
      wayfinderGatewayUrl="http://127.0.0.1:8088"
      hidePersonalInfo={false}
      settingsDisabled={false}
      onSettingsChange={onSettingsChange}
    />,
  );
  await screen.findByText(providerId === "codex" ? "Codex" : "Claude");
  return { ...view, onSettingsChange };
}

describe("ProviderDetailPane", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauri.getProviderCookieSourceOptions.mockResolvedValue([]);
    tauri.getProviderRegionOptions.mockResolvedValue([]);
    tauri.getCredentialStorageStatus.mockResolvedValue(null);
    tauri.getTokenAccountProviders.mockResolvedValue([]);
    tauri.refreshProviders.mockResolvedValue(undefined);
  });

  it("orders header, usage, error, connection, fields, accounts and the trailing groups", async () => {
    tauri.getProviderDetail.mockResolvedValue(
      detail("codex", { lastError: "boom", errorState: "unknown" }),
    );
    const { container } = await renderPane("codex");
    const order = [
      ".provider-detail-header-group",
      ".provider-usage-group",
      ".provider-detail-error-group",
      ".provider-metric-group",
      ".provider-connection-group",
      ".provider-fields-group",
      ".provider-accounts-group",
      ".provider-accent-color-group",
      ".provider-quota-warnings",
      ".provider-options-group",
    ].map((selector) => container.querySelector(selector));
    expect(order.every(Boolean)).toBe(true);
    for (let i = 1; i < order.length; i += 1) {
      expect(
        order[i - 1]!.compareDocumentPosition(order[i]!) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
    }
    const usage = container.querySelector(".provider-usage-group")!;
    expect(within(usage as HTMLElement).getByTestId("charts")).toBeTruthy();
    expect(within(usage as HTMLElement).getByText("ActionUsageDashboard")).toBeTruthy();
    expect(within(usage as HTMLElement).queryByText("ActionRefresh")).toBeNull();
    const accounts = container.querySelector(".provider-accounts-group")!;
    expect(within(accounts as HTMLElement).getByTestId("codex-accounts")).toBeTruthy();
  });

  it("leaves out the error group when the last fetch recorded no error", async () => {
    tauri.getProviderDetail.mockResolvedValue(detail("codex", { errorState: "ready" }));
    const { container } = await renderPane("codex");
    expect(container.querySelector(".provider-detail-error-group")).toBeNull();
  });

  it("drives the enabled-providers list from the header switch", async () => {
    tauri.getProviderDetail.mockResolvedValue(detail("codex"));
    const { onSettingsChange } = await renderPane("codex");
    const toggle = screen.getByRole("checkbox", { name: "ProviderEnabled" });
    expect(toggle).toBeChecked();
    await act(async () => fireEvent.click(toggle));
    expect(onSettingsChange).toHaveBeenCalledWith({ enabledProviders: ["claude"] });
  });

  it("shows the header switch off for a disabled provider and turns it on in catalog order", async () => {
    tauri.getProviderDetail.mockResolvedValue(detail("codex"));
    const settings = { ...baseSettings, enabledProviders: ["claude"] } as SettingsSnapshot;
    const { onSettingsChange } = await renderPane("codex", settings);
    const toggle = screen.getByRole("checkbox", { name: "ProviderEnabled" });
    expect(toggle).not.toBeChecked();
    await act(async () => fireEvent.click(toggle));
    expect(onSettingsChange).toHaveBeenCalledWith({ enabledProviders: ["codex", "claude"] });
  });

  it("refreshes from the header button", async () => {
    tauri.getProviderDetail.mockResolvedValue(detail("codex"));
    await renderPane("codex");
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "ActionRefresh" })));
    expect(tauri.refreshProviders).toHaveBeenCalledTimes(1);
  });

  it("shows the claude-swap path field under its Options toggle only while claude-swap is on", async () => {
    tauri.getProviderDetail.mockResolvedValue(detail("claude"));
    const { unmount } = await renderPane("claude");
    expect(screen.queryByRole("textbox", { name: "ClaudeSwapExecutablePath" })).toBeNull();
    expect(screen.getByTestId("claude-accounts")).toHaveAttribute("data-swap", "false");
    unmount();

    const on = {
      ...baseSettings,
      claudeSwapEnabled: true,
      claudeSwapExecutablePath: "C:/tools/cswap.exe",
    } as SettingsSnapshot;
    const view = await renderPane("claude", on);
    const fields = view.container.querySelector(".provider-fields-group") as HTMLElement;
    expect(within(fields).queryByRole("textbox", { name: "ClaudeSwapExecutablePath" })).toBeNull();
    const options = view.container.querySelector(".provider-options-group") as HTMLElement;
    const input = within(options).getByRole("textbox", { name: "ClaudeSwapExecutablePath" });
    expect(input).toHaveValue("C:/tools/cswap.exe");
    const toggleRow = within(options)
      .getByRole("checkbox", { name: "ClaudeSwapEnable" })
      .closest(".settings-field")!;
    expect(toggleRow.nextElementSibling).toBe(input.closest(".settings-field"));
    expect(screen.getByTestId("claude-accounts")).toHaveAttribute("data-swap", "true");
    expect(screen.getByTestId("claude-accounts")).toHaveAttribute("data-path", "C:/tools/cswap.exe");
  });

  it("locks Avoid keychain prompts while Credential Manager access is disabled", async () => {
    tauri.getProviderDetail.mockResolvedValue(detail("claude"));
    const settings = { ...baseSettings, disableKeychainAccess: true } as SettingsSnapshot;
    const { container } = await renderPane("claude", settings);
    const options = container.querySelector(".provider-options-group") as HTMLElement;
    const row = within(options).getByText("ProviderClaudeAvoidKeychainPrompts").closest(".settings-field") as HTMLElement;
    expect(within(row).getByRole("checkbox")).toBeDisabled();
  });
});
