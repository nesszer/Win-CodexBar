import type { CodexWorkspacesSessionUsage } from "../types/bridge";

/** Session list orderings of the Usage & Spend inspector (upstream #4730). */
export type SessionSort = "activity" | "tokens" | "cost";

export const SESSION_SORTS: SessionSort[] = ["activity", "tokens", "cost"];

/**
 * Matches the shown title or the session id. Titles are already replaced
 * by the backend under Hide personal information, so a hidden title can
 * never be found through search.
 */
export function sessionMatches(session: CodexWorkspacesSessionUsage, query: string): boolean {
  const needle = query.trim().toLocaleLowerCase();
  if (!needle) return true;
  return (
    session.displayTitle.toLocaleLowerCase().includes(needle) ||
    session.id.toLocaleLowerCase().includes(needle)
  );
}

/** Known cost, or `null` when nothing in the session could be priced. */
export function sessionCost(session: CodexWorkspacesSessionUsage): number | null {
  const { knownUsd, unknownTokens } = session.costEstimate;
  if (!Number.isFinite(knownUsd) || knownUsd < 0) return null;
  if (unknownTokens > 0 && knownUsd === 0) return null;
  return knownUsd;
}

function activityTime(session: CodexWorkspacesSessionUsage): number | null {
  if (!session.latestActivity) return null;
  const time = Date.parse(session.latestActivity);
  return Number.isNaN(time) ? null : time;
}

/** Missing values sort last in either direction. */
function compareKnown(left: number | null, right: number | null, ascending: boolean): number {
  if (left === right) return 0;
  if (left === null) return 1;
  if (right === null) return -1;
  return ascending ? left - right : right - left;
}

/** Filter by `query`, then order; ties fall back to latest activity, then id. */
export function inspectSessions(
  sessions: CodexWorkspacesSessionUsage[],
  query: string,
  sort: SessionSort = "activity",
  ascending = false,
): CodexWorkspacesSessionUsage[] {
  return sessions
    .filter((session) => sessionMatches(session, query))
    .sort((lhs, rhs) => {
      const primary =
        sort === "tokens"
          ? compareKnown(lhs.totals.totalTokens, rhs.totals.totalTokens, ascending)
          : sort === "cost"
            ? compareKnown(sessionCost(lhs), sessionCost(rhs), ascending)
            : compareKnown(activityTime(lhs), activityTime(rhs), ascending);
      if (primary !== 0) return primary;
      const recent = compareKnown(activityTime(lhs), activityTime(rhs), false);
      if (recent !== 0) return recent;
      return lhs.id < rhs.id ? -1 : lhs.id > rhs.id ? 1 : 0;
    });
}

/** Sub-dollar amounts keep up to six decimals, as upstream does. */
export function formatSessionCost(cost: number | null): string {
  if (cost === null || !Number.isFinite(cost) || cost < 0) return "—";
  if (cost === 0 || cost >= 1) return `$${cost.toFixed(2)}`;
  const minimum = 0.000001;
  const text = `$${Number(Math.max(minimum, cost).toFixed(6)).toLocaleString("en-US", {
    minimumFractionDigits: 2,
    maximumFractionDigits: 6,
  })}`;
  return cost < minimum ? `<${text}` : text;
}
