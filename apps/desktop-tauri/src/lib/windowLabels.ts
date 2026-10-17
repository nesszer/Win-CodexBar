import type { LocaleKey } from "../i18n/keys";

/** Localize raw provider window labels using the active locale. */
export function localizeWindowLabel(
  raw: string | undefined,
  t: (key: LocaleKey) => string,
  language?: string,
  windowMinutes?: number | null,
  windowId?: string,
): string {
  const normalized = raw?.trim().toLowerCase();
  if (windowId?.startsWith("claude-weekly-scoped-")) {
    const modelName = raw?.trim().replace(/\s+only\s*$/i, "").trim();
    const template = t("ClaudeScopedWeeklyLabel");
    return modelName ? template.replace("{}", modelName) : template.replace("{}", "");
  }
  // Upstream 0.55.0 #3070: quota windows in Simplified Chinese use their
  // actual duration instead of the conversational Session wording.
  if (language === "chinese" && normalized === "session" && windowMinutes != null) {
    if (windowMinutes === 7 * 24 * 60) return t("ProviderWeeklyLabel");
    if (windowMinutes >= 60 && windowMinutes <= 12 * 60 && windowMinutes % 60 === 0) {
      return `${windowMinutes / 60} 小时`;
    }
  }
  if (normalized === "session") {
    return t("ProviderSessionLabel");
  }
  if (normalized === "weekly") {
    return t("ProviderWeeklyLabel");
  }
  // F5 (upstream 0.48.0): monthly (30-day) window label.
  if (normalized === "monthly") {
    return t("ProviderMonthly");
  }
  return localizeProviderLabel(raw, t);
}

/**
 * Generic English labels that providers emit for windows and detail rows.
 * Brand, model and plan names are not listed, so they pass through unchanged.
 */
