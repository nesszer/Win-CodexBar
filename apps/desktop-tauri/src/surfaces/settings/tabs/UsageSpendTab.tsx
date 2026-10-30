import { useCallback, useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useLocale } from "../../../hooks/useLocale";
import { Field, Toggle } from "../../../components/FormControls";
import { UsageSpendDailyLedger } from "../../../components/UsageSpendDailyLedger";
import { getUsageSpendSummary, writeUsageSpendExport } from "../../../lib/tauri";
import { formatUsd, shareUsageSpendPng } from "../../../lib/usageSpendSharing";
import { buildSpendProviderRowGroups } from "../../../lib/usageSpendBreakdown";
import {
  SPEND_DASHBOARD_PERIODS,
  formatCompactTokens,
  isContractHidden,
  normalizeHiddenSourceIds,
  spendSources,
  spendTotalsByCurrency,
  toggleHiddenSource,
  visibleUsageSpendSummary,
} from "../../../lib/usageSpendDashboard";
import type {
  CodexWorkspacesProjectUsage,
  CodexWorkspacesSessionUsage,
  SpendContract,
  SpendTrendSource,
  UsageSpendSummary,
} from "../../../types/bridge";
import { SpendProviderIcon } from "../../../components/providers/SpendProviderIcon";
import type { LocaleKey } from "../../../i18n/keys";
import {
  costPeriodLabel,
  costPeriodShortLabel,
  normalizeCostPeriod,
  parseCostPeriod,
} from "../../../lib/costPeriod";
import type { TabProps } from "../settingsTabs";
import ProjectConversations from "./ProjectConversations";
import SessionInspector from "./SessionInspector";
import { UsageSpendProviderGroups } from "./UsageSpendProviderGroups";
import { UsageSpendTrendPanel } from "./UsageSpendTrendPanel";
import { TokenActivityGrid } from "./TokenActivityGrid";
import { mergeTokenActivity, type TokenActivityPoint } from "../../../lib/tokenActivity";

type Translate = (key: LocaleKey) => string;
type BreakdownView = "providers" | "projects" | "sessions";

/**
 * Settings > Usage & Spend, laid out like the macOS pane: header and refresh,
 * the time range, the per-currency stat strip, token mix and coverage, the
 * Providers / Projects / Sessions picker, the trend and ledger, then the data
 * controls (Track costs, OpenCodex, Sources, exports).
 *
 * Hidden sources only filter this view: every section and every export reads
 * `visibleUsageSpendSummary`.
 */
