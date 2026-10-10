// Shared by the split test files; each file registers these with vi.mock.
import { vi } from "vitest";

export const tauriMocks = {
  getCachedProviders: vi.fn(),
  getProviderChartData: vi.fn(),
  getProviderLocalUsageSummary: vi.fn(),
  refreshProviders: vi.fn(),
  refreshProvidersIfStale: vi.fn(),
  getSettingsSnapshot: vi.fn(),
  updateSettings: vi.fn(),
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
};

export const eventMocks = {
  listen: vi.fn(),
};

export const windowMocks = {
  getCurrentWindow: vi.fn(() => ({
    startDragging: vi.fn().mockResolvedValue(undefined),
  })),
};

export const coreMocks = {
  invoke: vi.fn().mockResolvedValue(undefined),
};