const PROVIDER_LABEL_KEYS: Record<string, LocaleKey> = {
  credits: "CreditsLabel",
  balance: "DetailCostBalance",
  spend: "UsageSpendSpend",
  usage: "ProviderUsage",
  requests: "OpenAIChartRequests",
  tokens: "OpenAIChartMetricTokens",
  quota: "WindowLabelQuota",
  daily: "WindowLabelDaily",
  budget: "WindowLabelBudget",
  "weekly quota": "WindowLabelWeeklyQuota",
  "monthly quota": "WindowLabelMonthlyQuota",
  status: "WindowLabelStatus",
  "shared pool": "WindowLabelSharedPool",
  reviews: "WindowLabelReviews",
  premium: "WindowLabelPremium",
  "premium weekly": "WindowLabelPremiumWeekly",
  plan: "WindowLabelPlan",
  personal: "WindowLabelPersonal",
  models: "WindowLabelModels",
  billing: "WindowLabelBilling",
  "api key limit": "WindowLabelApiKeyLimit",
  "weekly cost": "WindowLabelWeeklyCost",
  "daily cost": "WindowLabelDailyCost",
  cost: "WindowLabelCost",
  voices: "WindowLabelVoices",
  "team budget": "WindowLabelTeamBudget",
  "personal budget": "WindowLabelPersonalBudget",
  "secondary budget": "WindowLabelSecondaryBudget",
  subscription: "WindowLabelSubscription",
  standard: "WindowLabelStandard",
  "shared credits": "WindowLabelSharedCredits",
  "plan credits": "WindowLabelPlanCredits",
  "monthly credits": "WindowLabelMonthlyCredits",
  "bonus credits": "WindowLabelBonusCredits",
  "add-on credits": "WindowLabelAddOnCredits",
  savings: "WindowLabelSavings",
  rolling: "WindowLabelRolling",
  refresh: "WindowLabelRefresh",
  "rate limit": "WindowLabelRateLimit",
  points: "WindowLabelPoints",
  packages: "WindowLabelPackages",
  overage: "WindowLabelOverage",
  "on-demand": "WindowLabelOnDemand",
  memory: "WindowLabelMemory",
  "key allowance": "WindowLabelKeyAllowance",
  edits: "WindowLabelEdits",
  deployment: "WindowLabelDeployment",
  "daily free tokens": "WindowLabelDailyFreeTokens",
  cycle: "WindowLabelCycle",
  chat: "WindowLabelChat",
  cash: "WindowLabelCash",
  base: "WindowLabelBase",
  "agent usage": "WindowLabelAgentUsage",
  version: "WindowLabelVersion",
  project: "WindowLabelProject",
  key: "WindowLabelKey",
  email: "WindowLabelEmail",
  account: "WindowLabelAccount",
  "usage (last 7 days)": "WindowLabelUsageLast7Days",
  monthly: "ProviderLabelMonthly",
  hourly: "ProviderLabelHourly",
  quarterly: "ProviderLabelQuarterly",
  yearly: "ProviderLabelYearly",
  lifetime: "ProviderLabelLifetime",
  today: "ProviderLabelToday",
  total: "ProviderLabelTotal",
  expires: "ProviderLabelExpires",
  renews: "ProviderLabelRenews",
  "tokens today": "ProviderLabelTokensToday",
  "requests today": "ProviderLabelRequestsToday",
  "tokens (month)": "ProviderLabelTokensMonth",
  "requests (month)": "ProviderLabelRequestsMonth",
  "tokens (30 days)": "ProviderLabelTokens30Days",
  "spend today": "ProviderLabelSpendToday",
  "spend (30 days)": "ProviderLabelSpend30Days",
  "daily spend": "ProviderLabelDailySpend",
  "weekly spend": "ProviderLabelWeeklySpend",
  "monthly spend": "ProviderLabelMonthlySpend",
  "limit reset credits": "ProviderLabelLimitResetCredits",
  "cloud credits": "ProviderLabelCloudCredits",
  "reset credits": "ProviderLabelResetCredits",
  "additional credits": "ProviderLabelAdditionalCredits",
  "overage usage": "ProviderLabelOverageUsage",
  "overage cost": "ProviderLabelOverageCost",
  "total usage": "ProviderLabelTotalUsage",
  "orb usage": "ProviderLabelOrbUsage",
  "voice slots": "ProviderLabelVoiceSlots",
  "professional voices": "ProviderLabelProfessionalVoices",
  "voucher balance": "ProviderLabelVoucherBalance",
  "cash balance": "ProviderLabelCashBalance",
  "flex credits": "ProviderLabelFlexCredits",
  "event credits": "ProviderLabelEventCredits",
  "monthly plan": "ProviderLabelMonthlyPlan",
  "daily limit": "ProviderLabelDailyLimit",
  uncollected: "ProviderLabelUncollected",
  "key inactive": "ProviderLabelKeyInactive",
  "credits used": "ProviderLabelCreditsUsed",
  "ai credits": "ProviderLabelAiCredits",
  completions: "ProviderLabelCompletions",
  "additional budget": "ProviderLabelAdditionalBudget",
  "top model": "ProviderLabelTopModel",
  credit: "ProviderLabelCredit",
  user: "ProviderLabelUser",
  team: "ProviderLabelTeam",
  "usage billing": "ProviderLabelUsageBilling",
  "period resets": "ProviderLabelPeriodResets",
  "cycle used": "ProviderLabelCycleUsed",
  "cycle remaining": "ProviderLabelCycleRemaining",
  "all-time key usage": "ProviderLabelAllTimeKeyUsage",
  "key spending limit": "ProviderLabelKeySpendingLimit",
  "your shared usage": "ProviderLabelYourSharedUsage",
  "rest of organization": "ProviderLabelRestOfOrganization",
  reset: "ProviderLabelReset",
  "billable inference usage": "ProviderLabelBillableInferenceUsage",
  "gross inference usage": "ProviderLabelGrossInferenceUsage",
  "included inference amount": "ProviderLabelIncludedInferenceAmount",
  "spending limit": "ProviderLabelSpendingLimit",
  "prepaid balance": "ProviderLabelPrepaidBalance",
  loaded: "ProviderLabelLoaded",
  stored: "ProviderLabelStored",
  "subscription credits": "ProviderLabelSubscriptionCredits",
  "monthly grant": "ProviderLabelMonthlyGrant",
  "rollover credits": "ProviderLabelRolloverCredits",
  "top-up credits": "ProviderLabelTopUpCredits",
  "total usable": "ProviderLabelTotalUsable",
  "credits remaining": "ProviderLabelCreditsRemaining",
  "credits total added": "ProviderLabelCreditsTotalAdded",
  "credits balance": "ProviderLabelCreditsBalance",
  "api key remaining": "ProviderLabelApiKeyRemaining",
  "api key used": "ProviderLabelApiKeyUsed",
  "activity tokens": "ProviderLabelActivityTokens",
  "activity requests": "ProviderLabelActivityRequests",
  "activity models": "ProviderLabelActivityModels",
  "spend history (last 30 days)": "ProviderLabelSpendHistory30Days",
  "reset window": "ProviderLabelResetWindow",
  left: "ProviderLabelLeft",
  "spent this month": "ProviderLabelSpentThisMonth",
  "credit balance": "ProviderLabelCreditBalance",
  "billing remaining": "ProviderLabelBillingRemaining",
  "on-demand balance": "ProviderLabelOnDemandBalance",
  "rate-limit remaining": "ProviderLabelRateLimitRemaining",
  "billing type": "ProviderLabelBillingType",
  scope: "ProviderLabelScope",
  "subscription credits available": "ProviderLabelSubscriptionCreditsAvailable",
  "total credits available": "ProviderLabelTotalCreditsAvailable",
  "used this cycle": "ProviderLabelUsedThisCycle",
  "bank cap": "ProviderLabelBankCap",
  "next refill": "ProviderLabelNextRefill",
  "lifetime spend": "ProviderLabelLifetimeSpend",
  "tokens used today": "ProviderLabelTokensUsedToday",
  "daily allowance": "ProviderLabelDailyAllowance",
  "tokens remaining": "ProviderLabelTokensRemaining",
  "daily reset": "ProviderLabelDailyReset",
  "additional quota": "ProviderLabelAdditionalQuota",
  "token spend": "ProviderLabelTokenSpend",
  "spend limit": "ProviderLabelSpendLimit",
  "remaining budget": "ProviderLabelRemainingBudget",
  "pay-as-you-go balance": "ProviderLabelPayAsYouGoBalance",
  observed: "ProviderLabelObserved",
  "applicable budgets": "ProviderLabelApplicableBudgets",
  "requests (last 7 days)": "ProviderLabelRequestsLast7Days",
  "tokens (last 7 days)": "ProviderLabelTokensLast7Days",
  "attributed spend (last 7 days)": "ProviderLabelAttributedSpendLast7Days",
  "cost coverage (last 7 days)": "ProviderLabelCostCoverageLast7Days",
  default: "ProviderLabelDefault",
  "other models": "ProviderLabelOtherModels",
  "daily routines": "ProviderLabelDailyRoutines",
  "oauth apps": "ProviderLabelOAuthApps",
  "extra usage": "ProviderLabelExtraUsage",
  "extra usage balance": "ProviderLabelExtraUsageBalance",
  pass: "ProviderLabelPass",
  gateway: "ProviderLabelGateway",
  unknown: "ProviderLabelUnknown",
  custom: "ProviderLabelCustom",
  "live usage": "ProviderLabelLiveUsage",
  "browser teams": "ProviderLabelBrowserTeams",
  "input tokens": "ProviderLabelInputTokens",
  "output tokens": "ProviderLabelOutputTokens",
};

