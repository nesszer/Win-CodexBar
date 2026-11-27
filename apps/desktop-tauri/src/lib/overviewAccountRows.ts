import { buildCodexAccountSurfaceLabels } from "../components/codexAccountDisplay";
import { buildAccountOrdinals } from "../components/claudeAccountDisplay";
import type {
  AccountUsageEntryBridge,
  AccountUsageWindowBridge,
  ClaudeSwapOverviewAccountsBridge,
  CodexAccount,
  CodexAccountUsageSnapshot,
  CodexUsageWindow,
  ProviderUsageSnapshot,
  RateWindowSnapshot,
} from "../types/bridge";

/**
 * Cached managed-Codex accounts, as the shared account store reports them.
 * Reading this never fetches; it is the same disk snapshot the Settings list uses.
 */
export interface CodexOverviewAccounts {
  accounts: readonly CodexAccount[];
  snapshots: Readonly<Record<string, CodexAccountUsageSnapshot>>;
  displayNames?: Readonly<Record<string, string>>;
  accountOrdinals: Readonly<Record<string, number>>;
}

/**
 * Saved Claude token accounts and their last successful usage. Both come from
 * cache-only reads (`get_token_accounts`, `get_provider_account_usage`).
 */
export interface ClaudeTokenOverviewAccounts {
  accounts: readonly { id: string; label: string }[];
  usage: Readonly<Record<string, AccountUsageEntryBridge>>;
}

export interface OverviewEntry {
  /** React key and DOM id suffix. A plain card keeps the provider id. */
  key: string;
  provider: ProviderUsageSnapshot;
  /** The account a row belongs to; null for an ordinary provider card. */
  accountId: string | null;
}

export interface OverviewRowOptions {
  hidePersonalInfo: boolean;
  /** Localized "Account", used for the generic privacy label. */
  accountWord: string;
  /** Localized "No usage data", shown for an account with no cached usage. */
  unavailableText: string;
}

const CODEX_PROVIDER_ID = "codex";
const CLAUDE_PROVIDER_ID = "claude";

/**
 * Expand each Overview provider into one row per cached account. Only the
 * already-selected providers are passed in, so rows never take a provider slot.
 *
 * Codex expands over its managed accounts. Claude expands over its claude-swap
 * or tcr pool accounts when two or more are listed, which takes precedence, and
 * otherwise over its saved token accounts. Managed Claude logins stay one card.
 * A provider with at most one account keeps its single card. Each row is built
 * from that account's own data and never borrows the active account's quota,
 * plan, email or cost.
 */
export function expandOverviewAccountRows(
  providers: readonly ProviderUsageSnapshot[],
  codex: CodexOverviewAccounts | null,
  options: OverviewRowOptions,
  claudeTokens: ClaudeTokenOverviewAccounts | null = null,
  claudeSwap: ClaudeSwapOverviewAccountsBridge | null = null,
): OverviewEntry[] {
  const plain = (provider: ProviderUsageSnapshot): OverviewEntry => ({
    key: provider.providerId,
    provider,
    accountId: null,
  });
  return providers.flatMap((provider): OverviewEntry[] => {
    if (provider.providerId === CODEX_PROVIDER_ID && codex && codex.accounts.length > 1) {
      return codexAccountRows(provider, codex, options);
    }
    if (provider.providerId === CLAUDE_PROVIDER_ID) {
      if (claudeSwap && claudeSwap.accounts.length > 1) {
        return claudeSwapAccountRows(provider, claudeSwap, options);
      }
      if (claudeTokens && claudeTokens.accounts.length > 1) {
        return claudeTokenAccountRows(provider, claudeTokens, options);
      }
    }
    return [plain(provider)];
  });
}

function codexAccountRows(
  provider: ProviderUsageSnapshot,
  codex: CodexOverviewAccounts,
  options: OverviewRowOptions,
): OverviewEntry[] {
  const labels = buildCodexAccountSurfaceLabels(
    codex.accounts,
    codex.displayNames ?? {},
    codex.accountOrdinals,
    options.hidePersonalInfo,
    options.accountWord,
  );
  return codex.accounts.map((account) => ({
    key: `${provider.providerId}/account/${account.id}`,
    provider: codexAccountOverviewSnapshot(
      provider,
      codex.snapshots[account.id] ?? null,
      labels[account.id] ?? "",
      options.unavailableText,
    ),
    accountId: account.id,
  }));
}

function claudeTokenAccountRows(
  provider: ProviderUsageSnapshot,
  claude: ClaudeTokenOverviewAccounts,
  options: OverviewRowOptions,
): OverviewEntry[] {
  const ordinals = buildAccountOrdinals(claude.accounts);
  return claude.accounts.map((account) => {
    const ordinal = ordinals[account.id];
    const label = options.hidePersonalInfo
      ? ordinal === undefined
        ? "••••"
        : `${options.accountWord} ${ordinal}`
      : account.label;
    return {
      key: `${provider.providerId}/account/${account.id}`,
      provider: claudeAccountOverviewSnapshot(
        provider,
        claude.usage[account.id] ?? null,
        label,
        options.unavailableText,
      ),
      accountId: account.id,
    };
  });
}

/**
 * One row per claude-swap or tcr pool account. Hidden personal info shows the
 * slot as `Account N`, and the real label never enters the row in that mode.
 */
