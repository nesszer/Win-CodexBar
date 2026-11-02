import { normalizeResetDescription } from "../hooks/useFormattedResetTime";
import type { LocaleKey } from "../i18n/keys";
import type {
  DailyTargetSnapshot,
  PaceStage,
  RateWindowSnapshot,
  WindowPaceSnapshot,
  WorkdayTickStyle,
} from "../types/bridge";
import { type QuotaWarningSettings, resolveQuotaWarning } from "./quotaWarnings";
import { resetDescriptionFallback } from "./usageWindows";

type Translate = (key: LocaleKey) => string;

export type MetricLane = "primary" | "secondary" | "tertiary" | "extra";

type PaceKind = "session" | "weekly";

export type UsageThresholdSettings = QuotaWarningSettings;

interface LanePolicy {
  thresholdWindow: "session" | "weekly" | null;
  workdayTicks: boolean;
  pace: "byWindowLength" | "exactWindow" | "never";
}

export const SESSION_MINUTES = 300;
export const WEEK_MINUTES = 7 * 24 * 60;

const LANE_POLICY: Record<MetricLane, LanePolicy> = {
  primary: { thresholdWindow: "session", workdayTicks: false, pace: "byWindowLength" },
  secondary: { thresholdWindow: "weekly", workdayTicks: true, pace: "byWindowLength" },
  tertiary: { thresholdWindow: "weekly", workdayTicks: false, pace: "never" },
  extra: { thresholdWindow: null, workdayTicks: false, pace: "exactWindow" },
};

/** The quota warning window a lane follows (`null` for extra windows), as notifications do. */
export function quotaWarningWindowForLane(lane: MetricLane): "session" | "weekly" | null {
  return LANE_POLICY[lane].thresholdWindow;
}

const PACE_DIRECTION: Record<PaceStage, "onPace" | "deficit" | "reserve"> = {
  on_track: "onPace",
  slightly_ahead: "deficit",
  ahead: "deficit",
  far_ahead: "deficit",
  slightly_behind: "reserve",
  behind: "reserve",
  far_behind: "reserve",
};

interface CountdownKeys {
  daysHours: LocaleKey;
  daysMinutes: LocaleKey;
  days: LocaleKey;
  hoursMinutes: LocaleKey;
  hours: LocaleKey;
  minutes: LocaleKey;
}

const RESET_KEYS: CountdownKeys = {
  daysHours: "ResetsInDaysHours",
  daysMinutes: "ResetsInDaysMinutes",
  days: "ResetsInDaysOnly",
  hoursMinutes: "ResetsInHoursMinutes",
  hours: "ResetsInHoursOnly",
  minutes: "ResetsInMinutes",
};

const DURATION_KEYS: CountdownKeys = {
  daysHours: "DurationDaysHours",
  daysMinutes: "DurationDaysMinutes",
  days: "DurationDays",
  hoursMinutes: "DurationHoursMinutes",
  hours: "DurationHours",
  minutes: "DurationMinutes",
};

export interface MetricRowInput {
  snap: RateWindowSnapshot;
  lane: MetricLane;
  providerId: string;
  resetText: string | null;
  compact: boolean;
  showAsUsed: boolean;
  showResetWhenExhausted: boolean;
  paceEnabled: boolean;
  usageThresholds: UsageThresholdSettings | null;
  /** Settings > Menu > Show quota warning markers. */
  quotaWarningMarkersVisible: boolean;
  weeklyProgressWorkDays: number | null;
  /** Settings > Menu > Workday tick appearance; "hidden" draws no workday ticks. */
  workdayTickStyle: WorkdayTickStyle;
  now: number;
}

export interface UsageBarMarker {
  percent: number;
  kind: "warning" | "workday";
}

export interface UsageBarModel {
  fillPercent: number;
  valuePercent: number;
  valueText: string;
  pacePercent: number | null;
  paceDeficit: boolean;
  markers: UsageBarMarker[];
  /** How `workday` markers are drawn. */
  workdayTickStyle: Exclude<WorkdayTickStyle, "hidden">;
}

