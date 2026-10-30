import type {
  AccountUsageEntryBridge,
  AgentSession,
  AgentSessionDiscoveryResult,
  ClaudeAccount,
  ClaudeSwapAccount,
  ClaudeSwapAccountsState,
  CodexAccount,
  CodexAccountsStateBridge,
  CodexAccountUsageSnapshot,
  CodexSessionDetailError,
  CodexSessionDetails,
  CodexSessionToolActivity,
  CodexSessionTurn,
  CodexToolOperation,
  CodexWorkspacesSessionUsage,
  DetectedBrowserBridge,
  ProviderCatalogEntry,
  ProviderUsageSnapshot,
  QuotaWindowHistoryBridge,
  SettingsSnapshot,
  SpendModelRow,
  TokenAccountBridge,
} from "../../types/bridge";
import { ukLocaleOverrides } from "../locale";
import {
  PROOF_PROVIDER_CATALOG,
  baseSettings,
  isoAfter,
  isoBefore,
  providerSnapshot,
  rateWindow,
} from "./common";

export const PROOF_SCENARIO_NAMES = [
  "default",
  "two-codex-accounts",
  "overview-all-accounts",
  "overview-all-accounts-private",
  "errors",
  "empty",
  "cursor-weekly",
  "openai-live-spend",
  "claude-plan-hidden",
  "codex-tool-activity",
  "usage-spend-unknown-tokens",
  "claude-swap-tcr",
  "claude-swap-tcr-remaining",
  "overview-claude-swap-accounts",
  "overview-claude-swap-accounts-private",
  "overview-tcr-accounts",
  "overview-tcr-accounts-private",
  "codex-conversations",
  "codex-conversations-private",
  "codex-session-inspector",
  "claude-reset-credits",
  "claude-reset-credits-hidden",
  "kpi-long-titles",
  "uk-regen-countdown",
  "claudeplatform",
  "cookie-no-browsers",
  "vercel",
  "vercel-team-id-invalid",
  "kimi-bonus-credits",
  "agent-sessions-hosts",
  "copilot-daily-target",
] as const;

export type ProofScenarioName = (typeof PROOF_SCENARIO_NAMES)[number];

/** Everything the mocked bridge serves for one scenario. All data is synthetic. */
export interface ProofScenario {
  name: ProofScenarioName;
  settings: SettingsSnapshot;
  catalog: ProviderCatalogEntry[];
  providers: ProviderUsageSnapshot[];
  codexAccounts: CodexAccountsStateBridge;
  claudeAccounts: ClaudeAccount[];
  /** Local-log spend; providers without an entry have no local logs. */
  localSpend: LocalSpendFixture[];
  /** Claude token accounts; absent means Claude offers no token accounts. */
  claudeTokenAccounts?: TokenAccountBridge[];
  /** Cached last usage per Claude token account id; an account without an entry shows no usage. */
  claudeTokenAccountUsage?: Record<string, AccountUsageEntryBridge>;
  /** External claude-swap/tcr accounts; absent means the adapter is off. */
  claudeSwapAccounts?: ClaudeSwapAccountsState;
  /** Cached last usage per claude-swap/tcr account id, for Overview rows. Absent means none cached. */
  claudeSwapOverviewUsage?: Record<string, AccountUsageEntryBridge>;
  /** Quota-window history served with chart data; absent means none. */
  quotaWindowHistory?: QuotaWindowHistoryBridge[];
  /** Synthetic replacements for single locale keys; absent means the shipped English strings. */
  localeOverrides?: Record<string, string>;
  /** Local Codex conversations in the Usage & Spend contract, unmasked. */
  codexConversations?: CodexWorkspacesSessionUsage[];
  /** Per-session Codex tool activity, unredacted; the router redacts it under Hide personal information. */
  codexToolActivity?: CodexSessionToolActivity[];
  /** Per-session request-level usage; a session with neither details nor an error is unavailable. */
  codexSessionDetails?: CodexSessionDetails[];
  /** Code `get_codex_session_details` rejects with, by session id. */
  codexSessionDetailErrors?: Record<string, CodexSessionDetailError>;
  /** Per-model rows in the Codex Usage & Spend contract; a null token count renders as unknown. */
  codexSpendModels?: SpendModelRow[];
  /** Browsers the cookie section detects; absent means the router's default pair. */
  detectedBrowsers?: DetectedBrowserBridge[];
  /** Saved workspace, team or base-URL values by provider id; absent means none saved. */
  workspaceIds?: Record<string, string>;
  /** Error a workspace-value save rejects with, by provider id; absent means saves succeed. */
  workspaceIdSaveErrors?: Record<string, string>;
  /** Agent-session discovery served by `list_agent_sessions`; absent means disabled. */
  agentSessions?: AgentSessionDiscoveryResult;
}

export interface LocalSpendFixture {
  providerId: string;
  daily: number;
  tokensPerDay: number;
  model: string;
}

function codexSpend(): LocalSpendFixture {
  return { providerId: "codex", daily: 1.84, tokensPerDay: 412_000, model: "gpt-5-codex" };
}

function claudeSpend(): LocalSpendFixture {
  return { providerId: "claude", daily: 2.37, tokensPerDay: 289_000, model: "claude-sonnet-4" };
}

const SESSION_MINUTES = 300;
const WEEK_MINUTES = 10_080;
const MONTH_MINUTES = 43_200;
const DAY_MINUTES = 1_440;

function codexSnapshot(now: Date, email: string): ProviderUsageSnapshot {
  return providerSnapshot(
    {
      providerId: "codex",
      displayName: "Codex",
      primary: rateWindow(now, 34, SESSION_MINUTES, 142),
      primaryLabel: "Session",
      updatedAt: isoBefore(now, 1),
    },
    {
      secondary: rateWindow(now, 58, WEEK_MINUTES, 3 * DAY_MINUTES + 190),
      secondaryLabel: "Weekly",
      planName: "Plus",
      accountEmail: email,
      sourceLabel: "oauth",
      pace: {
        stage: "slightly_behind",
        deltaPercent: 4,
        willLastToReset: true,
        etaSeconds: null,
        expectedUsedPercent: 54,
        actualUsedPercent: 58,
      },
    },
  );
}

