import { useEffect, useMemo, useRef, useState } from "react";
import type { LocaleKey } from "../../../i18n/keys";
import { getCodexSessionDetails } from "../../../lib/tauri";
import { formatCompactTokens } from "../../../lib/usageSpendDashboard";
import {
  SESSION_SORTS,
  formatSessionCost,
  inspectSessions,
  sessionCost,
  type SessionSort,
} from "../../../lib/sessionInspector";
import type {
  CodexSessionDetailCheckpoint,
  CodexSessionDetailError,
  CodexSessionDetails,
  CodexSessionTurn,
  CodexWorkspacesSessionUsage,
} from "../../../types/bridge";
import ToolActivity from "./ToolActivity";

type Translate = (key: LocaleKey) => string;

const SORT_KEYS: Record<SessionSort, LocaleKey> = {
  activity: "UsageSpendRecentActivity",
  tokens: "UsageSpendColTokens",
  cost: "UsageSpendColCost",
};

type DetailState =
  | { status: "loading" }
  | { status: "loaded"; details: CodexSessionDetails }
  | { status: "error"; code: CodexSessionDetailError };

interface SessionInspectorProps {
  conversations: CodexWorkspacesSessionUsage[];
  /** History window of the spend contract; request details read the same dates. */
  historyDays?: number;
  t: Translate;
}

/**
 * Search, sort and open one Codex session (upstream #4730). The list reads
 * only the session summaries; request-level detail loads when a session is
 * selected and is dropped when the selection changes.
 */
export default function SessionInspector({ conversations, historyDays, t }: SessionInspectorProps) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<SessionSort>("activity");
  const [ascending, setAscending] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const rows = useMemo(
    () => inspectSessions(conversations, query, sort, ascending),
    [conversations, query, sort, ascending],
  );
  const selected = conversations.find((session) => session.id === selectedId) ?? null;

  return (
    <div className="usage-spend__inspector" style={{ display: "grid", gap: 10, marginTop: 9 }}>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center" }}>
        <input
          type="search"
          className="provider-detail-field__input"
          style={{ flex: "1 1 180px", minWidth: 0 }}
          placeholder={t("UsageSpendSearchSessions")}
          aria-label={t("UsageSpendSearchSessions")}
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <select
          className="provider-detail-select"
          aria-label={t("UsageSpendSortBy")}
          value={sort}
          onChange={(event) => setSort(event.target.value as SessionSort)}
        >
          {SESSION_SORTS.map((option) => (
            <option key={option} value={option}>
              {t(SORT_KEYS[option])}
            </option>
          ))}
        </select>
        <button
          type="button"
          className="credential-btn credential-btn--secondary"
          aria-pressed={ascending}
          onClick={() => setAscending((value) => !value)}
        >
          {ascending ? t("UsageSpendAscending") : t("UsageSpendDescending")}
        </button>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(0, 2fr) minmax(0, 3fr)", gap: 12 }}>
        <div role="list" aria-label={t("UsageSpendViewSessions")} style={{ display: "grid", gap: 4, alignContent: "start" }}>
          {rows.length === 0 ? (
            <p className="settings-section__caption">{t("UsageSpendNoMatchingSessions")}</p>
          ) : (
            rows.map((session) => (
              <div role="listitem" key={session.id}>
                <button
                  type="button"
                  className="usage-spend__row-button"
                  aria-pressed={session.id === selectedId}
                  onClick={() => setSelectedId(session.id)}
                  style={{ width: "100%" }}
                >
                  <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    <strong title={session.displayTitle}>{session.displayTitle}</strong>
                    <span className="settings-section__caption" style={{ display: "block" }}>
                      {formatCompactTokens(session.totals.totalTokens)} {t("UsageSpendTokens")}
                      {session.latestActivity ? ` · ${formatTime(session.latestActivity)}` : ""}
                    </span>
                  </span>
                  <span>{summaryCostText(session)}</span>
                </button>
              </div>
            ))
          )}
        </div>

        <div className="provider-detail-section" style={{ padding: "10px 12px", minWidth: 0 }}>
          {selected ? (
            <SessionDetailPane key={selected.id} session={selected} historyDays={historyDays} t={t} />
          ) : (
            <p className="settings-section__caption">{t("UsageSpendSelectSession")}</p>
          )}
        </div>
      </div>
    </div>
  );
}

function summaryCostText(session: CodexWorkspacesSessionUsage): string {
  const partial = session.costEstimate.unknownTokens > 0 && sessionCost(session) !== null;
  return `${partial ? "~" : ""}${formatSessionCost(sessionCost(session))}`;
}

