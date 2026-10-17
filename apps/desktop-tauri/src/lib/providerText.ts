import type { LocaleKey } from "../i18n/keys";
import { localizeProviderLabel } from "./windowLabels";

type Translate = (key: LocaleKey) => string;

/**
 * Provider detail lines arrive in English (cached snapshots stay
 * language-neutral). These tables translate the generic shapes at render
 * time; numbers, currency and provider names are kept verbatim, and text that
 * matches nothing passes through unchanged.
 */
const SENTENCES: Record<string, LocaleKey> = {
  "no quota reported": "ProviderTextNoQuotaReported",
  "no quota data": "ProviderTextNoQuotaData",
  "usage unavailable": "ProviderTextUsageUnavailable",
  "no balance information returned": "ProviderTextNoBalanceInfo",
  unlimited: "ProviderTextUnlimited",
  unavailable: "ProviderTextUnavailable",
  expired: "ProviderTextExpired",
  "overdue invoices": "ProviderTextOverdueInvoices",
  "subscription active": "ProviderTextSubscriptionActive",
  "virtual key is inactive": "ProviderTextVirtualKeyInactive",
  "credit usage unavailable": "ProviderTextCreditUsageUnavailable",
  "no token quota": "ProviderTextNoTokenQuota",
  "token quota unavailable": "ProviderTextTokenQuotaUnavailable",
  "no active model quota": "ProviderTextNoActiveModelQuota",
  "billing usage unavailable": "ProviderTextBillingUsageUnavailable",
  "no applicable budgets reported": "ProviderTextNoApplicableBudgets",
  "no active subscription kwh": "ProviderTextNoActiveSubscriptionKwh",
  "no token-plan usage": "ProviderTextNoTokenPlanUsage",
  "no edit predictions included": "ProviderTextNoEditPredictions",
  "daily usage": "ProviderTextDailyUsage",
  "resets daily": "ProviderTextResetsDaily",
  "no active 5h session": "ProviderTextNoActiveSession",
  "key quota": "ProviderTextKeyQuota",
  "account balance": "ProviderTextAccountBalance",
  "monthly budget": "ProviderTextMonthlyBudget",
  "api spend": "ProviderTextApiSpend",
  offline: "ProviderTextOffline",
  "not reported": "ProviderTextNotReported",
  "no limit configured": "ProviderTextNoLimitConfigured",
  "unavailable right now": "ProviderTextUnavailableRightNow",
  "management api key not configured": "ProviderTextManagementKeyNotConfigured",
  "management api key required": "ProviderTextManagementKeyRequired",
  "response was invalid": "ProviderTextResponseInvalid",
  "request timed out": "ProviderTextRequestTimedOut",
  "request failed": "ProviderTextRequestFailed",
  "last 30 completed utc days": "ProviderTextLast30CompletedUtcDays",
  "not included in this login response": "ProviderTextNotIncludedInLogin",
  "no cap reported": "ProviderTextNoCapReported",
  "no daily cap reported": "ProviderTextNoDailyCapReported",
  "daily free-token usage unavailable": "ProviderTextDailyFreeTokenUnavailable",
  "no priced requests": "ProviderTextNoPricedRequests",
  "budget balance unavailable": "ProviderTextBudgetBalanceUnavailable",
  "balance unavailable for api calls": "ProviderTextBalanceUnavailableForApi",
  "account balance unavailable": "ProviderTextAccountBalanceUnavailable",
  "no recurring credits": "ProviderTextNoRecurringCredits",
  "no credit balance reported": "ProviderTextNoCreditBalanceReported",
  "no weekly quota reported": "ProviderTextNoWeeklyQuotaReported",
  "no budget set": "ProviderTextNoBudgetSet",
  "no allowance": "ProviderTextNoAllowance",
  "memory limit not reported": "ProviderTextMemoryLimitNotReported",
  "subscription active; quota was not included in this login response":
    "ProviderTextSubscriptionQuotaNotIncluded",
  "no plan credit allowance reported": "ProviderTextNoPlanCreditAllowance",
  "active - check dashboard for details": "ProviderTextActiveCheckDashboard",
  "spending cap, not balance": "ProviderTextSpendingCapNotBalance",
  "total key usage": "ProviderTextTotalKeyUsage",
  "total account usage": "ProviderTextTotalAccountUsage",
  "team credits": "ProviderTextTeamCredits",
  "no usage this month": "ProviderTextNoUsageThisMonth",
  "cycle ended": "ProviderTextCycleEnded",
  "resets now": "ProviderTextResetsNow",
  "last 30 days": "ProviderTextLast30Days",
  "current month": "ProviderTextCurrentMonth",
  "billing cycle": "ProviderTextBillingCycle",
  "current billing period": "ProviderTextCurrentBillingPeriod",
  "this month (api key)": "ProviderTextThisMonthApiKey",
  "last 30 days (utc)": "ProviderTextLast30DaysUtc",
  "last 7 days · attributed": "ProviderTextLast7DaysAttributed",
  "last 7 days · this key": "ProviderTextLast7DaysThisKey",
  "prepaid credits": "ProviderTextPrepaidCredits",
  "api credits": "ProviderTextApiCredits",
  "approx. spend": "ProviderTextApproxSpend",
  "on-demand (billing cycle)": "ProviderTextOnDemandBillingCycle",
  suspended: "ProviderTextSuspended",
};