function claudeSnapshot(now: Date): ProviderUsageSnapshot {
  return providerSnapshot(
    {
      providerId: "claude",
      displayName: "Claude",
      primary: rateWindow(now, 22, SESSION_MINUTES, 61),
      primaryLabel: "Session (5h)",
      updatedAt: isoBefore(now, 2),
    },
    {
      secondary: rateWindow(now, 41, WEEK_MINUTES, 5 * DAY_MINUTES + 30),
      secondaryLabel: "Weekly",
      planName: "Max",
      accountEmail: "claude.user@example.com",
      accountOrganization: "Example Studio",
      sourceLabel: "oauth",
      // Synthetic renewal date; calendar-only, so it renders as "Plan renews: Dec 1, 2026".
      subscription: {
        startsAt: null,
        expiresAt: null,
        renewsAt: "2026-12-01T00:00:00Z",
        renewsAtIsDateOnly: true,
      },
    },
  );
}

function cursorSnapshot(now: Date, updatedMinutesAgo = 3): ProviderUsageSnapshot {
  return providerSnapshot(
    {
      providerId: "cursor",
      displayName: "Cursor",
      primary: rateWindow(now, 62, MONTH_MINUTES, 12 * DAY_MINUTES),
      primaryLabel: "Plan",
      updatedAt: isoBefore(now, updatedMinutesAgo),
    },
    {
      planName: "Pro",
      accountEmail: "cursor.user@example.com",
      sourceLabel: "web",
    },
  );
}

/**
 * X Subscription Cursor account (upstream #4695): the unified weekly pool is the
 * primary "Weekly" meter. The monthly Auto/API lanes and the Grok Bot extra are
 * gone; on-demand spend stays in the cost section.
 */
function cursorWeeklySnapshot(now: Date): ProviderUsageSnapshot {
  return providerSnapshot(
    {
      providerId: "cursor",
      displayName: "Cursor",
      primary: rateWindow(now, 10.2, WEEK_MINUTES, 5 * DAY_MINUTES),
      primaryLabel: "Weekly",
      updatedAt: isoBefore(now, 3),
    },
    {
      planName: "Cursor Enterprise",
      accountEmail: "cursor.user@example.com",
      sourceLabel: "web",
      cost: {
        used: 4.47,
        limit: null,
        remaining: null,
        currencyCode: "USD",
        period: "On-demand (billing cycle)",
        resetsAt: isoAfter(now, 12 * DAY_MINUTES),
        formattedUsed: "$4.47",
        formattedLimit: null,
      },
    },
  );
}

function geminiSnapshot(now: Date): ProviderUsageSnapshot {
  return providerSnapshot(
    {
      providerId: "gemini",
      displayName: "Gemini",
      primary: rateWindow(now, 15, DAY_MINUTES, 9 * 60),
      primaryLabel: "Daily",
      updatedAt: isoBefore(now, 4),
    },
    {
      planName: "Free",
      accountEmail: "gemini.user@example.com",
      sourceLabel: "cli",
    },
  );
}

function copilotSnapshot(now: Date): ProviderUsageSnapshot {
  return providerSnapshot(
    {
      providerId: "copilot",
      displayName: "Copilot",
      primary: rateWindow(now, 48, MONTH_MINUTES, 18 * DAY_MINUTES),
      primaryLabel: "Premium",
      updatedAt: isoBefore(now, 5),
    },
    {
      planName: "Business",
      accountEmail: "copilot.user@example.com",
      sourceLabel: "api",
    },
  );
}

/** Console prepaid credits; siloed from Claude: no plan or email, ever. */
function claudePlatformSnapshot(now: Date): ProviderUsageSnapshot {
  return providerSnapshot(
    {
      providerId: "claudeplatform",
      displayName: "Claude Platform",
      primary: rateWindow(now, 38, MONTH_MINUTES, 21 * DAY_MINUTES),
      primaryLabel: "API credit",
      updatedAt: isoBefore(now, 2),
    },
    {
      secondary: rateWindow(now, 12, MONTH_MINUTES, 45 * DAY_MINUTES),
      secondaryLabel: "Purchased credit",
      accountOrganization: "Example Studio",
      sourceLabel: "web",
    },
  );
}

/**
 * Opt-in personal AI Gateway budget (synthetic): spend, limit and remaining
 * rows under a "Personal budget" section, with the team as the organization.
 */
function vercelSnapshot(now: Date): ProviderUsageSnapshot {
  return providerSnapshot(
    {
      providerId: "vercel",
      displayName: "Vercel",
      primary: rateWindow(now, 42, MONTH_MINUTES, 17 * DAY_MINUTES),
      primaryLabel: "Personal budget",
      updatedAt: isoBefore(now, 2),
    },
    {
      secondary: null,
      accountOrganization: "Example Team",
      sourceLabel: "api",
      paceAuthoritative: false,
      displayDetails: [
        {
          id: "vercel-budget-spend",
          sectionTitle: "Personal budget",
          title: "Spend",
          value: "$42.00",
          secondaryValue: null,
          progress: null,
        },
        {
          id: "vercel-budget-limit",
          sectionTitle: "Personal budget",
          title: "Limit",
          value: "$100.00",
          secondaryValue: null,
          progress: null,
        },
        {
          id: "vercel-budget-remaining",
          sectionTitle: "Personal budget",
          title: "Remaining",
          value: "$58.00",
          secondaryValue: null,
          progress: null,
        },
      ],
    },
  );
}

/**
 * Lapsed Kimi plan (upstream #4700): no Code pool, so the weekly lane is
 * informational. The membership "Total usage" pool and two live gift pools
 * (upstream #4691) remain. Expired and malformed gifts never reach the fixture.
 */