export interface MetricRowPresentation {
  percentText: string | null;
  resetText: string | null;
  metaText: string | null;
  bar: UsageBarModel;
}

function fill(template: string, ...values: Array<string | number>): string {
  return values.reduce<string>((text, value) => text.replace("{}", String(value)), template);
}

function clampPercent(value: number): number {
  return Math.min(100, Math.max(0, value));
}

/** `%.0f` rounds exact halves to even. */
function roundHalfEven(value: number): number {
  const rounded = Math.round(value);
  return Math.abs(value % 1) === 0.5 && rounded % 2 !== 0 ? rounded - 1 : rounded;
}

function percentLabel(percent: number, suffix: string): string {
  if (percent > 0 && percent < 1) return `<1% ${suffix}`;
  return `${roundHalfEven(percent)}% ${suffix}`;
}

function countdownParts(seconds: number) {
  if (seconds < 1) return null;
  const total = Math.max(1, Math.ceil(seconds / 60));
  return { days: Math.floor(total / 1440), hours: Math.floor(total / 60) % 24, minutes: total % 60 };
}

function formatCountdown(seconds: number, keys: CountdownKeys, t: Translate): string | null {
  const parts = countdownParts(seconds);
  if (!parts) return null;
  const { days, hours, minutes } = parts;
  if (days > 0) {
    if (hours > 0) return fill(t(keys.daysHours), days, hours);
    if (minutes > 0) return fill(t(keys.daysMinutes), days, minutes);
    return fill(t(keys.days), days);
  }
  if (hours > 0) {
    return minutes > 0 ? fill(t(keys.hoursMinutes), hours, minutes) : fill(t(keys.hours), hours);
  }
  return fill(t(keys.minutes), minutes);
}

function intlLocale(t: Translate): string | undefined {
  const locale = t("IntlLocale");
  return /^[a-z]{2}-[A-Z]{2}$/.test(locale) ? locale : undefined;
}

function sameLocalDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate()
  );
}