export default function UsageSpendTab({ settings, set, saving }: TabProps) {
  const { t } = useLocale();
  const period = normalizeCostPeriod(settings.spendDashboardPeriod ?? settings.costReportingPeriod);
  const trackCosts = settings.costUsageEnabled ?? true;
  const includeOpenCodex = settings.openCodexUsageLogsEnabled ?? false;
  const hideNativeCodex = settings.hideNativeCodexCostWhenOpenCodexPresent ?? false;
  const hiddenKey = normalizeHiddenSourceIds(settings.spendDashboardHiddenSourceIds ?? []).join(",");
  const hidden = useMemo(() => (hiddenKey ? hiddenKey.split(",") : []), [hiddenKey]);

  const [summary, setSummary] = useState<UsageSpendSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [shareError, setShareError] = useState<string | null>(null);
  const [view, setView] = useState<BreakdownView>("providers");

  const load = useCallback(async (forceRefresh = false, silent = false) => {
    if (!silent) {
      setLoading(true);
      setError(null);
    }
    try {
      const data = await getUsageSpendSummary({ period, forceRefresh });
      setSummary(data);
      if (silent) setError(null);
    } catch (err: unknown) {
      if (!silent) setError(err instanceof Error ? err.message : String(err));
    } finally {
      if (!silent) setLoading(false);
    }
    // The OpenCodex flags and Track costs change what the backend scans.
  }, [period, trackCosts, includeOpenCodex, hideNativeCodex]);

  useEffect(() => {
    void load(false);
  }, [load]);

  // Upstream 0.55.0 #3106 parity: provider token/cost publications refresh the
  // pane silently while it is open. A settings save (Track costs, OpenCodex)
  // lands after the optimistic toggle, so rescan when it is persisted too.
  // Bursts are coalesced into one rebuild.
  useEffect(() => {
    const unlisteners: UnlistenFn[] = [];
    let timer: ReturnType<typeof setTimeout> | null = null;
    let cancelled = false;
    const schedule = () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        if (!cancelled) void load(false, true);
      }, 200);
    };
    for (const event of ["provider-updated", "settings-changed"]) {
      void listen(event, schedule)
        .then((fn) => {
          if (cancelled) fn();
          else unlisteners.push(fn);
        })
        .catch(() => {});
    }
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
      for (const fn of unlisteners) fn();
    };
  }, [load]);

  const tracking = trackCosts && summary?.costTrackingEnabled !== false;
  const visible = useMemo(
    () => (summary && tracking ? visibleUsageSpendSummary(summary, hidden) : null),
    [summary, tracking, hidden],
  );
  const contractHidden = summary ? isContractHidden(summary, hidden) : false;
  const periodTitle = costPeriodLabel(visible?.reportingPeriod ?? period, t);

  const tokenActivity = useMemo(
    () => (summary && tracking ? mergeTokenActivity(summary.tokenActivity, hidden) : []),
    [summary, tracking, hidden],
  );

  // The year of token activity drives the grid only; exports keep their period-scoped shape.
  const exportJson = visible ? JSON.stringify({ ...visible, tokenActivity: undefined }, null, 2) : null;

  const onShare = useCallback(() => {
    setShareError(null);
    const failure = shareUsageSpendPng(
      visible,
      t("UsageSpendTitle"),
      `codexbar-usage-spend-${visible?.reportingDay ?? "unknown"}.png`,
      periodTitle,
    );
    if (failure) setShareError(t(failure as LocaleKey));
  }, [visible, t, periodTitle]);

  const onCopyJson = useCallback(async () => {
    setShareError(null);
    if (!exportJson) {
      setShareError(t("UsageSpendShareEmpty"));
      return;
    }
    try {
      await navigator.clipboard.writeText(exportJson);
    } catch (cause: unknown) {
      setShareError(cause instanceof Error ? cause.message : String(cause));
    }
  }, [exportJson, t]);

  const onExportJson = useCallback(async () => {
    setShareError(null);
    if (!exportJson || !visible) {
      setShareError(t("UsageSpendShareEmpty"));
      return;
    }
    try {
      const path = await save({
        defaultPath: `codexbar-usage-spend-${visible.reportingDay}.json`,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      await writeUsageSpendExport(path, exportJson);
    } catch (cause: unknown) {
      setShareError(cause instanceof Error ? cause.message : String(cause));
    }
  }, [exportJson, visible, t]);

  const sources = spendSources(summary, hidden);
  const exportsDisabled = loading || !visible;

  return (
    <section className="settings-section usage-spend">
      <div className="usage-spend__header">
        <div>
          <h3 className="settings-section__title settings-section__title--bold">{t("UsageSpendTitle")}</h3>
          <p className="settings-section__caption">{t("UsageSpendCaption")}</p>
        </div>
        <button
          type="button"
          className="credential-btn credential-btn--secondary usage-spend__refresh"
          disabled={loading || !trackCosts}
          onClick={() => void load(true)}
        >
          <span aria-hidden="true">↻</span> {loading ? t("UsageSpendLoading") : t("UsageSpendRefresh")}
        </button>
      </div>

      {trackCosts && (
        <PeriodPicker value={period} disabled={saving} onChange={(next) => set({ spendDashboardPeriod: next })} t={t} />
      )}

      {error && (
        <div className="usage-spend__failures" role="alert">
          <strong>{t("UsageSpendRefreshFailures")}</strong>
          <span>{error}</span>
        </div>
      )}

      {!tracking ? (
        <div className="usage-spend__off" role="status">
          <strong>{t("UsageSpendTrackingOffTitle")}</strong>
          <p className="settings-section__caption">{t("UsageSpendTrackingOffBody")}</p>
        </div>
      ) : (
        visible && (
          <DashboardBody
            summary={visible}
            tokenActivity={tokenActivity}
            contractHidden={contractHidden}
            loading={loading}
            view={view}
            onView={setView}
            hidePersonalInfo={settings.hidePersonalInfo ?? false}
            t={t}
          />
        )
      )}

      <details className="usage-spend__controls" open>
        <summary>{t("UsageSpendDataControls")}</summary>
        <div className="settings-group__box">
          <Field label={t("UsageSpendTrackCosts")} description={t("UsageSpendTrackCostsHelper")}>
            <Toggle
              variant="switch"
              checked={trackCosts}
              disabled={saving}
              onChange={(value) => set({ costUsageEnabled: value })}
            />
          </Field>
          {/* Mac PreferencesSpendDashboardPane.swift:498-512: OpenCodex and the Sources
              filter only show while Track costs is on (#825). */}
          {trackCosts && (
            <>
              <Field label={t("UsageSpendOpenCodexImport")}>
                <Toggle
                  variant="switch"
                  checked={includeOpenCodex}
                  disabled={saving}
                  onChange={(value) => set({ openCodexUsageLogsEnabled: value })}
                />
              </Field>
              {includeOpenCodex && (
                <Field label={t("UsageSpendHideNativeCodex")}>
                  <Toggle
                    variant="switch"
                    checked={hideNativeCodex}
                    disabled={saving}
                    onChange={(value) => set({ hideNativeCodexCostWhenOpenCodexPresent: value })}
                  />
                </Field>
              )}
              {sources.length > 0 && (
                <fieldset className="usage-spend__sources">
                  <legend className="settings-field__label">{t("UsageSpendSources")}</legend>
                  {sources.map((source) => (
                    <Toggle
                      key={source.id}
                      label={source.displayName}
                      checked={!hidden.includes(source.id)}
                      disabled={saving}
                      onChange={(show) =>
                        set({ spendDashboardHiddenSourceIds: toggleHiddenSource(hidden, source.id, show) })
                      }
                    />
                  ))}
                </fieldset>
              )}
            </>
          )}
          <div className="usage-spend__exports">
            <button
              type="button"
              className="credential-btn credential-btn--secondary"
              disabled={exportsDisabled}
              onClick={() => void onCopyJson()}
            >
              {t("UsageSpendCopyJson")}
            </button>
            <button
              type="button"
              className="credential-btn credential-btn--secondary"
              disabled={exportsDisabled}
              onClick={() => void onExportJson()}
            >
              {t("UsageSpendSaveJson")}
            </button>
            <button
              type="button"
              className="credential-btn credential-btn--secondary usage-spend__share"
              disabled={exportsDisabled}
              onClick={onShare}
            >
              {t("UsageSpendShare")}
            </button>
          </div>
          {shareError && <p className="settings-section__error">{shareError}</p>}
        </div>
      </details>
    </section>
  );
}

/** The macOS 7d / 30d / 90d / Month to date / All segments; a legacy custom window gets its own segment. */
function PeriodPicker({
  value,
  disabled,
  onChange,
  t,
}: {
  value: string;
  disabled: boolean;
  onChange: (period: string) => void;
  t: Translate;
}) {
  const options: string[] = [...SPEND_DASHBOARD_PERIODS];
  if (!options.includes(value)) options.push(value);
  const label = (raw: string) =>
    parseCostPeriod(raw)?.kind === "monthToDate" ? t("CostPeriodMonthToDate") : costPeriodShortLabel(raw, t);
  return (
    <div className="provider-detail-segmented usage-spend__segmented" role="radiogroup" aria-label={t("UsageSpendPeriodLabel")}>
      {options.map((option) => (
        <button
          key={option}
          type="button"
          role="radio"
          aria-checked={option === value}
          title={costPeriodLabel(option, t)}
          className={`provider-detail-segmented__option${option === value ? " is-active" : ""}`}
          disabled={disabled}
          onClick={() => {
            if (option !== value) onChange(option);
          }}
        >
          {label(option)}
        </button>
      ))}
    </div>
  );
}

function DashboardBody({
  summary,
  tokenActivity,
  contractHidden,
  loading,
  view,
  onView,
  hidePersonalInfo,
  t,
}: {
  summary: UsageSpendSummary;
  tokenActivity: TokenActivityPoint[];
  contractHidden: boolean;
  loading: boolean;
  view: BreakdownView;
  onView: (view: BreakdownView) => void;
  hidePersonalInfo: boolean;
  t: Translate;
}) {
  const totals = spendTotalsByCurrency(summary.rows);
  const contract = summary.contract;
  const [selectedDay, setSelectedDay] = useState<string | null>(null);
  const views: { id: BreakdownView; label: string }[] = [
    { id: "providers", label: t("UsageSpendViewProviders") },
    { id: "projects", label: t("UsageSpendProjects") },
    { id: "sessions", label: t("UsageSpendViewSessions") },
  ];

  return (
    <>
      {totals.length === 0 && !loading && <p className="settings-section__caption">{t("UsageSpendEmpty")}</p>}
      {totals.map((entry) => (
        <div key={entry.currency} className="usage-spend__currency">
          <h4 className="usage-spend__currency-code">{entry.currency}</h4>
          <p className="settings-section__caption">{t("UsageSpendLocalHistory")}</p>
          <dl className="usage-spend__strip">
            <Stat label={t("UsageSpendEstimatedSpend")} value={formatUsd(entry.cost, entry.currency)} large />
            <Stat label={t("UsageSpendTrackedTokens")} value={formatCompactTokens(entry.tokens)} large />
            <Stat label={t("UsageSpendSources")} value={entry.sources.toLocaleString()} large />
          </dl>
        </div>
      ))}

      {/* Upstream v0.73.0 places the year of activity right after the currency groups. */}
      {tokenActivity.length > 0 && (
        <TokenActivityGrid points={tokenActivity} selectedDay={selectedDay} onSelectDay={setSelectedDay} t={t} />
      )}

      {!contractHidden && <TokenMix contract={contract} t={t} />}
      {!contractHidden && <CoverageLine contract={contract} t={t} />}

      <div className="usage-spend__card">
        <div className="provider-detail-segmented usage-spend__segmented" role="tablist" aria-label={t("UsageSpendBreakdownLabel")}>
          {views.map((option) => (
            <button
              key={option.id}
              type="button"
              role="tab"
              aria-selected={option.id === view}
              className={`provider-detail-segmented__option${option.id === view ? " is-active" : ""}`}
              onClick={() => onView(option.id)}
            >
              {option.label}
            </button>
          ))}
        </div>
        {view === "providers" && (
          <UsageSpendProviderGroups
            groups={buildSpendProviderRowGroups(summary.rows, contract)}
            caption={t("UsageSpendModelsPeriodCaption").replace("{}", costPeriodLabel(summary.reportingPeriod, t))}
            showHeading={false}
            t={t}
          />
        )}
        {view === "projects" && (
          <ProjectsPanel contract={contract} hidePersonalInfo={hidePersonalInfo} t={t} />
        )}
        {view === "sessions" &&
          (contract.conversations.length === 0 ? (
            <p className="settings-section__caption">{t("UsageSpendNoSessions")}</p>
          ) : (
            <SessionInspector
              conversations={contract.conversations}
              historyDays={contract.historyDays}
              t={t}
            />
          ))}
      </div>

      {summary.trend && summary.trend.sources.length > 0 && (
        <UsageSpendTrendPanel trend={summary.trend} t={t} renderSourceIcon={renderSpendSourceIcon} />
      )}
      {!contractHidden && <UsageSpendDailyLedger daily={contract.daily} />}
    </>
  );
}

/** Brand artwork beside each explorer legend entry and inspector row (upstream chart legend). */
function renderSpendSourceIcon(source: SpendTrendSource) {
  return <SpendProviderIcon providerId={source.providerId} />;
}

function Stat({ label, value, large }: { label: string; value: string; large?: boolean }) {
  return (
    <div className={`usage-spend__stat${large ? " usage-spend__stat--large" : ""}`}>
      <dt>{label}</dt>
      <dd>{value}</dd>
    </div>
  );
}

/** Five-cell token mix for the detail contract; a source that never records a kind shows "—". */
function TokenMix({ contract, t }: { contract: SpendContract; t: Translate }) {
  const mix = contract.tokenMix;
  const cells: [LocaleKey, number | null][] = [
    ["UsageSpendTokenInput", mix.inputTokens],
    ["UsageSpendTokenOutput", mix.outputTokens],
    ["UsageSpendTokenCacheRead", mix.cacheReadTokens],
    ["UsageSpendTokenCacheWrite", mix.cacheCreationTokens],
    ["UsageSpendTokenReasoning", mix.reasoningTokens],
  ];
  if (cells.every(([, value]) => value == null)) return null;
  return (
    <dl className="usage-spend__mix" aria-label={t("UsageSpendTokenMix")}>
      {cells.map(([key, value]) => (
        <Stat key={key} label={t(key)} value={formatCompactTokens(value)} />
      ))}
    </dl>
  );
}

function CoverageLine({ contract, t }: { contract: SpendContract; t: Translate }) {
  const coverage = contract.priceCoverage;
  const counted = coverage.priced + coverage.unpriced + coverage.unmetered + coverage.estimated;
  if (counted === 0 && contract.provenance === "unknown") return null;
  const provenance = contract.provenance === "listPriceEstimate"
    ? t("UsageSpendApiEstimate")
    : contract.provenance === "vendorMetered"
      ? t("UsageSpendVendorMetered")
      : contract.provenance === "mixed"
        ? t("UsageSpendMixedSources")
        : t("UsageSpendUnknown");
  const parts = [
    `${t("UsageSpendPriced")} ${coverage.priced}`,
    `${t("UsageSpendUnpricedCount")} ${coverage.unpriced}`,
    `${t("UsageSpendUnmetered")} ${coverage.unmetered}`,
    `${t("UsageSpendEstimated")} ${coverage.estimated}`,
    provenance,
  ];
  if (contract.customPricingActive) parts.push(t("UsageSpendCustomPricingActive"));
  return <p className="settings-section__caption usage-spend__coverage">{parts.join(" · ")}</p>;
}

/** Rows shown before the show-all control, for projects and for independent chats alike. */
const PROJECT_INITIAL_ROWS = 8;

function ProjectsPanel({
  contract,
  hidePersonalInfo,
  t,
}: {
  contract: SpendContract;
  hidePersonalInfo: boolean;
  t: Translate;
}) {
  // Independent chats are conversations with no project folder (upstream 0.72.0);
  // they get their own region so they are not counted as projects.
  const projectRows = contract.projects.filter((project) => !project.isProjectless);
  const chatRows = contract.projects.filter((project) => project.isProjectless);
  const partial = contract.projectSourceStatus != null && contract.projectSourceStatus !== "complete";

  return (
    <div className="settings-section__group">
      <p className="settings-section__caption" style={{ marginTop: 4 }}>
        {t("UsageSpendProjectsPeriodCaption").replace("{}", costPeriodLabel(contract.reportingPeriod, t))}
        {partial ? ` · ${t("UsageSpendPartialHistory")}` : ""}.
      </p>

      {projectRows.length === 0 ? (
        <p className="settings-section__caption">{t("UsageSpendNoProjects")}</p>
      ) : (
        <ProjectRowList
          rows={projectRows}
          conversations={contract.conversations}
          historyDays={contract.historyDays}
          hidePersonalInfo={hidePersonalInfo}
          t={t}
        />
      )}

      {chatRows.length > 0 && (
        <div role="region" aria-label={t("UsageSpendIndependentChats")} style={{ marginTop: 16 }}>
          <h4 style={{ margin: "0 0 4px" }}>{t("UsageSpendIndependentChats")}</h4>
          <ProjectRowList
            rows={chatRows}
            conversations={contract.conversations}
            historyDays={contract.historyDays}
            hidePersonalInfo={hidePersonalInfo}
            independent
            t={t}
          />
        </div>
      )}
    </div>
  );
}

function ProjectRowList({
  rows,
  conversations,
  historyDays,
  hidePersonalInfo,
  independent = false,
  t,
}: {
  rows: CodexWorkspacesProjectUsage[];
  conversations: CodexWorkspacesSessionUsage[];
  historyDays: number;
  hidePersonalInfo: boolean;
  independent?: boolean;
  t: Translate;
}) {
  const [showAll, setShowAll] = useState(false);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const visible = showAll ? rows : rows.slice(0, PROJECT_INITIAL_ROWS);

  return (
    <>
      <div style={{ display: "grid", gap: 6, marginTop: 10 }}>
        {visible.map((project) => {
          const isExpanded = expanded.has(project.id);
          const partialCost = project.costEstimate.unknownTokens > 0;
          const path = hidePersonalInfo ? null : project.path;
          const sharesName = rows.some((other) => other.id !== project.id && other.displayName === project.displayName);
          return (
            <div key={project.id} className="provider-detail-section" style={{ padding: "10px 12px" }}>
              <button
                type="button"
                onClick={() =>
                  setExpanded((current) => {
                    const next = new Set(current);
                    if (next.has(project.id)) next.delete(project.id);
                    else next.add(project.id);
                    return next;
                  })
                }
                aria-expanded={isExpanded}
                className="usage-spend__row-button"
              >
                <span style={{ minWidth: 0 }}>
                  <strong title={path ?? project.displayName}>{project.displayName}</strong>
                  <span className="settings-section__caption" style={{ display: "block" }}>
                    {independent ? `${t("UsageSpendIndependentChat")} · ` : ""}
                    {project.sessionCount} {t("UsageSpendConversations")}
                    {project.topModel ? ` · ${project.topModel}` : ""}
                  </span>
                  {path && sharesName && (
                    <span className="settings-section__caption usage-spend__project-path" title={path}>
                      {path}
                    </span>
                  )}
                </span>
                <span>{partialCost ? "~" : ""}${project.costEstimate.knownUsd.toFixed(2)}</span>
                <span aria-hidden="true">{isExpanded ? "▾" : "▸"}</span>
              </button>

              {isExpanded && (
                <ProjectConversations
                  conversations={conversations.filter((session) => session.projectId === project.id)}
                  historyDays={historyDays}
                  t={t}
                />
              )}
            </div>
          );
        })}
      </div>
      {rows.length > PROJECT_INITIAL_ROWS && (
        <button
          type="button"
          className="credential-btn credential-btn--secondary"
          style={{ marginTop: 8 }}
          aria-expanded={showAll}
          onClick={() => setShowAll((value) => !value)}
        >
          {showAll ? t("UsageSpendShowLess") : `${t("UsageSpendShowAll")} (${rows.length})`}
        </button>
      )}
    </>
  );
}
