import { beforeEach, vi } from "vitest";
import { render } from "@testing-library/react";
import FloatBar from "../floatbar/FloatBar";
import { LocaleProvider } from "../i18n/LocaleProvider";
import { buildBundle } from "./localeHarness";
import type {
  BootstrapState,
  ProviderUsageSnapshot,
  RateWindowSnapshot,
  SettingsSnapshot,
} from "../types/bridge";
import { makeRateWindow, makeSettings, makeUsageSnapshot } from "./fixtures";
import { tauriMocks, eventMocks } from "./floatBarMocks";

export type RateWindowOptions = {
  exhausted?: boolean;
  informational?: boolean;
  resetsAt?: string | null;
  resetDescription?: string | null;
  descriptionIsDetail?: boolean;
};

export function rateWindow(used: number, opts: RateWindowOptions = {}): RateWindowSnapshot {
  return makeRateWindow(used, {
    resetsAt: opts.resetsAt ?? null,
    resetDescription: opts.resetDescription ?? null,
    isExhausted: opts.exhausted ?? false,
    isInformational: opts.informational,
    descriptionIsDetail: opts.descriptionIsDetail,
  });
}

export function snapshot(
  id: string,
  display: string,
  used: number,
  opts: {
    exhausted?: boolean;
    error?: string | null;
    errorState?: ProviderUsageSnapshot["errorState"];
    resetsAt?: string | null;
    resetDescription?: string | null;
    descriptionIsDetail?: boolean;
    informational?: boolean;
    secondary?: {
      used: number;
      exhausted?: boolean;
      informational?: boolean;
      resetsAt?: string | null;
      resetDescription?: string | null;
    };
    selected?: {
      used: number;
      exhausted?: boolean;
      informational?: boolean;
      resetsAt?: string | null;
      resetDescription?: string | null;
    };
  } = {},
): ProviderUsageSnapshot {
  const primary = rateWindow(used, opts);
  const secondary = opts.secondary
    ? rateWindow(opts.secondary.used, opts.secondary)
    : null;
  const selectedMetric = opts.selected
    ? rateWindow(opts.selected.used, opts.selected)
    : primary.isInformational && secondary && !secondary.isInformational
      ? secondary
      : primary;

  return makeUsageSnapshot(id, {
    displayName: display,
    primary,
    selectedMetric,
    secondary,
    updatedAt: "2026-05-15T00:00:00Z",
    error: opts.error ?? null,
    errorState: opts.errorState ?? "ready",
  });
}

export function settings(overrides: Partial<SettingsSnapshot> = {}): SettingsSnapshot {
  return makeSettings({ enabledProviders: ["claude", "codex"], floatBarEnabled: true, ...overrides });
}

export function bootstrap(settingsOverrides: Partial<SettingsSnapshot> = {}): BootstrapState {
  return {
    contractVersion: "v1",
    providers: [],
    settings: settings(settingsOverrides),
  };
}

export function renderFloatBar(state: BootstrapState) {
  return render(
    <LocaleProvider>
      <FloatBar state={state} />
    </LocaleProvider>,
  );
}

export function setupFloatBarTests() {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.refreshProviders.mockResolvedValue(undefined);
    tauriMocks.refreshProvidersIfStale.mockResolvedValue(undefined);
    tauriMocks.getProviderLocalUsageSummary.mockResolvedValue(null);
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({
        ResetsInHoursMinutes: "Resets in {}h {}m",
        ResetsInDaysHours: "Resets in {}d {}h",
        ResetsInHoursOnly: "Resets in {}h",
        ProviderTextNoActiveSession: "No active 5h session",
        ProviderTextApiRate: "{} API-rate",
        ProviderTextNoBudgetSet: "No budget set",
        TrayResetsDueNow: "Resetting",
        PanelToday: "Today",
        PanelUsedSuffix: "used",
        OverviewSpendEstimate: "Estimate",
        ProviderIssueAuthRequired: "Sign-in required",
        ProviderIssueSessionExpired: "Session expired",
        ProviderIssueLocalRuntimeOffline: "Local runtime offline",
        ProviderIssueUnknown: "Usage unavailable",
        CostPeriodShortMonthToDate: "MTD",
        CostPeriodShortDays: "{}d",
        FloatBarNoProviders: "No providers",
        FloatBarRemainingSuffix: "remaining",
      }),
    );
    eventMocks.listen.mockResolvedValue(() => {});
  });
}