function absoluteResetDescription(target: Date, now: Date, t: Translate): string {
  const locale = intlLocale(t);
  const time = new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit" });
  if (sameLocalDay(target, now)) return time.format(target);
  const tomorrow = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  if (sameLocalDay(target, tomorrow)) return fill(t("ResetTomorrowAt"), time.format(target));
  return new Intl.DateTimeFormat(locale, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(target);
}

/**
 * Upstream `syntheticRegenDetail`: "Regenerates %@" around the countdown, always
 * relative. The bare localized duration fills `RegeneratesCountdown`, which owns
 * its own preposition per locale; zero remaining uses the whole-sentence
 * `RegeneratesNow`.
 */
function regenText(seconds: number, t: Translate): string {
  const duration = formatCountdown(seconds, DURATION_KEYS, t);
  return duration === null ? t("RegeneratesNow") : fill(t("RegeneratesCountdown"), duration);
}

export function metricResetText(
  snap: RateWindowSnapshot,
  relative: boolean,
  now: number,
  t: Translate,
): string | null {
  const target = snap.resetsAt ? Date.parse(snap.resetsAt) : Number.NaN;
  if (!Number.isFinite(target)) {
    return normalizeResetDescription(resetDescriptionFallback(snap), t);
  }
  if (snap.resetRegenerates) {
    return regenText((target - now) / 1000, t);
  }
  if (!relative) {
    return fill(t("ResetsAtLabel"), absoluteResetDescription(new Date(target), new Date(now), t));
  }
  return formatCountdown((target - now) / 1000, RESET_KEYS, t) ?? t("ProviderTextResetsNow");
}

function paceKind(lane: MetricLane, windowMinutes: number | null, resetWindow: boolean): PaceKind | null {
  // Reset-window pace is weekly pace on every card lane, as on the Mac;
  // extra windows keep their exact-window rule.
  if (resetWindow && lane !== "extra") return "weekly";
  const rule = LANE_POLICY[lane].pace;
  if (rule === "never" || windowMinutes == null) return null;
  if (rule === "exactWindow") {
    if (windowMinutes === SESSION_MINUTES) return "session";
    return windowMinutes === WEEK_MINUTES ? "weekly" : null;
  }
  return windowMinutes <= SESSION_MINUTES ? "session" : "weekly";
}

function visiblePace(input: MetricRowInput): { pace: WindowPaceSnapshot; kind: PaceKind } | null {
  const { snap } = input;
  const pace = snap.pace;
  if (!input.paceEnabled || !pace || !(snap.usedPercent < 100)) return null;
  // A regenerating window shows its regen line instead of pace, as upstream
  // MenuCardView+ModelHelpers.swift:179-186 and MenuCardView.swift:1407-1415
  // do. The window keeps its length, so workday ticks and the tray still use it.
  if (snap.resetRegenerates) return null;
  if (!Number.isFinite(pace.expectedUsedPercent) || !Number.isFinite(pace.actualUsedPercent)) return null;
  const kind = paceKind(input.lane, snap.windowMinutes, pace.resetWindow === true);
  if (!kind) return null;
  const expectsEnough = pace.expectedUsedPercent >= 3;
  if (kind === "session" ? !expectsEnough : !expectsEnough && pace.etaSeconds !== 0) return null;
  return { pace, kind };
}

function paceMetaText(pace: WindowPaceSnapshot, kind: PaceKind, t: Translate): string {
  const delta = Math.round(Math.abs(pace.deltaPercent));
  const direction = PACE_DIRECTION[pace.stage];
  const left =
    delta === 0 || direction === "onPace"
      ? t("PaceOnPace")
      : fill(t(direction === "deficit" ? "PaceInDeficit" : "PaceInReserve"), delta);
  let right: string | null = null;
  if (pace.willLastToReset) {
    right = t("PaceLastsUntilReset");
  } else if (pace.etaSeconds != null) {
    const duration = formatCountdown(pace.etaSeconds, DURATION_KEYS, t);
    if (kind === "session") {
      right = duration ? fill(t("PaceProjectedEmptyIn"), duration) : t("PaceProjectedEmptyNow");
    } else {
      right = duration ? fill(t("PaceRunsOutIn"), duration) : t("PaceRunsOutNow");
    }
  }
  return right ? `${left} · ${right}` : left;
}

const DAILY_TARGET_KEYS: Record<string, LocaleKey> = {
  "%": "PaceDailyTargetPercent",
  credits: "PaceDailyTargetCredits",
  requests: "PaceDailyTargetRequests",
};

/**
 * Upstream `UsagePaceText.dailyTarget`: the per-day share in the allowance's
 * own unit. Units without a key (for example a currency) show no target.
 */
function dailyTargetText(target: DailyTargetSnapshot, t: Translate): string | null {
  const key = DAILY_TARGET_KEYS[target.unit];
  if (!key || !Number.isFinite(target.amount)) return null;
  const digits = target.unit === "%" ? 1 : 2;
  const amount = new Intl.NumberFormat(intlLocale(t), { maximumFractionDigits: digits }).format(
    Math.max(0, target.amount),
  );
  return fill(t(key), amount);
}

function normalizedMarkerPercents(values: number[]): number[] {
  const kept: number[] = [];
  for (const value of values.map(clampPercent)) {
    if (value > 0 && value < 100 && !kept.some((other) => Math.abs(other - value) < 0.001)) {
      kept.push(value);
    }
  }
  return kept;
}

function barMarkers(input: MetricRowInput): UsageBarMarker[] {
  const policy = LANE_POLICY[input.lane];
  const window = policy.thresholdWindow;
  const settings = input.usageThresholds;
  // A window set to Off for this provider shows no warning markers (Mac parity).
  const resolved =
    window && settings && input.quotaWarningMarkersVisible
      ? resolveQuotaWarning(settings, input.providerId, window)
      : null;
  const warnings =
    resolved?.enabled
      ? normalizedMarkerPercents(
          [resolved.high, resolved.critical].map((usedThreshold) =>
            input.showAsUsed ? usedThreshold : 100 - usedThreshold,
          ),
        )
      : [];
  const workDays = input.weeklyProgressWorkDays;
  const showsWorkdays =
    policy.workdayTicks &&
    input.workdayTickStyle !== "hidden" &&
    input.snap.windowMinutes === WEEK_MINUTES &&
    workDays != null &&
    workDays >= 2 &&
    workDays <= 7;
  const workdays = showsWorkdays
    ? normalizedMarkerPercents(
        Array.from({ length: workDays - 1 }, (_, index) => ((index + 1) * 100) / workDays),
      ).filter((day) => !warnings.some((warning) => Math.abs(warning - day) < 0.001))
    : [];
  return [
    ...warnings.map((percent) => ({ percent, kind: "warning" as const })),
    ...workdays.map((percent) => ({ percent, kind: "workday" as const })),
  ].sort((a, b) => a.percent - b.percent);
}

function barValueText(valuePercent: number, markers: UsageBarMarker[], t: Translate): string {
  const parts = [`${valuePercent}%`];
  const groups: Array<[UsageBarMarker["kind"], LocaleKey]> = [
    ["warning", "UsageBarQuotaWarnings"],
    ["workday", "UsageBarWorkDays"],
  ];
  for (const [kind, key] of groups) {
    const percents = markers.filter((marker) => marker.kind === kind).map((marker) => `${Math.round(marker.percent)}%`);
    if (percents.length > 0) parts.push(`${t(key)}: ${percents.join(", ")}`);
  }
  return parts.join(". ");
}

function renderedFillPercent(percent: number): number {
  const shown = Math.round(percent);
  if (shown <= 0) return 0;
  if (shown >= 100) return 100;
  return percent;
}

export function metricRowPresentation(input: MetricRowInput, t: Translate): MetricRowPresentation {
  const { snap, showAsUsed } = input;
  const used = Number.isFinite(snap.usedPercent) ? Math.max(0, snap.usedPercent) : 0;
  const shown = clampPercent(showAsUsed ? used : 100 - used);
  const resetTarget = snap.resetsAt ? Date.parse(snap.resetsAt) : Number.NaN;
  const replacesPercent =
    input.showResetWhenExhausted && snap.isExhausted && resetTarget > input.now && input.resetText !== null;
  const visible = visiblePace(input);
  const markers = barMarkers(input);
  // The daily target shares the pace setting and the non-compact meta line.
  const dailyTarget =
    input.paceEnabled && !input.compact && snap.dailyTarget
      ? dailyTargetText(snap.dailyTarget, t)
      : null;
  const metaParts = [
    visible && !input.compact ? paceMetaText(visible.pace, visible.kind, t) : null,
    dailyTarget,
  ].filter((part): part is string => part !== null);
  const valuePercent = Math.round(shown);
  const pacePercent =
    visible && PACE_DIRECTION[visible.pace.stage] !== "onPace"
      ? clampPercent(showAsUsed ? visible.pace.expectedUsedPercent : 100 - visible.pace.expectedUsedPercent)
      : null;
  return {
    percentText: replacesPercent
      ? null
      : percentLabel(shown, t(showAsUsed ? "PanelUsedSuffix" : "PanelLeftSuffix")),
    resetText: input.compact && !replacesPercent ? null : input.resetText,
    metaText: metaParts.length > 0 ? metaParts.join(" · ") : null,
    bar: {
      fillPercent: renderedFillPercent(shown),
      valuePercent,
      valueText: barValueText(valuePercent, markers, t),
      pacePercent,
      paceDeficit: visible ? visible.pace.actualUsedPercent > visible.pace.expectedUsedPercent : false,
      markers,
      workdayTickStyle: input.workdayTickStyle === "high_contrast" ? "high_contrast" : "subtle",
    },
  };
}