function claudeSwapAccountRows(
  provider: ProviderUsageSnapshot,
  swap: ClaudeSwapOverviewAccountsBridge,
  options: OverviewRowOptions,
): OverviewEntry[] {
  return swap.accounts.map((account) => {
    const label = options.hidePersonalInfo
      ? `${options.accountWord} ${account.slot}`
      : account.label;
    return {
      key: `${provider.providerId}/account/${account.id}`,
      provider: claudeAccountOverviewSnapshot(
        provider,
        swap.usage[account.id] ?? null,
        label,
        options.unavailableText,
      ),
      accountId: account.id,
    };
  });
}

/**
 * Build one Codex account's card model. Only display identity (provider id, name,
 * source) comes from the provider card; plan, email, windows and cost come from
 * this account's own usage record.
 */
function codexAccountOverviewSnapshot(
  base: ProviderUsageSnapshot,
  usage: CodexAccountUsageSnapshot | null,
  label: string,
  unavailableText: string,
): ProviderUsageSnapshot {
  const windows = [usage?.primaryWindow, usage?.secondaryWindow]
    .map(codexRateWindow)
    .filter((window): window is RateWindowSnapshot => window !== null);
  const hasUsage = windows.length > 0;
  const primary = windows[0] ?? emptyRateWindow();
  return {
    providerId: base.providerId,
    displayName: base.displayName,
    primary,
    selectedMetric: primary,
    secondary: windows[1] ?? null,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [],
    cost: usage?.cost ?? null,
    subscription: usage?.subscription ?? null,
    planName: usage?.plan ?? null,
    accountEmail: label.trim() || null,
    sourceLabel: base.sourceLabel,
    updatedAt: usage?.updatedAt ?? base.updatedAt,
    error: hasUsage ? null : unavailableText,
    errorState: hasUsage ? "ready" : "unknown",
    pace: null,
    accountOrganization: null,
    trayStatusLabel: null,
    primaryLabel: null,
    secondaryLabel: null,
    tertiaryLabel: null,
    accountLabel: null,
    hasSuccessfulClaudeCliQuota: false,
    fetchDurationMs: null,
    wayfinderUsage: null,
  };
}

/**
 * Build one Claude account's card model (token or claude-swap/tcr) from its
 * cached last-fetched usage. The cache holds windows and an update time only, so
 * plan, cost and subscription stay empty. A missing entry renders as "No usage
 * data" with an empty update time ("Not fetched yet"); it never shows the active
 * account's time.
 */
function claudeAccountOverviewSnapshot(
  base: ProviderUsageSnapshot,
  entry: AccountUsageEntryBridge | null,
  label: string,
  unavailableText: string,
): ProviderUsageSnapshot {
  const primaryWindow = claudeRateWindow(entry?.primary);
  const secondaryWindow = claudeRateWindow(entry?.secondary);
  const hasUsage = primaryWindow !== null || secondaryWindow !== null;
  const primary = primaryWindow ?? emptyRateWindow();
  return {
    providerId: base.providerId,
    displayName: base.displayName,
    primary,
    selectedMetric: primary,
    secondary: secondaryWindow,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [],
    cost: null,
    subscription: null,
    planName: null,
    accountEmail: label.trim() || null,
    sourceLabel: base.sourceLabel,
    updatedAt: entry?.updatedAt ?? "",
    error: hasUsage ? null : unavailableText,
    errorState: hasUsage ? "ready" : "unknown",
    pace: null,
    accountOrganization: null,
    trayStatusLabel: null,
    primaryLabel: null,
    secondaryLabel: null,
    tertiaryLabel: null,
    accountLabel: null,
    hasSuccessfulClaudeCliQuota: false,
    fetchDurationMs: null,
    wayfinderUsage: null,
  };
}

function codexRateWindow(window: CodexUsageWindow | null | undefined): RateWindowSnapshot | null {
  if (!window) return null;
  return rateWindow(
    window.usedPercent,
    window.limitWindowSeconds > 0 ? Math.round(window.limitWindowSeconds / 60) : null,
    window.resetAt,
  );
}

function claudeRateWindow(
  window: AccountUsageWindowBridge | null | undefined,
): RateWindowSnapshot | null {
  if (!window) return null;
  return rateWindow(window.usedPercent, window.windowMinutes ?? null, window.resetsAt ?? null);
}

function rateWindow(
  rawUsedPercent: number,
  windowMinutes: number | null,
  resetsAt: string | null,
): RateWindowSnapshot {
  const usedPercent = Math.min(100, Math.max(0, rawUsedPercent));
  return {
    usedPercent,
    remainingPercent: 100 - usedPercent,
    windowMinutes,
    resetsAt,
    resetDescription: null,
    isExhausted: usedPercent >= 100,
    reservePercent: null,
    reserveDescription: null,
    isInformational: false,
    descriptionIsDetail: false,
    reserveWillLastToReset: false,
    reserveEtaSeconds: null,
    pace: null,
    monthlyLimitBlock: null,
  };
}

function emptyRateWindow(): RateWindowSnapshot {
  return {
    usedPercent: 0,
    remainingPercent: 100,
    windowMinutes: null,
    resetsAt: null,
    resetDescription: null,
    isExhausted: false,
    reservePercent: null,
    reserveDescription: null,
    isInformational: false,
    descriptionIsDetail: false,
    reserveWillLastToReset: false,
    reserveEtaSeconds: null,
    pace: null,
    monthlyLimitBlock: null,
  };
}