/** Case-sensitive sentences: "today" trails a value, "Today" is a label. */
const EXACT: Record<string, LocaleKey> = {
  today: "ProviderTextTodayLower",
  "this week": "ProviderTextThisWeek",
  "this month": "ProviderTextThisMonthLower",
  "This month": "ProviderTextThisMonth",
};

/** "<value> <unit phrase>" lines, keyed by the lowercase unit phrase. */
const UNITS: Record<string, LocaleKey> = {
  requests: "ProviderTextRequests",
  tokens: "ProviderTextTokens",
  credits: "ProviderTextCredits",
  "weekly credits": "ProviderTextWeeklyCredits",
  "add-on credits": "ProviderTextAddOnCredits",
  "event credits": "ProviderTextEventCredits",
  "refresh credits": "ProviderTextRefreshCredits",
  "credits available": "ProviderTextCreditsAvailable",
  units: "ProviderTextUnits",
  predictions: "ProviderTextPredictions",
  flows: "ProviderTextFlows",
  conversations: "ProviderTextConversations",
  available: "ProviderTextAvailable",
  remaining: "ProviderTextRemaining",
  used: "ProviderTextUsed",
  "tokens today": "ProviderTextTokensToday",
  "tokens remaining": "ProviderTextTokensRemaining",
  "spent this month": "ProviderTextSpentThisMonth",
  "monthly grant": "ProviderTextMonthlyGrant",
  "total usable": "ProviderTextTotalUsable",
  "model(s)": "ProviderTextModels",
  "api-rate": "ProviderTextApiRate",
  "credits left": "ProviderTextCreditsLeft",
  "credits total": "ProviderTextCreditsTotal",
  "credits remaining": "ProviderTextCreditsRemaining",
  "credits used": "ProviderTextCreditsUsed",
  "monthly credits remaining": "ProviderTextMonthlyCreditsRemaining",
  "ai credits used": "ProviderTextAiCreditsUsed",
  "minutes used": "ProviderTextMinutesUsed",
  "minutes remaining": "ProviderTextMinutesRemaining",
  "cloud models available": "ProviderTextCloudModelsAvailable",
  successes: "ProviderTextSuccesses",
  prompt: "ProviderTextPrompt",
  completion: "ProviderTextCompletion",
  owed: "ProviderTextOwed",
  "usd remaining": "ProviderTextUsdRemaining",
  spent: "ProviderTextSpent",
  "days active": "ProviderTextDaysActive",
  "days total": "ProviderTextDaysTotal",
  "reset credits available": "ProviderTextResetCreditsAvailable",
  "reset credit available": "ProviderTextResetCreditsAvailable",
  left: "ProviderTextLeft",
  "this month": "ProviderTextThisMonthValue",
  "credit-funded": "ProviderTextCreditFunded",
};

