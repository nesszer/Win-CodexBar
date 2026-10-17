import type { LocaleKey } from "../i18n/keys";
import type {
  ModelRateLimitSnapshot,
  ProviderThroughputSnapshot,
} from "../types/bridge";

/**
 * Pure helpers for the OpenAI API Tokens/min chart and the manual per-model
 * limit editor (upstream 0.75.0 #4495). The shell validates the same rules
 * again on save; these keep the editor from sending rows it would reject.
 */

/** Largest integer a JavaScript number holds exactly. */
export const MAX_RATE_LIMIT = Number.MAX_SAFE_INTEGER;
export const MAX_MODEL_ID_LENGTH = 200;

export const LIVE_WINDOW_MINUTES = [15, 60, 360] as const;
export type LiveWindowMinutes = (typeof LIVE_WINDOW_MINUTES)[number];

export interface TpmBar {
  /** Minute start, epoch seconds. */
  start: number;
  tokens: number;
  /** Not reported yet: drawn as pending, never as a zero or a peak. */
  isPending: boolean;
}

/** The regular one-minute grid, oldest first. */
export function tpmBars(snapshot: ProviderThroughputSnapshot): TpmBar[] {
  return snapshot.minutes.map((minute) => ({
    start: minute.start,
    tokens: minute.totalTokens,
    isPending: minute.isPending,
  }));
}

/** True when at least one reported (non-pending) minute carries tokens. */
export function hasReportedTraffic(snapshot: ProviderThroughputSnapshot): boolean {
  return snapshot.minutes.some((minute) => !minute.isPending && minute.totalTokens > 0);
}

export interface ThroughputLimitLine {
  model: string;
  /** Peak non-pending limit-counted tokens in one minute. */
  peakTokens: number;
  tokensPerMinuteLimit: number;
  percent: number;
}

/**
 * The limit line for the busiest model that has a manual limit, mirroring
 * `throughput_limit_utilization`. Project-scoped snapshots never get one (their
 * caps are per project, not per organization). Matches model IDs exactly; the
 * shell's shared-pool aliasing is not mirrored here.
 */
export function throughputLimitLine(
  snapshot: ProviderThroughputSnapshot,
  limits: readonly ModelRateLimitSnapshot[],
): ThroughputLimitLine | null {
  if (snapshot.projectId !== null) return null;
  let best: ThroughputLimitLine | null = null;
  for (const model of snapshot.models) {
    const limit = limits.find((row) => row.model === model.name);
    if (!limit || limit.tokensPerMinute <= 0) continue;
    const percent = (model.peakLimitCountedTokens / limit.tokensPerMinute) * 100;
    // A tie keeps the earlier model, as the shell does.
    if (!best || percent > best.percent) {
      best = {
        model: model.name,
        peakTokens: model.peakLimitCountedTokens,
        tokensPerMinuteLimit: limit.tokensPerMinute,
        percent,
      };
    }
  }
  return best;
}

/** One editor row. Numbers stay strings until validation. */
export interface LimitDraft {
  model: string;
  tokensPerMinute: string;
  requestsPerMinute: string;
}

export type LimitDraftResult =
  | { ok: true; limits: ModelRateLimitSnapshot[] }
  | { ok: false; error: LocaleKey; index: number };

export function draftsFromLimits(limits: readonly ModelRateLimitSnapshot[]): LimitDraft[] {
  return limits.map((limit) => ({
    model: limit.model,
    tokensPerMinute: String(limit.tokensPerMinute),
    requestsPerMinute:
      limit.requestsPerMinute == null ? "" : String(limit.requestsPerMinute),
  }));
}

const WHOLE_NUMBER = /^\d+$/;

/** C0 controls and DEL; checked by code point so the pattern needs no control escapes. */
function hasControlCharacter(value: string): boolean {
  for (const char of value) {
    const code = char.codePointAt(0) ?? 0;
    if (code <= 0x1f || code === 0x7f) return true;
  }
  return false;
}

/**
 * Validates every row before save. A bad row is reported with its index; the
 * caller keeps the editor state so nothing is dropped silently. A requests
 * limit of 0 or an empty field means "no requests limit", as upstream.
 */
export function parseLimitDrafts(drafts: readonly LimitDraft[]): LimitDraftResult {
  const seen = new Set<string>();
  const limits: ModelRateLimitSnapshot[] = [];
  for (let index = 0; index < drafts.length; index += 1) {
    const draft = drafts[index];
    const model = draft.model.trim();
    if (model.length === 0) return { ok: false, error: "OpenAiLimitErrorModel", index };
    // Code points, matching the shell's `chars().count()` check.
    if ([...model].length > MAX_MODEL_ID_LENGTH || hasControlCharacter(model)) {
      return { ok: false, error: "OpenAiLimitErrorLine", index };
    }
    if (seen.has(model)) return { ok: false, error: "OpenAiLimitErrorDuplicate", index };
    seen.add(model);

    const tpm = draft.tokensPerMinute.trim();
    const tokensPerMinute = Number(tpm);
    if (
      !WHOLE_NUMBER.test(tpm) ||
      tokensPerMinute < 1 ||
      tokensPerMinute > MAX_RATE_LIMIT
    ) {
      return { ok: false, error: "OpenAiLimitErrorTpm", index };
    }

    const rpm = draft.requestsPerMinute.trim();
    let requestsPerMinute: number | null = null;
    if (rpm.length > 0) {
      if (!WHOLE_NUMBER.test(rpm) || Number(rpm) > MAX_RATE_LIMIT) {
        return { ok: false, error: "OpenAiLimitErrorRpm", index };
      }
      requestsPerMinute = Number(rpm) > 0 ? Number(rpm) : null;
    }

    limits.push({ model, tokensPerMinute, requestsPerMinute });
  }
  return { ok: true, limits };
}

const compactFormat = new Intl.NumberFormat("en-US", {
  notation: "compact",
  maximumFractionDigits: 1,
});

/** Compact token count, e.g. 1.2K or 3.4M. */
export function formatTokens(value: number): string {
  return compactFormat.format(value);
}

/** Local clock time of an epoch-seconds value. */
export function formatClock(epochSeconds: number): string {
  return new Date(epochSeconds * 1000).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}