function kimiSnapshot(now: Date): ProviderUsageSnapshot {
  return providerSnapshot(
    {
      providerId: "kimi",
      displayName: "Kimi",
      primary: { ...rateWindow(now, 0, null, null), isInformational: true, resetDescription: "No weekly quota reported" },
      primaryLabel: "Weekly",
      updatedAt: isoBefore(now, 1),
    },
    {
      extraRateWindows: [
        {
          id: "kimi-monthly",
          title: "Total usage",
          window: rateWindow(now, 36, MONTH_MINUTES, 14 * DAY_MINUTES),
        },
      ],
      accountEmail: null,
      sourceLabel: "web",
      displayDetails: [
        {
          id: "kimi-gift-0",
          sectionTitle: "Bonus credits",
          title: "Invite to Earn Credit",
          value: "75% left",
          secondaryValue: "Expires Dec 31, 2026 at 11:59 PM",
          progress: { used: 0.25, total: 1 },
        },
        {
          id: "kimi-gift-1",
          sectionTitle: "Bonus credits",
          title: "Welcome Credit",
          value: "40% left",
          secondaryValue: "Expires Jan 15, 2027 at 9:00 AM",
          progress: { used: 0.6, total: 1 },
        },
      ],
    },
  );
}

function codexAccount(
  now: Date,
  id: string,
  email: string,
  source: CodexAccount["source"],
  nickname: string | null,
): CodexAccount {
  return {
    id,
    nickname,
    emailHint: email,
    authSubject: null,
    providerAccountId: null,
    codexHomePath: source === "ambient" ? "C:\\Users\\proof\\.codex" : `C:\\proof\\codex-homes\\${id}`,
    source,
    createdAt: isoBefore(now, 30 * DAY_MINUTES),
    updatedAt: isoBefore(now, 60),
    lastAuthenticatedAt: isoBefore(now, 2 * DAY_MINUTES),
  };
}

function codexAccountUsage(
  now: Date,
  email: string,
  plan: string,
  sessionUsed: number,
  weeklyUsed: number,
): CodexAccountUsageSnapshot {
  return {
    email,
    providerAccountId: null,
    plan,
    allowed: true,
    limitReached: false,
    primaryWindow: {
      usedPercent: sessionUsed,
      resetAt: isoAfter(now, 142),
      limitWindowSeconds: SESSION_MINUTES * 60,
    },
    secondaryWindow: {
      usedPercent: weeklyUsed,
      resetAt: isoAfter(now, 3 * DAY_MINUTES + 190),
      limitWindowSeconds: WEEK_MINUTES * 60,
    },
    credits: null,
    updatedAt: isoBefore(now, 1),
  };
}

function singleCodexAccount(now: Date): CodexAccountsStateBridge {
  const email = "dev@example.com";
  return {
    accounts: [codexAccount(now, "proof-codex-ambient", email, "ambient", null)],
    accountOrdinals: { "proof-codex-ambient": 1 },
    snapshots: { "proof-codex-ambient": codexAccountUsage(now, email, "Plus", 34, 58) },
  };
}

function claudeAccounts(now: Date): ClaudeAccount[] {
  return [
    {
      id: "proof-claude-1",
      email: "claude.user@example.com",
      organization: "Example Studio",
      plan: "Max",
      isActive: true,
      isSaved: true,
      usage: {
        fiveHour: { usedPercent: 22, resetsAt: isoAfter(now, 61) },
        sevenDay: { usedPercent: 41, resetsAt: isoAfter(now, 5 * DAY_MINUTES + 30) },
        updatedAt: isoBefore(now, 2),
      },
    },
  ];
}

const CLAUDE_WORK_TOKEN_ID = "00000000-0000-4000-8000-00000000c1a1";
const CLAUDE_PERSONAL_TOKEN_ID = "00000000-0000-4000-8000-00000000c1a2";

function claudeTokenAccounts(): TokenAccountBridge[] {
  return [
    {
      id: CLAUDE_WORK_TOKEN_ID,
      label: "Work",
      addedAt: "Oct 01, 2026",
      lastUsed: null,
      isActive: true,
      claudeConfigDirectory: "C:\\Users\\proof\\.claude-work",
      claudeConfigDirectoryAvailable: false,
    },
    {
      id: CLAUDE_PERSONAL_TOKEN_ID,
      label: "Personal",
      addedAt: "Oct 02, 2026",
      lastUsed: null,
      isActive: false,
      claudeConfigDirectory: null,
      claudeConfigDirectoryAvailable: false,
    },
  ];
}

function emptyCodexAccounts(): CodexAccountsStateBridge {
  return { accounts: [], accountOrdinals: {}, snapshots: {} };
}

const DEFAULT_ENABLED = ["codex", "claude", "cursor", "gemini", "copilot"];

function defaultScenario(now: Date): ProofScenario {
  return {
    name: "default",
    settings: baseSettings({ enabledProviders: DEFAULT_ENABLED }),
    catalog: PROOF_PROVIDER_CATALOG,
    providers: [
      codexSnapshot(now, "dev@example.com"),
      claudeSnapshot(now),
      cursorSnapshot(now),
      geminiSnapshot(now),
      copilotSnapshot(now),
    ],
    codexAccounts: singleCodexAccount(now),
    claudeAccounts: claudeAccounts(now),
    localSpend: [codexSpend(), claudeSpend()],
    claudeTokenAccounts: claudeTokenAccounts(),
  };
}

function twoCodexAccountsScenario(now: Date): ProofScenario {
  const work = "work@example.com";
  const personal = "personal@example.com";
  return {
    name: "two-codex-accounts",
    settings: baseSettings({ enabledProviders: ["codex", "claude"] }),
    catalog: PROOF_PROVIDER_CATALOG,
    providers: [codexSnapshot(now, work), claudeSnapshot(now)],
    codexAccounts: {
      accounts: [
        codexAccount(now, "proof-codex-work", work, "ambient", "Work"),
        codexAccount(now, "proof-codex-personal", personal, "managedByApp", "Personal"),
      ],
      accountOrdinals: { "proof-codex-work": 1, "proof-codex-personal": 2 },
      snapshots: {
        "proof-codex-work": codexAccountUsage(now, work, "Plus", 34, 58),
        "proof-codex-personal": codexAccountUsage(now, personal, "Pro", 9, 17),
      },
    },
    claudeAccounts: claudeAccounts(now),
    localSpend: [codexSpend(), claudeSpend()],
  };
}