/** "<prefix> <value>" lines, keyed by the lowercase prefix. */
const PREFIXES: Array<[string, LocaleKey]> = [
  ["spent this month: ", "ProviderTextSpentThisMonthPrefix"],
  ["model: ", "ProviderTextModelPrefix"],
  ["cost: ", "ProviderTextCostPrefix"],
  ["method: ", "ProviderTextMethodPrefix"],
  ["category: ", "ProviderTextCategoryPrefix"],
  ["spend: ", "ProviderTextSpendPrefix"],
  ["key valid: ", "ProviderTextKeyValidPrefix"],
  ["monthly refill: ", "ProviderTextMonthlyRefillPrefix"],
  ["suspended: ", "ProviderTextSuspendedPrefix"],
  ["deficit: ", "ProviderTextDeficitPrefix"],
  ["balance: ", "ProviderTextBalanceColonPrefix"],
  ["voucher ", "ProviderTextVoucherPrefix"],
  ["input ", "ProviderTextInputPrefix"],
  ["output ", "ProviderTextOutputPrefix"],
  ["exp. ", "ProviderTextExpPrefix"],
  ["expires ", "ProviderTextExpiresPrefix"],
  ["monthly spend ", "ProviderTextMonthlySpendPrefix"],
  ["deployment: ", "ProviderTextDeploymentPrefix"],
  ["balance ", "ProviderTextBalancePrefix"],
  ["cash ", "ProviderTextCashPrefix"],
  ["ends ", "ProviderTextEnds"],
];

const V = String.raw`[-+]?[$¥€£]?\d[\d,]*(?:\.\d+)?\s?[KMBkmb]?%?`;
const VALUE = new RegExp(String.raw`^(${V}(?:\s*/\s*${V})?)\s+(.+)$`);
const PATTERNS: Array<[RegExp, LocaleKey]> = [
  [new RegExp(String.raw`^(${V}) of (${V}) credits$`, "i"), "ProviderTextOfCredits"],
  [new RegExp(String.raw`^(${V}) of (${V}) remaining$`, "i"), "ProviderTextOfRemaining"],
  [new RegExp(String.raw`^(${V}) tokens over last (\d+) days$`, "i"), "ProviderTextTokensOverLastDays"],
  [new RegExp(String.raw`^(${V}) over last (\d+) days$`, "i"), "ProviderTextOverLastDays"],
  [new RegExp(String.raw`^(${V}) used, (${V}) remaining$`, "i"), "ProviderTextUsedRemaining"],
  [new RegExp(String.raw`^(${V}) total credits \((${V}) free\)$`, "i"), "ProviderTextTotalCreditsFree"],
  [new RegExp(String.raw`^(${V}) used / (\w+)$`, "i"), "ProviderTextUsedPerPeriod"],
  [new RegExp(String.raw`^(${V}) of (${V}) left$`, "i"), "ProviderTextMinutesUsedRemaining"],
  [new RegExp(String.raw`^(${V}\s*/\s*${V}) credits \((${V}) remaining\)$`, "i"), "ProviderTextCreditsWithRemaining"],
  [
    new RegExp(String.raw`^(${V}) input / (${V}) output / (${V}) cached tokens$`, "i"),
    "ProviderTextInputOutputCached",
  ],
  [new RegExp(String.raw`^(${V}) audio hours / (${V}) billable hours$`, "i"), "ProviderTextAudioBillableHours"],
  [new RegExp(String.raw`^(${V}) credits expire on (.+)$`, "i"), "ProviderTextCreditsExpireOn"],
  [
    new RegExp(String.raw`^(.+? \d+d): (${V}) input, (${V}) output tokens, (${V}) requests$`, "i"),
    "ProviderTextActivityTokens",
  ],
  [new RegExp(String.raw`^(${V}) of (${V})$`, "i"), "ProviderTextOf"],
  [/^last (\d+) days$/i, "ProviderTextLastDays"],
  [/^no (.+) api balance available$/i, "ProviderTextNoBrandApiBalance"],
  [/^no (.+) budget quota reported$/i, "ProviderTextNoBrandBudgetQuotaReported"],
  [/^no cached (.+) quota details$/i, "ProviderTextNoCachedBrandQuota"],
  [/^no active (.+) tier allowance$/i, "ProviderTextNoActiveBrandTier"],
  [/^no (.+) quota reported$/i, "ProviderTextNoBrandQuotaReported"],
];

