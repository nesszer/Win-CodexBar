import { useState } from "react";
import type { LocaleKey } from "../../../i18n/keys";
import type {
  CodexWorkspacesSessionUsage,
  CodexWorkspacesTurnPerformance,
} from "../../../types/bridge";

/** Conversations shown before the expand control, and the most ever shown. */
export const CONVERSATION_INITIAL_ROWS = 8;
export const CONVERSATION_MAX_ROWS = 50;

interface ProjectConversationsProps {
  /** Already ranked by the backend: cost descending, unpriced last. */
  conversations: CodexWorkspacesSessionUsage[];
  t: (key: LocaleKey) => string;
}

export default function ProjectConversations({ conversations, t }: ProjectConversationsProps) {
  const [showAll, setShowAll] = useState(false);
  const capped = conversations.slice(0, CONVERSATION_MAX_ROWS);
  const visible = showAll ? capped : capped.slice(0, CONVERSATION_INITIAL_ROWS);

  return (
    <div style={{ display: "grid", gap: 5, marginTop: 9, paddingTop: 8, borderTop: "1px solid var(--border-subtle)" }}>
      {visible.map((session, index) => (
        <div key={session.id} style={{ display: "grid", gap: 3 }}>
          <div style={{ display: "grid", gridTemplateColumns: "auto minmax(0, 1fr) auto", gap: 10 }}>
            <span className="settings-section__caption" style={{ minWidth: "2.5ch", textAlign: "right" }}>
              #{index + 1}
            </span>
            <span
              title={session.displayTitle}
              style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
            >
              {session.displayTitle}
            </span>
            <span>
              {session.costEstimate.unknownTokens > 0 ? "~" : ""}
              ${session.costEstimate.knownUsd.toFixed(2)}
            </span>
          </div>
          {session.turnPerformance && (
            <TurnPerformance performance={session.turnPerformance} t={t} />
          )}
        </div>
      ))}
      {capped.length > CONVERSATION_INITIAL_ROWS && (
        <button
          type="button"
          className="credential-btn credential-btn--secondary"
          style={{ justifySelf: "start" }}
          aria-expanded={showAll}
          onClick={() => setShowAll((value) => !value)}
        >
          {showAll ? t("UsageSpendShowLess") : `${t("UsageSpendShowAll")} (${capped.length})`}
        </button>
      )}
    </div>
  );
}

function formatRate(value: number): string {
  return value.toFixed(1);
}

function formatSeconds(milliseconds: number): string {
  return (milliseconds / 1000).toFixed(1);
}

function formatOptionalSeconds(milliseconds: number | null, t: (key: LocaleKey) => string): string {
  return milliseconds === null ? "—" : `${formatSeconds(milliseconds)} ${t("UsageSpendSeconds")}`;
}

interface TurnPerformanceProps {
  performance: CodexWorkspacesTurnPerformance;
  t: (key: LocaleKey) => string;
}

/** Observed Codex turn latency and throughput for one conversation (upstream #4304). */
function TurnPerformance({ performance, t }: TurnPerformanceProps) {
  const unit = t("UsageSpendTokensPerSecond");
  const { details } = performance;
  const cached =
    details.cachedInputFraction === null
      ? `${t("UsageSpendCachedInput")}: ${t("UsageSpendCacheUnavailable")}`
      : `${t("UsageSpendCachedInput")}: ${Math.round(details.cachedInputFraction * 100)}%`;

  return (
    <div
      className="settings-section__caption"
      style={{ paddingLeft: "calc(2.5ch + 10px)", display: "grid", gap: 3 }}
    >
      <span title={t("UsageSpendTurnOutputHelp")}>
        {t("UsageSpendTurnOutput")}: {formatRate(performance.outputTokensPerSecond)} {unit}
        {" · "}
        <span title={t("UsageSpendFirstTokenHelp")}>
          {t("UsageSpendFirstToken")}: {formatOptionalSeconds(performance.medianFirstTokenMilliseconds, t)}
        </span>
        {" · "}
        {t("UsageSpendTurnDuration")}: {formatOptionalSeconds(performance.medianDurationMilliseconds, t)}
      </span>
      <details>
        <summary>{t("UsageSpendPerformanceDetails")}</summary>
        <div style={{ display: "grid", gap: 3, paddingTop: 4 }}>
          <span>{t("UsageSpendPerformanceCaveat")}</span>
          <span>
            {t("UsageSpendP95Duration")}:{" "}
            {details.p95DurationMilliseconds === null
              ? t("UsageSpendNeedsTwentyTurns")
              : formatOptionalSeconds(details.p95DurationMilliseconds, t)}
          </span>
          <span>
            {t("UsageSpendP95FirstToken")}:{" "}
            {details.p95FirstTokenMilliseconds === null
              ? t("UsageSpendNeedsTwentyTurns")
              : formatOptionalSeconds(details.p95FirstTokenMilliseconds, t)}
          </span>
          <span>
            {t("UsageSpendMiddleHalfRate")}:{" "}
            {details.outputRateLowerQuartile === null || details.outputRateUpperQuartile === null
              ? t("UsageSpendNeedsFourTurns")
              : `${formatRate(details.outputRateLowerQuartile)}–${formatRate(details.outputRateUpperQuartile)} ${unit}`}
          </span>
          <span>{cached}</span>
          {details.groups.length > 0 && (
            <div style={{ display: "grid", gap: 2 }}>
              <span>{t("UsageSpendByModelEffort")}</span>
              {details.groups.map((group) => (
                <span key={`${group.model ?? ""}|${group.reasoningEffort ?? ""}`}>
                  {group.model ?? t("UsageSpendUnknownModel")} /{" "}
                  {group.reasoningEffort ?? t("UsageSpendUnknownEffort")}: {formatRate(group.outputTokensPerSecond)}{" "}
                  {unit}
                </span>
              ))}
            </div>
          )}
        </div>
      </details>
    </div>
  );
}