/**
 * Overview with "Show all accounts in Overview" on: Codex expands to one row per
 * cached managed account, and Claude to one row per saved token account. Only
 * the Work token account has cached usage, so Personal shows "No usage data".
 * The private variant lists Personal first although its id sorts second, so
 * "Account N" must follow the row order.
 */
function overviewAllAccountsScenario(now: Date, hidePersonalInfo: boolean): ProofScenario {
  const scenario = twoCodexAccountsScenario(now);
  return {
    ...scenario,
    name: hidePersonalInfo ? "overview-all-accounts-private" : "overview-all-accounts",
    settings: baseSettings({
      enabledProviders: ["codex", "claude"],
      mergedOverviewAllAccounts: true,
      hidePersonalInfo,
    }),
    claudeTokenAccounts: hidePersonalInfo ? claudeTokenAccounts().reverse() : claudeTokenAccounts(),
    claudeTokenAccountUsage: {
      [CLAUDE_WORK_TOKEN_ID]: {
        primary: { usedPercent: 22, windowMinutes: 300, resetsAt: isoAfter(now, 61) },
        secondary: { usedPercent: 41, windowMinutes: 10_080, resetsAt: isoAfter(now, 5 * DAY_MINUTES + 30) },
        updatedAt: isoBefore(now, 2),
      },
    },
  };
}

function errorsScenario(now: Date): ProofScenario {
  const claude = providerSnapshot(
    {
      providerId: "claude",
      displayName: "Claude",
      primary: rateWindow(now, 0, SESSION_MINUTES, null),
      primaryLabel: "Session (5h)",
      updatedAt: isoBefore(now, 1),
    },
    {
      error: "Your Claude session has expired. Sign in again to resume usage updates.",
      errorState: "expiredSession",
      sourceLabel: "oauth",
    },
  );
  return {
    name: "errors",
    settings: baseSettings({ enabledProviders: ["codex", "claude", "cursor"] }),
    catalog: PROOF_PROVIDER_CATALOG,
    providers: [
      codexSnapshot(now, "dev@example.com"),
      claude,
      // Last good data from three hours ago: older than the 10-minute stale cutoff.
      cursorSnapshot(now, 180),
    ],
    codexAccounts: singleCodexAccount(now),
    claudeAccounts: [],
    localSpend: [codexSpend(), claudeSpend()],
  };
}

/** Cursor card on the unified weekly pool, with on-demand spend in the cost section. */
function cursorWeeklyScenario(now: Date): ProofScenario {
  return {
    name: "cursor-weekly",
    settings: baseSettings({ enabledProviders: ["cursor"] }),
    catalog: PROOF_PROVIDER_CATALOG,
    providers: [cursorWeeklySnapshot(now)],
    codexAccounts: emptyCodexAccounts(),
    claudeAccounts: [],
    localSpend: [],
  };
}

function emptyScenario(): ProofScenario {
  return {
    name: "empty",
    settings: baseSettings({ enabledProviders: [] }),
    catalog: PROOF_PROVIDER_CATALOG,
    providers: [],
    codexAccounts: emptyCodexAccounts(),
    claudeAccounts: [],
    localSpend: [],
  };
}

const MINUTE_SECONDS = 60;
const DAY_SECONDS = 86_400;

/**
 * OpenAI API card with both a Tokens/min window and daily spend, so the card
 * offers the Live · Spend switch. The last two minutes are still reporting.
 */
function openAiApiSnapshot(now: Date): ProviderUsageSnapshot {
  const nowSeconds = Math.floor(now.getTime() / 1000);
  const firstMinute = Math.floor(nowSeconds / MINUTE_SECONDS) * MINUTE_SECONDS - 59 * MINUTE_SECONDS;
  const minutes = Array.from({ length: 60 }, (_, i) => {
    const isPending = i >= 58;
    const totalTokens = isPending ? 0 : 6_000 + ((i * 7_919) % 18_000);
    return {
      start: firstMinute + i * MINUTE_SECONDS,
      totalTokens,
      requests: isPending ? 0 : 2 + (i % 5),
      isPending,
    };
  });
  const reported = minutes.filter((minute) => !minute.isPending);
  const peak = reported.reduce((best, minute) => (minute.totalTokens > best.totalTokens ? minute : best));
  const lastFive = reported.slice(-5);
  const firstDay = Math.floor(nowSeconds / DAY_SECONDS) * DAY_SECONDS - 29 * DAY_SECONDS;
  const daily = Array.from({ length: 30 }, (_, i) => {
    const costUsd = Math.round((1.5 + ((i * 37) % 11) * 0.45) * 100) / 100;
    const totalTokens = Math.round(costUsd * 190_000);
    return {
      startTime: firstDay + i * DAY_SECONDS,
      endTime: firstDay + (i + 1) * DAY_SECONDS,
      costUsd,
      requests: 120 + i,
      inputTokens: Math.round(totalTokens * 0.7),
      cachedInputTokens: Math.round(totalTokens * 0.1),
      outputTokens: Math.round(totalTokens * 0.3),
      totalTokens,
      lineItems: [{ name: "Text tokens", costUsd }],
      models: [],
    };
  });
  return providerSnapshot(
    {
      providerId: "openaiapi",
      displayName: "OpenAI API",
      primary: rateWindow(now, 0, null, null),
      primaryLabel: "Spend",
      updatedAt: isoBefore(now, 1),
    },
    {
      planName: "Admin API",
      sourceLabel: "api",
      throughput: {
        minutes,
        frontier: reported[reported.length - 1].start,
        latestTokensPerMinute: reported[reported.length - 1].totalTokens,
        peakTokensPerMinute: peak.totalTokens,
        peakAt: peak.start,
        fiveMinuteAverageTokensPerMinute: Math.round(
          lastFive.reduce((sum, minute) => sum + minute.totalTokens, 0) / lastFive.length,
        ),
        models: [],
        windowMinutes: 60,
        fetchedAt: nowSeconds,
        projectId: null,
      },
      openAiApiUsage: { historyDays: 30, projectId: null, daily },
    },
  );
}