function fill(template: string, values: Array<string | number>): string {
  return values.reduce<string>((text, value) => text.replace("{}", String(value)), template);
}

/** Localize an English duration such as "2h 5m", "3d", "4 days" or "2 months"; null otherwise. */
export function localizeDuration(body: string, t: Translate): string | null {
  const lower = body.trim().toLowerCase();
  let m = /^(\d+)d (\d+)h$/.exec(lower);
  if (m) return fill(t("DurationDaysHours"), [m[1], m[2]]);
  m = /^(\d+)h (\d+)m$/.exec(lower);
  if (m) return fill(t("DurationHoursMinutes"), [m[1], m[2]]);
  m = /^(\d+)(?:d| days?)$/.exec(lower);
  if (m) return fill(t("DurationDays"), [m[1]]);
  m = /^(\d+)(?:h| hours?)$/.exec(lower);
  if (m) return fill(t("DurationHours"), [m[1]]);
  m = /^(\d+)(?:m| minutes?)$/.exec(lower);
  if (m) return fill(t("DurationMinutes"), [m[1]]);
  m = /^(\d+) months?$/.exec(lower);
  if (m) return fill(t("DurationMonths"), [m[1]]);
  return null;
}

/** Localize the countdown body of "Resets in …" ("2h 5m", "11m", "3 days"). */
export function localizeResetCountdown(body: string, t: Translate): string {
  const lower = body.toLowerCase();
  let m = /^(\d+)d (\d+)h$/.exec(lower);
  if (m) return fill(t("ResetsInDaysHours"), [m[1], m[2]]);
  m = /^(\d+)h (\d+)m$/.exec(lower);
  if (m) return fill(t("ResetsInHoursMinutes"), [m[1], m[2]]);
  m = /^(\d+)(?:m| minutes?)$/.exec(lower);
  if (m) return fill(t("ResetsInMinutes"), [m[1]]);
  m = /^(\d+)(?:h| hours?)$/.exec(lower);
  if (m) return fill(t("ResetsInHoursOnly"), [m[1]]);
  m = /^(\d+)(?:d| days?)$/.exec(lower);
  if (m) return fill(t("ResetsInDaysOnly"), [m[1]]);
  m = /^(\d+) seconds?$/.exec(lower);
  if (m) return fill(t("ResetsInMinutes"), [Math.max(1, Math.ceil(Number(m[1]) / 60))]);
  return fill(t("TrayResetsInLabel"), [localizeDuration(body, t) ?? body]);
}

const COUNTDOWNS: Array<[string, LocaleKey]> = [
  ["renews in ", "ProviderTextRenewsIn"],
  ["expires in ", "ProviderTextExpiresIn"],
  ["cycle ends in ", "ProviderTextCycleEndsIn"],
];

const MONTHS = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
const DATE =
  /\b(Jan|Feb|Mar|Apr|May|Jun|Jul|Aug|Sep|Oct|Nov|Dec)[a-z]* (\d{1,2})(?:, (\d{4}))?(?: at (\d{1,2}):(\d{2}) ?([AP]M))?(?=$|[^\w:])/g;

