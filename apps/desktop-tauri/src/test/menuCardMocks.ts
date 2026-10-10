// Shared by the split test files; each file registers these with vi.mock.
import { vi } from "vitest";

export const tauriMocks = {
  getProviderChartData: vi.fn(),
  getDeepSeekPricingStatus: vi.fn(),
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
  claudeAccountsList: vi.fn(),
  getCodexAccountsState: vi.fn(),
};

export const eventMocks = {
  listen: vi.fn(),
};
