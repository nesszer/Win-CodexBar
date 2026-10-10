import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type {
  ProviderCatalogEntry,
  ProviderUsageSnapshot,
  SettingsSnapshot,
} from "../../../types/bridge";

const hookMocks = vi.hoisted(() => ({
  useProviders: vi.fn(),
}));

vi.mock("../../../hooks/useProviders", () => hookMocks);
vi.mock("../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => key }),
}));
vi.mock("../../../lib/tauri", () => ({
  reorderProviders: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("../providers/ProviderDetailPane", () => ({
  ProviderDetailPane: () => null,
}));

import ProvidersTab from "./ProvidersTab";
import { makeRateWindow } from "../../../test/fixtures";

const provider: ProviderCatalogEntry = {
  id: "codex",
  displayName: "Codex",
  cookieDomain: null,
};

const settings = {
  enabledProviders: [provider.id],
  resetTimeRelative: true,
  providerMetrics: {},
} as SettingsSnapshot;

describe("ProvidersTab", () => {
  it("shows the real secondary percentage when the primary is informational", () => {
    const snapshot: ProviderUsageSnapshot = {
      providerId: provider.id,
      displayName: provider.displayName,
      primary: makeRateWindow(0, { isInformational: true }),
      selectedMetric: makeRateWindow(42),
      secondary: makeRateWindow(42),
      modelSpecific: null,
      tertiary: null,
      extraRateWindows: [],
      cost: null,
      planName: null,
      accountEmail: null,
      sourceLabel: "auto",
      updatedAt: new Date().toISOString(),
      error: null,
      errorState: "ready",
      pace: null,
      accountOrganization: null,
      trayStatusLabel: null,
    };
    hookMocks.useProviders.mockReturnValue({ providers: [snapshot] });

    render(
      <ProvidersTab
        settings={settings}
        providers={[provider]}
        set={vi.fn()}
        saving={false}
      />,
    );

    expect(screen.getByText("42%")).toBeInTheDocument();
  });
});
