import type { RateWindowSnapshot } from "../types/bridge";

export interface PaceBudget {
  now: number;
  nextHour: number;
  nextFiveHours: number;
  today: number;
}

interface PaceWindow {
  startMs: number;
  resetMs: number;
  durationMs: number;
}

const HOUR_MS = 60 * 60 * 1000;

function paceWindow(
  snap: RateWindowSnapshot,
  nowMs: number,
): PaceWindow | null {
  if (
    snap.windowMinutes == null ||
    !Number.isFinite(snap.windowMinutes) ||
    snap.windowMinutes <= 0 ||
    !snap.resetsAt
  ) {
    return null;
  }

  const resetMs = Date.parse(snap.resetsAt);
  const durationMs = snap.windowMinutes * 60 * 1000;
  if (!Number.isFinite(resetMs) || resetMs <= nowMs || durationMs <= 0) {
    return null;
  }

  return {
    startMs: resetMs - durationMs,
    resetMs,
    durationMs,
  };
}

function startOfTomorrow(now: Date): number {
  const tomorrow = new Date(now);
  tomorrow.setHours(24, 0, 0, 0);
  return tomorrow.getTime();
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function getPaceBudget(
  snap: RateWindowSnapshot,
  now = new Date(),
): PaceBudget | null {
  const nowMs = now.getTime();
  if (
    !Number.isFinite(nowMs) ||
    snap.isExhausted ||
    !Number.isFinite(snap.usedPercent) ||
    !Number.isFinite(snap.remainingPercent)
  ) {
    return null;
  }

  const window = paceWindow(snap, nowMs);
  if (!window) return null;

  const remaining = Math.max(0, snap.remainingPercent);
  const budgetAt = (targetMs: number) => {
    const cappedTarget = Math.min(targetMs, window.resetMs);
    const elapsed = clamp(
      cappedTarget - window.startMs,
      0,
      window.durationMs,
    );
    const expectedUsed = (elapsed / window.durationMs) * 100;
    return clamp(expectedUsed - snap.usedPercent, 0, remaining);
  };

  return {
    now: budgetAt(nowMs),
    nextHour: budgetAt(nowMs + HOUR_MS),
    nextFiveHours: budgetAt(nowMs + 5 * HOUR_MS),
    today: budgetAt(startOfTomorrow(now)),
  };
}

export interface PaceChartSnapshot {
  elapsedPercent: number;
  usedPercent: number;
  projectedPercent: number;
}

export function getPaceChartSnapshot(
  snap: RateWindowSnapshot,
  now = new Date(),
): PaceChartSnapshot | null {
  const nowMs = now.getTime();
  const window = paceWindow(snap, nowMs);
  if (!window || !Number.isFinite(snap.usedPercent)) return null;

  const elapsedPercent = clamp(
    ((nowMs - window.startMs) / window.durationMs) * 100,
    0,
    100,
  );
  const usedPercent = clamp(snap.usedPercent, 0, 100);
  const projectedPercent =
    elapsedPercent > 0
      ? clamp((usedPercent / elapsedPercent) * 100, 0, 100)
      : usedPercent;

  return { elapsedPercent, usedPercent, projectedPercent };
}