function fill(template: string, ...values: string[]): string {
  return values.reduce<string>((text, value) => text.replace("{}", value), template);
}

/** Exact generic labels and numbered window shapes; null when nothing matches. */
function genericLabel(trimmed: string, t: (key: LocaleKey) => string): string | null {
  const normalized = trimmed.toLowerCase();
  const key = PROVIDER_LABEL_KEYS[normalized];
  if (key) return t(key);
  if (normalized === "session") return t("ProviderSessionLabel");
  if (normalized === "weekly") return t("ProviderWeeklyLabel");
  const session = /^session \((\d+h)\)$/.exec(normalized);
  if (session) return `${t("ProviderSessionLabel")} (${session[1]})`;
  const hours = /^(\d+)(?:-| )hours?$/.exec(normalized);
  if (hours) return fill(t("WindowLabelHours"), hours[1]);
  const hourQuota = /^(\d+)-hour quota$/.exec(normalized);
  if (hourQuota) return fill(t("WindowLabelHourQuota"), hourQuota[1]);
  const days = /^(\d+)-day$/.exec(normalized);
  if (days) return fill(t("WindowLabelDays"), days[1]);
  const hourLimit = /^(\d+) hour limit$/.exec(normalized);
  if (hourLimit) return fill(t("ProviderLabelHourLimit"), hourLimit[1]);
  const dayLimit = /^(\d+) day limit$/.exec(normalized);
  if (dayLimit) return fill(t("ProviderLabelDayLimit"), dayLimit[1]);
  return null;
}

/** Localize a generic provider label ("Credits", "5-hour", ...); other text passes through. */
export function localizeProviderLabel(
  raw: string | null | undefined,
  t: (key: LocaleKey) => string,
): string {
  const trimmed = raw?.trim() ?? "";
  if (!trimmed) return raw ?? "";
  const localized = translateLabel(trimmed, t);
  // Same words in the UI language (English): keep the provider's casing.
  if (localized == null || localized.toLowerCase() === trimmed.toLowerCase()) return raw ?? "";
  return localized;
}

function translateLabel(trimmed: string, t: (key: LocaleKey) => string): string | null {
  const generic = genericLabel(trimmed, t);
  if (generic != null) return generic;
  const only = /^(.+) only$/i.exec(trimmed);
  if (only) return fill(t("ProviderLabelModelOnly"), only[1]);
  // "Team Acme" names a team; lowercase tails ("Team credits") are not names.
  const team = /^Team ([A-Z0-9].*)$/.exec(trimmed);
  if (team) return fill(t("ProviderLabelTeamNamed"), team[1]);
  const balance = /^(.+) balance$/i.exec(trimmed);
  if (balance) return fill(t("ProviderTextBalanceSuffix"), balance[1]);
  // "Gemini Weekly", "Codex Spark 5-hour": keep the brand, localize the
  // longest generic tail. The head must read as a name (capitalized words, no
  // punctuation), and a head that is itself generic is not a brand.
  const words = trimmed.split(/\s+/);
  for (let i = 1; i < words.length; i += 1) {
    const head = words.slice(0, i).join(" ");
    if (!/^[A-Z][\w.-]*(?: [A-Z0-9][\w.-]*)*$/.test(head) || genericLabel(head, t) != null) continue;
    const rest = words.slice(i).join(" ");
    if (!/^[A-Z0-9]/.test(rest)) continue;
    const tail = genericLabel(rest, t);
    if (tail != null) return `${head} ${tail}`;
  }
  return null;
}