function formatTime(value: string | null): string {
  if (!value) return "—";
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function errorCode(error: unknown): CodexSessionDetailError {
  const text = error instanceof Error ? error.message : String(error);
  return text === "limitExceeded" || text === "sourceChanged" ? text : "unavailable";
}

function SessionDetailPane({
  session,
  historyDays,
  t,
}: {
  session: CodexWorkspacesSessionUsage;
  historyDays?: number;
  t: Translate;
}) {
  const [state, setState] = useState<DetailState>({ status: "loading" });
  const request = useRef(0);
  // A refreshed summary is a new checkpoint, so the details are read again.
  const checkpointKey = JSON.stringify({ totals: session.totals, costEstimate: session.costEstimate });

  useEffect(() => {
    const id = ++request.current;
    const checkpoint = JSON.parse(checkpointKey) as CodexSessionDetailCheckpoint;
    setState({ status: "loading" });
    getCodexSessionDetails(session.id, checkpoint, historyDays).then(
      (details) => {
        if (request.current === id) setState({ status: "loaded", details });
      },
      (error: unknown) => {
        if (request.current === id) setState({ status: "error", code: errorCode(error) });
      },
    );
    return () => {
      request.current += 1;
    };
  }, [session.id, checkpointKey, historyDays]);

  const { totals } = session;
  return (
    <div style={{ display: "grid", gap: 8 }} data-testid="session-detail">
      <div style={{ minWidth: 0 }}>
        <h4 style={{ margin: 0, overflowWrap: "anywhere" }}>{session.displayTitle}</h4>
        <span className="settings-section__caption">
          {[session.topModel, formatTime(session.latestActivity)].filter(Boolean).join(" · ")}
        </span>
      </div>
      <dl className="usage-spend__mix">
        <Metric label={t("UsageSpendTokenInput")} value={formatCompactTokens(totals.inputTokens)} />
        <Metric label={t("UsageSpendCachedInput")} value={formatCompactTokens(totals.cachedInputTokens)} />
        <Metric label={t("UsageSpendTokenOutput")} value={formatCompactTokens(totals.outputTokens)} />
        <Metric label={t("UsageSpendColTokens")} value={formatCompactTokens(totals.totalTokens)} />
        <Metric label={t("UsageSpendEstimatedSpend")} value={summaryCostText(session)} />
      </dl>
      <p className="settings-section__caption" style={{ margin: 0 }}>{t("UsageSpendCachedReasoningNote")}</p>

      <section aria-label={t("UsageSpendModelRequests")} style={{ display: "grid", gap: 4 }}>
        <h5 style={{ margin: "4px 0 0" }}>{t("UsageSpendModelRequests")}</h5>
        <p className="settings-section__caption" style={{ margin: 0 }}>{t("UsageSpendTurnCaption")}</p>
        {state.status === "loading" && (
          <span className="settings-section__caption" role="status">{t("UsageSpendLoading")}</span>
        )}
        {state.status === "error" && (
          <span className="settings-section__caption" role="alert">
            {state.code === "limitExceeded"
              ? t("UsageSpendRequestDetailsLimit")
              : t("UsageSpendRequestDetailsUnavailable")}
          </span>
        )}
        {state.status === "loaded" &&
          (state.details.turns.length === 0 ? (
            <span className="settings-section__caption">{t("UsageSpendNoTurnRecords")}</span>
          ) : (
            <TurnList turns={state.details.turns} t={t} />
          ))}
      </section>

      <section style={{ display: "grid", gap: 4 }}>
        <p className="settings-section__caption" style={{ margin: 0 }}>{t("UsageSpendToolActivityNoCost")}</p>
        <ToolActivity sessionId={session.id} historyDays={historyDays} t={t} />
      </section>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="usage-spend__stat">
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}

function turnTitle(turn: CodexSessionTurn, number: number, t: Translate): string {
  return turn.turnId === null
    ? t("UsageSpendUnattributedUsage")
    : t("UsageSpendTurnNumber").replace("{}", String(number));
}

function TurnList({ turns, t }: { turns: CodexSessionTurn[]; t: Translate }) {
  let number = 0;
  return (
    <div style={{ display: "grid", gap: 4 }} data-testid="session-turns">
      {turns.map((turn) => {
        if (turn.turnId !== null) number += 1;
        const title = turnTitle(turn, number, t);
        return (
          <details key={turn.turnId ?? "unattributed"} className="settings-section__caption">
            <summary>
              <strong>{title}</strong>
              {` · ${turn.requestCount} ${t("UsageSpendRequests")} · ${formatCompactTokens(turn.totalTokens)} ${t("UsageSpendTokens")} · ${formatSessionCost(turn.costUsd)}`}
              {turn.firstActivity ? ` · ${formatTime(turn.firstActivity)}` : ""}
            </summary>
            <div style={{ display: "grid", gap: 3, padding: "4px 0 4px 14px" }}>
              <span>
                {t("UsageSpendTokenInput")} {formatCompactTokens(turn.inputTokens)} · {t("UsageSpendCachedInput")}{" "}
                {formatCompactTokens(turn.cachedInputTokens)} · {t("UsageSpendTokenOutput")}{" "}
                {formatCompactTokens(turn.outputTokens)} · {t("UsageSpendTokenReasoning")}{" "}
                {formatCompactTokens(turn.reasoningTokens)}
              </span>
              {turn.requests.map((request, index) => (
                <span key={request.id}>
                  <strong>{t("UsageSpendRequestNumber").replace("{}", String(index + 1))}</strong>
                  {` · ${request.modelName} · ${formatTime(request.timestamp)} · ${formatCompactTokens(request.inputTokens)} / ${formatCompactTokens(request.cachedInputTokens)} / ${formatCompactTokens(request.outputTokens)} · ${formatSessionCost(request.costUsd)}`}
                </span>
              ))}
            </div>
          </details>
        );
      })}
    </div>
  );
}