function openAiLiveSpendScenario(now: Date): ProofScenario {
  return {
    name: "openai-live-spend",
    settings: baseSettings({ enabledProviders: ["openaiapi"] }),
    catalog: PROOF_PROVIDER_CATALOG,
    providers: [openAiApiSnapshot(now)],
    codexAccounts: emptyCodexAccounts(),
    claudeAccounts: [],
    localSpend: [],
  };
}

/** Default data with the Claude "Plan renewal / expiry" usage item hidden (`section:subscription`). */
function claudePlanHiddenScenario(now: Date): ProofScenario {
  const base = defaultScenario(now);
  return {
    ...base,
    name: "claude-plan-hidden",
    providers: base.providers.map((snapshot) =>
      snapshot.providerId === "claude"
        ? { ...snapshot, hiddenUsageItemIds: ["section:subscription"] }
        : snapshot,
    ),
  };
}

function toolOperation(
  now: Date,
  index: number,
  overrides: Partial<CodexToolOperation>,
): CodexToolOperation {
  return {
    turnId: "proof-turn-1",
    itemId: `proof-item-${index}`,
    kind: "command",
    name: "shell",
    preview: null,
    completedAt: isoBefore(now, 40 + index * 3),
    outcome: "completed",
    exitCode: 0,
    durationMilliseconds: 420,
    timing: "native",
    needsAttention: false,
    isSlow: false,
    ...overrides,
  };
}

function codexToolActivityOperations(now: Date): CodexToolOperation[] {
  return [
    toolOperation(now, 1, { preview: "pnpm test --run", durationMilliseconds: 48_200, isSlow: true }),
    toolOperation(now, 2, {
      preview: "cargo clippy --all-targets",
      outcome: "nonzeroExit",
      exitCode: 101,
      durationMilliseconds: 12_900,
      needsAttention: true,
      isSlow: true,
    }),
    toolOperation(now, 3, {
      kind: "mcp",
      name: "docs/search",
      preview: "{\"query\":\"proof harness\"}",
      outcome: "toolError",
      exitCode: null,
      durationMilliseconds: 870,
      needsAttention: true,
    }),
    toolOperation(now, 4, {
      kind: "fileChange",
      name: "fileChange",
      exitCode: null,
      durationMilliseconds: 2_400,
      timing: "recordedInterval",
    }),
    toolOperation(now, 5, { preview: "git status --short", durationMilliseconds: 95 }),
    toolOperation(now, 6, {
      kind: "webSearch",
      name: "webSearch",
      outcome: "unknown",
      exitCode: null,
      durationMilliseconds: null,
      timing: null,
    }),
  ];
}

/** Default data plus one Codex conversation with native tool operations (upstream #4363). */
function codexToolActivityScenario(now: Date): ProofScenario {
  const sessionId = "proof-session-tools";
  return {
    ...defaultScenario(now),
    name: "codex-tool-activity",
    codexConversations: [
      {
        id: sessionId,
        projectId: "proof-project",
        displayTitle: "Fix the proof harness",
        cwd: null,
        startedAt: isoBefore(now, 120),
        latestActivity: isoBefore(now, 40),
        totals: { inputTokens: 182_000, cachedInputTokens: 96_000, outputTokens: 14_500, totalTokens: 196_500 },
        costEstimate: { knownUsd: 1.42, unknownTokens: 0 },
        topModel: "gpt-5-codex",
        turnPerformance: null,
      },
    ],
    codexToolActivity: [
      {
        sessionId,
        operations: codexToolActivityOperations(now),
        ignoredRecordCount: 1,
        isPartial: true,
        redacted: false,
      },
    ],
  };
}

/** Default data plus Codex model rows where one model's token counts are unknown, not zero. */
function usageSpendUnknownTokensScenario(now: Date): ProofScenario {
  return {
    ...defaultScenario(now),
    name: "usage-spend-unknown-tokens",
    codexSpendModels: [
      {
        model: "gpt-5-codex",
        costUsd: 1.42,
        inputTokens: 182_000,
        outputTokens: 14_500,
        cacheReadTokens: 96_000,
        totalTokens: 196_500,
        customPricing: false,
      },
      {
        model: "gpt-5-mini",
        costUsd: null,
        inputTokens: null,
        outputTokens: null,
        cacheReadTokens: null,
        totalTokens: null,
        customPricing: false,
      },
    ],
  };
}

/** A synthetic tcr pool read through the claude-swap adapter: two read-only cards, no active account. */
function tcrPoolAccount(
  now: Date,
  slot: number,
  email: string,
  fiveHour: number,
  sevenDay: number,
): ClaudeSwapAccount {
  return {
    id: `tcr:${slot}`,
    source: "tcr",
    slot,
    label: email,
    email,
    organization: null,
    alias: null,
    isActive: false,
    action: null,
    isDisabled: false,
    status: "ok",
    error: null,
    fiveHour: { usedPercent: fiveHour, resetsAt: isoAfter(now, 95) },
    sevenDay: { usedPercent: sevenDay, resetsAt: isoAfter(now, 4 * 24 * 60) },
    scoped: [],
    spend: null,
    historicalUsage: null,
  };
}

function claudeSwapTcrScenario(now: Date): ProofScenario {
  const base = defaultScenario(now);
  // Only pool-a has enough recorded history for a forecast; pool-b stays without one.
  const forecasted = tcrPoolAccount(now, 1, "pool-a@example.com", 42, 18);
  forecasted.sessionEquivalentForecast = {
    estimatedWindowsToExhaustWeekly: 5,
    windowsUntilReset: 12,
    availableWindowsUntilReset: 10,
    sampleCount: 3,
    weeklyResetsAt: isoAfter(now, 4 * 24 * 60),
    weeklyUsedPercent: 18,
  };
  return {
    ...base,
    name: "claude-swap-tcr",
    settings: {
      ...base.settings,
      claudeSwapEnabled: true,
      claudeSwapExecutablePath: "C:\\tools\\tcr.exe",
      claudeSwapSource: "tcr",
    },
    claudeSwapAccounts: {
      enabled: true,
      source: "tcr",
      executableConfigured: true,
      accounts: [forecasted, tcrPoolAccount(now, 2, "pool-b@example.com", 7, 63)],
      error: null,
    },
  };
}