/**
 * Reformat English "Jan 5", "Jan 5, 2026" and "Jan 5 at 3:00 PM" dates in
 * the UI locale. The parts are formatted in UTC so the wall-clock value the
 * provider wrote is kept; trailing zone text ("UTC") stays as written.
 */
export function localizeDates(text: string, t: Translate): string {
  const locale = t("IntlLocale");
  if (locale === "en-US" || !/^[a-z]{2}-[A-Z]{2}$/.test(locale)) return text;
  return text.replace(DATE, (match, mon: string, day: string, year?: string, h?: string, min?: string, ampm?: string) => {
    const month = MONTHS.indexOf(mon.toLowerCase());
    let hour = h ? Number(h) % 12 : 0;
    if (ampm === "PM") hour += 12;
    // 2024 is a leap year, so a yearless "Feb 29" still formats.
    const date = new Date(Date.UTC(year ? Number(year) : 2024, month, Number(day), hour, min ? Number(min) : 0));
    if (date.getUTCDate() !== Number(day)) return match;
    try {
      return new Intl.DateTimeFormat(locale, {
        timeZone: "UTC",
        month: "short",
        day: "numeric",
        ...(year ? { year: "numeric" as const } : {}),
        ...(h ? { hour: "numeric" as const, minute: "2-digit" as const } : {}),
      }).format(date);
    } catch {
      return match;
    }
  });
}

function translateSegment(trimmed: string, t: Translate): string | null {
  const lower = trimmed.toLowerCase();
  const exact = EXACT[trimmed] ?? SENTENCES[lower];
  if (exact) return t(exact);
  if (lower.startsWith("resets in ") || lower.startsWith("reset in ")) {
    return localizeResetCountdown(trimmed.slice(trimmed.indexOf(" in ") + 4).trim(), t);
  }
  for (const [prefix, key] of COUNTDOWNS) {
    if (!lower.startsWith(prefix)) continue;
    const duration = localizeDuration(trimmed.slice(prefix.length), t);
    if (duration) return fill(t(key), [duration]);
  }
  for (const [pattern, key] of PATTERNS) {
    const m = pattern.exec(trimmed);
    if (m) return fill(t(key), m.slice(1));
  }
  const partial = /^(.+) \(partial\)$/i.exec(trimmed);
  if (partial) return fill(t("ProviderTextPartial"), [translateSegment(partial[1], t) ?? partial[1]]);
  const value = VALUE.exec(trimmed);
  if (value) {
    const unit = UNITS[value[2].toLowerCase()];
    if (unit) return fill(t(unit), [value[1]]);
    if (value[2].toLowerCase() === "conversation") return fill(t("ProviderTextConversations"), [value[1]]);
  }
  const label = localizeProviderLabel(trimmed, t);
  if (label !== trimmed) return label;
  for (const [prefix, key] of PREFIXES) {
    if (lower.startsWith(prefix)) return fill(t(key), [trimmed.slice(prefix.length).trim()]);
  }
  const balance = /^(.+) balance$/i.exec(trimmed);
  if (balance) return fill(t("ProviderTextBalanceSuffix"), [balance[1]]);
  return null;
}

function localizeSegment(segment: string, t: Translate): string {
  const trimmed = segment.trim();
  if (!/[a-z]{2}/i.test(trimmed)) return segment;
  const translated = translateSegment(trimmed, t);
  // Same words in the UI language (English): keep the provider's casing.
  const text = translated == null || translated.toLowerCase() === trimmed.toLowerCase() ? segment : translated;
  return localizeDates(text, t);
}

/** Localize a provider detail line; " · "-joined parts are translated one by one. */
export function localizeProviderText(
  text: string | null | undefined,
  t: Translate,
): string {
  if (!text) return text ?? "";
  const whole = SENTENCES[text.trim().toLowerCase()];
  if (whole) return t(whole);
  return text
    .split(" · ")
    .map((segment) => localizeSegment(segment, t))
    .join(" · ");
}