/**
 * Three claude-swap pool accounts with Overview rows on. Slot 1 is active, slot 2
 * has usage, and slot 3 has no cached usage ("No usage data"). The pool is
 * claude-swap, so it stays switchable; the tcr pool fixture covers read-only rows.
 */
function overviewClaudeSwapAccountsScenario(now: Date, hidePersonalInfo: boolean): ProofScenario {
  const base = defaultScenario(now);
  const active: ClaudeSwapAccount = {
    ...tcrPoolAccount(now, 1, "work@example.com", 22, 41),
    id: "claude-swap:1",
    source: "claude-swap",
    isActive: true,
  };
  const second: ClaudeSwapAccount = {
    ...tcrPoolAccount(now, 2, "personal@example.com", 63, 12),
    id: "claude-swap:2",
    source: "claude-swap",
  };
  const unfetched: ClaudeSwapAccount = {
    ...tcrPoolAccount(now, 3, "backup@example.com", 0, 0),
    id: "claude-swap:3",
    source: "claude-swap",
    status: "token_expired",
    fiveHour: null,
    sevenDay: null,
    error: "Re-login required. Re-authenticate this account in claude-swap.",
  };
  return {
    ...base,
    name: hidePersonalInfo
      ? "overview-claude-swap-accounts-private"
      : "overview-claude-swap-accounts",
    settings: {
      ...baseSettings({
        enabledProviders: DEFAULT_ENABLED,
        mergedOverviewAllAccounts: true,
        hidePersonalInfo,
      }),
      claudeSwapEnabled: true,
      claudeSwapExecutablePath: "C:\\tools\\cswap.exe",
      claudeSwapSource: "claude-swap",
    },
    claudeSwapAccounts: {
      enabled: true,
      source: "claude-swap",
      executableConfigured: true,
      accounts: [active, second, unfetched],
      error: null,
    },
    claudeSwapOverviewUsage: {
      "claude-swap:1": {
        primary: { usedPercent: 22, windowMinutes: 300, resetsAt: isoAfter(now, 61) },
        secondary: { usedPercent: 41, windowMinutes: 10_080, resetsAt: isoAfter(now, 5 * DAY_MINUTES + 30) },
        updatedAt: isoBefore(now, 2),
      },
      "claude-swap:2": {
        primary: { usedPercent: 63, windowMinutes: 300, resetsAt: isoAfter(now, 95) },
        secondary: { usedPercent: 12, windowMinutes: 10_080, resetsAt: isoAfter(now, 4 * DAY_MINUTES) },
        updatedAt: isoBefore(now, 9),
      },
    },
  };
}

/** The tcr pool from `claude-swap-tcr` with usage bars set to show what is left. */
function claudeSwapTcrRemainingScenario(now: Date): ProofScenario {
  const base = claudeSwapTcrScenario(now);
  return {
    ...base,
    name: "claude-swap-tcr-remaining",
    settings: { ...base.settings, showAsUsed: false },
  };
}

/** The tcr pool from `claude-swap-tcr` with Overview rows on and cached usage. */
function overviewTcrAccountsScenario(now: Date, hidePersonalInfo: boolean): ProofScenario {
  const base = claudeSwapTcrScenario(now);
  return {
    ...base,
    name: hidePersonalInfo ? "overview-tcr-accounts-private" : "overview-tcr-accounts",
    settings: {
      ...base.settings,
      mergedOverviewAllAccounts: true,
      hidePersonalInfo,
    },
    claudeSwapOverviewUsage: {
      "tcr:1": {
        primary: { usedPercent: 42, windowMinutes: 300, resetsAt: isoAfter(now, 61) },
        secondary: { usedPercent: 18, windowMinutes: 10_080, resetsAt: isoAfter(now, 4 * DAY_MINUTES) },
        updatedAt: isoBefore(now, 2),
      },
      "tcr:2": {
        primary: { usedPercent: 7, windowMinutes: 300, resetsAt: isoAfter(now, 95) },
        secondary: { usedPercent: 63, windowMinutes: 10_080, resetsAt: isoAfter(now, 4 * DAY_MINUTES) },
        updatedAt: isoBefore(now, 9),
      },
    },
  };
}

function codexConversation(
  now: Date,
  id: string,
  displayTitle: string,
  minutesAgo: number,
  knownUsd: number,
  unknownTokens = 0,
): CodexWorkspacesSessionUsage {
  return {
    id,
    projectId: "proof-project",
    displayTitle,
    cwd: null,
    startedAt: isoBefore(now, minutesAgo + 30),
    latestActivity: isoBefore(now, minutesAgo),
    totals: { inputTokens: 184_000, cachedInputTokens: 120_000, outputTokens: 9_400, totalTokens: 193_400 },
    costEstimate: { knownUsd, unknownTokens },
    topModel: "gpt-5-codex",
    turnPerformance: null,
  };
}

/** Default data plus three synthetic Codex conversations for the tray card list. */
function codexConversationsScenario(now: Date): ProofScenario {
  return {
    ...defaultScenario(now),
    name: "codex-conversations",
    codexConversations: [
      codexConversation(now, "019f0000-0000-7000-8000-00000000a001", "Port the tray conversations list", 600, 3.12),
      codexConversation(now, "019f0000-0000-7000-8000-00000000a002", "Session 019f...0000a002", 45, 0.87, 2_000),
      codexConversation(now, "019f0000-0000-7000-8000-00000000a003", "Fix the parser fixture", 5, 1.46),
    ],
  };
}

function proofTurn(
  turnId: string | null,
  firstActivity: string | null,
  requests: [string, number, number, number, number | null][],
): CodexSessionTurn {
  const rows = requests.map(([timestamp, input, cached, output, costUsd], index) => ({
    id: `request-${index}`,
    modelName: "gpt-5-codex",
    timestamp,
    inputTokens: input,
    cachedInputTokens: cached,
    outputTokens: output,
    reasoningTokens: Math.round(output / 3),
    totalTokens: input + output,
    costUsd,
  }));
  const sum = (pick: (row: (typeof rows)[number]) => number) => rows.reduce((total, row) => total + pick(row), 0);
  const priced = rows.every((row) => row.costUsd !== null);
  return {
    turnId,
    firstActivity,
    lastActivity: turnId === null ? null : (rows[rows.length - 1]?.timestamp ?? null),
    inputTokens: sum((row) => row.inputTokens),
    cachedInputTokens: sum((row) => row.cachedInputTokens),
    outputTokens: sum((row) => row.outputTokens),
    reasoningTokens: sum((row) => row.reasoningTokens),
    totalTokens: sum((row) => row.totalTokens),
    requestCount: rows.length,
    costUsd: priced ? sum((row) => row.costUsd ?? 0) : null,
    requests: rows,
  };
}

/** Three conversations for the Sessions inspector: one with request details, one over the limit, one unavailable. */
function codexSessionInspectorScenario(now: Date): ProofScenario {
  const detailed = "019f0000-0000-7000-8000-00000000b001";
  const large = "019f0000-0000-7000-8000-00000000b002";
  const stale = "019f0000-0000-7000-8000-00000000b003";
  return {
    ...defaultScenario(now),
    name: "codex-session-inspector",
    codexConversations: [
      codexConversation(now, detailed, "Port the session inspector", 15, 0.4213),
      codexConversation(now, large, "Index the archived rollouts", 240, 7.5),
      codexConversation(now, stale, "Review the parser fixture", 60, 1.2),
    ],
    codexSessionDetails: [
      {
        sessionId: detailed,
        requestCount: 4,
        turns: [
          proofTurn("turn-one", isoBefore(now, 30), [
            [isoBefore(now, 30), 60_000, 40_000, 3_000, 0.12],
            [isoBefore(now, 29), 64_000, 48_000, 2_400, 0.1013],
          ]),
          proofTurn("turn-two", isoBefore(now, 20), [[isoBefore(now, 20), 50_000, 30_000, 3_200, 0.15]]),
          proofTurn(null, null, [[isoBefore(now, 16), 10_000, 2_000, 800, 0.05]]),
        ],
      },
    ],
    codexSessionDetailErrors: { [large]: "limitExceeded" },
  };
}

/** The same conversations with Hide personal information on. */
function codexConversationsPrivateScenario(now: Date): ProofScenario {
  const base = codexConversationsScenario(now);
  return {
    ...base,
    name: "codex-conversations-private",
    settings: { ...base.settings, hidePersonalInfo: true },
  };
}

/**
 * Default data with a saved Claude limit-reset credit (`reset-credits` inventory
 * item, as the OAuth and Web sources produce). `hidden` adds the Claude
 * "Limit reset credits" section to `hiddenUsageItemIds`.
 */
function claudeResetCreditsScenario(now: Date, hidden: boolean): ProofScenario {
  const base = defaultScenario(now);
  return {
    ...base,
    name: hidden ? "claude-reset-credits-hidden" : "claude-reset-credits",
    providers: base.providers.map((snapshot) =>
      snapshot.providerId === "claude"
        ? {
            ...snapshot,
            inventory: [
              {
                id: "reset-credits",
                title: "Limit Reset Credits",
                availableCount: 2,
                nextExpiresAt: isoAfter(now, 3 * DAY_MINUTES),
              },
            ],
            hiddenUsageItemIds: hidden ? ["section:claude-reset-credits"] : [],
          }
        : snapshot,
    ),
  };
}

/**
 * Default data plus an estimated current quota window for Claude. The inline
 * dashboard then shows the "Est. ..." current-window KPI titles in the narrow
 * two-column grid. The title is replaced with a long synthetic string, as a
 * long translation would be, so the proof can check that KPI titles wrap.
 */
function kpiLongTitlesScenario(now: Date): ProofScenario {
  const base = defaultScenario(now);
  return {
    ...base,
    name: "kpi-long-titles",
    localeOverrides: {
      PanelCurrentWindowTokens: "aktuelles Zeitfenster, Tokens insgesamt (geschaetzt)",
    },
    quotaWindowHistory: [
      {
        providerId: "claude",
        accountScope: null,
        historyCoverageEstablished: true,
        windows: [
          {
            offset: 0,
            start: isoBefore(now, 2 * 60),
            end: isoAfter(now, 3 * 60),
            totalTokens: 184_000,
            totalCostUsd: 1.42,
            tokensAreComplete: true,
            costIsComplete: true,
            entryCount: 9,
            boundariesAreEstimated: true,
          },
        ],
      },
    ],
  };
}

/**
 * Default data with Claude's session window marked as regenerating, shown under
 * the shipped uk-UA strings. The reset is 2 days 3 hours 30 minutes away; the
 * countdown rounds up to whole minutes, so it reads "2 дн. 3 год" (uk-UA) and
 * "Regenerates in 2d 3h" (en-US) for the first few minutes after load.
 */
function ukRegenCountdownScenario(now: Date): ProofScenario {
  const base = defaultScenario(now);
  const resetsAt = isoAfter(now, 2 * DAY_MINUTES + 3 * 60 + 30);
  return {
    ...base,
    name: "uk-regen-countdown",
    localeOverrides: ukLocaleOverrides([
      "RegeneratesCountdown",
      "RegeneratesNow",
      "DurationDaysHours",
      "DurationDaysMinutes",
      "DurationDays",
      "DurationHoursMinutes",
      "DurationHours",
      "DurationMinutes",
    ]),
    providers: base.providers.map((snap) => {
      if (snap.providerId !== "claude") return snap;
      const primary = { ...snap.primary, resetsAt, resetRegenerates: true };
      return { ...snap, primary, selectedMetric: primary };
    }),
  };
}

function claudePlatformScenario(now: Date): ProofScenario {
  return {
    name: "claudeplatform",
    settings: baseSettings({ enabledProviders: ["claudeplatform"] }),
    catalog: PROOF_PROVIDER_CATALOG,
    providers: [claudePlatformSnapshot(now)],
    codexAccounts: emptyCodexAccounts(),
    claudeAccounts: [],
    localSpend: [],
  };
}

/** Synthetic personal-budget card and settings pane for the opt-in Vercel budget. */
function vercelScenario(now: Date): ProofScenario {
  return {
    name: "vercel",
    settings: baseSettings({ enabledProviders: ["vercel"] }),
    catalog: PROOF_PROVIDER_CATALOG,
    providers: [vercelSnapshot(now)],
    codexAccounts: emptyCodexAccounts(),
    claudeAccounts: [],
    localSpend: [],
  };
}

/** Vercel with a saved Team ID whose next save the shell rejects as invalid. */
function vercelTeamIdInvalidScenario(now: Date): ProofScenario {
  return {
    ...vercelScenario(now),
    name: "vercel-team-id-invalid",
    workspaceIds: { vercel: "team_proof" },
    workspaceIdSaveErrors: { vercel: "Vercel Team ID is invalid" },
  };
}

/**
 * Default data on a machine with no importable browser, so the cookie section
 * shows its empty-state hint beside the paste box.
 */
function cookieNoBrowsersScenario(now: Date): ProofScenario {
  return {
    ...defaultScenario(now),
    name: "cookie-no-browsers",
    detectedBrowsers: [],
  };
}

/** Kimi on a lapsed plan, showing the "Bonus credits" rows beside the membership pool. */
function kimiBonusCreditsScenario(now: Date): ProofScenario {
  return {
    name: "kimi-bonus-credits",
    settings: baseSettings({ enabledProviders: ["kimi"] }),
    catalog: PROOF_PROVIDER_CATALOG,
    providers: [kimiSnapshot(now)],
    codexAccounts: emptyCodexAccounts(),
    claudeAccounts: [],
    localSpend: [],
  };
}

/** A monthly allowance with a daily target and no pace: 52% left over 18 days. */
function copilotDailyTargetScenario(now: Date): ProofScenario {
  const snapshot = copilotSnapshot(now);
  return {
    name: "copilot-daily-target",
    settings: baseSettings({ enabledProviders: ["copilot"] }),
    catalog: PROOF_PROVIDER_CATALOG,
    providers: [
      { ...snapshot, primary: { ...snapshot.primary, pace: null, dailyTarget: { amount: 52 / 18, unit: "%" } } },
    ],
    codexAccounts: emptyCodexAccounts(),
    claudeAccounts: [],
    localSpend: [],
  };
}

function proofAgentSession(
  id: string,
  host: string,
  provider: AgentSession["provider"],
  projectName: string,
): AgentSession {
  return {
    id,
    provider,
    source: "cli",
    state: "active",
    pid: null,
    transcriptPath: null,
    host,
    workspace: { cwd: null, projectName },
    activity: { startedAt: null, lastActivityAt: null },
    focusTarget: { kind: "none" },
  };
}

/** Two reachable hosts with sessions and one unreachable host; all names are synthetic. */
function agentSessionsHostsScenario(now: Date): ProofScenario {
  const base = defaultScenario(now);
  return {
    ...base,
    name: "agent-sessions-hosts",
    settings: { ...base.settings, agentSessionsEnabled: true, agentSessionsLocalOnly: false },
    agentSessions: {
      status: "hosts",
      hosts: [
        {
          host: "WORKSTATION-A",
          sessions: [
            proofAgentSession("proof-1", "WORKSTATION-A", "codex", "example-project"),
            proofAgentSession("proof-2", "WORKSTATION-A", "claude", "example-notes"),
          ],
          error: null,
        },
        {
          host: "BUILD-BOX",
          sessions: [proofAgentSession("proof-3", "BUILD-BOX", "claude", "example-ci")],
          error: null,
        },
        { host: "LAB-HOST", sessions: [], error: "ssh: connection timed out" },
      ],
    },
  };
}

const BUILDERS: Record<ProofScenarioName, (now: Date) => ProofScenario> = {
  default: defaultScenario,
  "two-codex-accounts": twoCodexAccountsScenario,
  "overview-all-accounts": (now) => overviewAllAccountsScenario(now, false),
  "overview-all-accounts-private": (now) => overviewAllAccountsScenario(now, true),
  errors: errorsScenario,
  empty: emptyScenario,
  "cursor-weekly": cursorWeeklyScenario,
  "openai-live-spend": openAiLiveSpendScenario,
  "claude-plan-hidden": claudePlanHiddenScenario,
  "codex-tool-activity": codexToolActivityScenario,
  "usage-spend-unknown-tokens": usageSpendUnknownTokensScenario,
  "claude-swap-tcr": claudeSwapTcrScenario,
  "claude-swap-tcr-remaining": claudeSwapTcrRemainingScenario,
  "overview-claude-swap-accounts": (now) => overviewClaudeSwapAccountsScenario(now, false),
  "overview-claude-swap-accounts-private": (now) => overviewClaudeSwapAccountsScenario(now, true),
  "overview-tcr-accounts": (now) => overviewTcrAccountsScenario(now, false),
  "overview-tcr-accounts-private": (now) => overviewTcrAccountsScenario(now, true),
  "codex-conversations": codexConversationsScenario,
  "codex-conversations-private": codexConversationsPrivateScenario,
  "codex-session-inspector": codexSessionInspectorScenario,
  "claude-reset-credits": (now) => claudeResetCreditsScenario(now, false),
  "claude-reset-credits-hidden": (now) => claudeResetCreditsScenario(now, true),
  "kpi-long-titles": kpiLongTitlesScenario,
  "uk-regen-countdown": ukRegenCountdownScenario,
  claudeplatform: claudePlatformScenario,
  "cookie-no-browsers": cookieNoBrowsersScenario,
  vercel: vercelScenario,
  "vercel-team-id-invalid": vercelTeamIdInvalidScenario,
  "kimi-bonus-credits": kimiBonusCreditsScenario,
  "agent-sessions-hosts": agentSessionsHostsScenario,
  "copilot-daily-target": copilotDailyTargetScenario,
};

export function isProofScenarioName(value: string): value is ProofScenarioName {
  return (PROOF_SCENARIO_NAMES as readonly string[]).includes(value);
}

/** Fixture timestamps are relative to `now` so reset countdowns stay plausible. */
export function buildProofScenario(name: ProofScenarioName, now: Date = new Date()): ProofScenario {
  return BUILDERS[name](now);
}
