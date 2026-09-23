import { useCallback, useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useLocale } from "../../../hooks/useLocale";
import { useCurrency } from "../../../hooks/CurrencyProvider";
import { convertCurrencyAmount, normalizePreferredCurrency } from "../../../lib/currency";
import {
  getSettingsSnapshot,
  getUsageSpendSummary,
  updateSettings,
  writeUsageSpendExport,
} from "../../../lib/tauri";
import {
  shareUsageSpendPng,
} from "../../../lib/usageSpendSharing";
import type { CostSummaryDisplayStyle, SettingsSnapshot, SpendContract, UsageSpendSummary } from "../../../types/bridge";
import type { LocaleKey } from "../../../i18n/keys";
import type { TabProps } from "../settingsTabs";

export default function UsageSpendTab(_props: TabProps) {
  const { t } = useLocale();
  const { format, preferredCode, rates } = useCurrency();
  const [summary, setSummary] = useState<UsageSpendSummary | null>(null);
  const [selectedDays, setSelectedDays] = useState<0 | 7 | 30>(30);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [shareError, setShareError] = useState<string | null>(null);
  const [includeOpenCodex, setIncludeOpenCodex] = useState(false);
  const [hideNativeCodex, setHideNativeCodex] = useState(false);
  const [showAllModels, setShowAllModels] = useState(false);
  const [showAllProjects, setShowAllProjects] = useState(false);
  const [expandedProjects, setExpandedProjects] = useState<Set<string>>(() => new Set());
  const tableRef = useRef<HTMLTableElement | null>(null);

  useEffect(() => {
    void getSettingsSnapshot()
      .then((settings) => {
        setIncludeOpenCodex(settings.openCodexUsageLogsEnabled ?? false);
        setHideNativeCodex(settings.hideNativeCodexCostWhenOpenCodexPresent ?? false);
      })
      .catch(() => {
        // Keep the safe default (off) if settings cannot be loaded.
      });
  }, []);

  const load = useCallback(async (forceRefresh = false, silent = false) => {
    if (!silent) {
      setLoading(true);
      setError(null);
    }
    try {
      const data = await getUsageSpendSummary({ historyDays: selectedDays, forceRefresh });
      setSummary(data);
    } catch (err: unknown) {
      if (!silent) setError(err instanceof Error ? err.message : String(err));
    } finally {
      if (!silent) setLoading(false);
    }
  }, [hideNativeCodex, includeOpenCodex, selectedDays]);

  useEffect(() => {
    load(false);
  }, [load]);

  // Upstream 0.55.0 #3106 parity: provider token/cost publications refresh
  // Usage & Spend silently while the pane is open. Coalesce bursts so a
  // multi-provider refresh does not trigger one dashboard rebuild per event.
  useEffect(() => {
    let unlisten: UnlistenFn | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let cancelled = false;
    void listen("provider-updated", () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        if (!cancelled) void load(false, true);
      }, 200);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    }).catch(() => {});
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
      unlisten?.();
    };
  }, [load]);

  const onShare = useCallback(() => {
    setShareError(null);
    const error = shareUsageSpendPng(
      summary,
      t("UsageSpendTitle"),
      `codexbar-usage-spend-${summary?.reportingDay ?? "unknown"}.png`,
      {
        formatMetric: (cost, tokens, currency, tokenLabel) =>
          formatSpendDisplayMetric(cost, tokens, currency, tokenLabel, format),
        displayCurrency: (sourceCurrency) => spendDisplayCurrency(sourceCurrency, preferredCode, rates),
      },
    );
    if (error) setShareError(t(error as LocaleKey));
  }, [format, preferredCode, rates, summary, t]);

  const onCopyJson = useCallback(async () => {
    setShareError(null);
    if (!summary) {
      setShareError(t("UsageSpendShareEmpty"));
      return;
    }
    try {
      await navigator.clipboard.writeText(JSON.stringify(summary, null, 2));
    } catch (cause: unknown) {
      setShareError(cause instanceof Error ? cause.message : String(cause));
    }
  }, [summary, t]);

  const onSaveJson = useCallback(async () => {
    setShareError(null);
    if (!summary) {
      setShareError(t("UsageSpendShareEmpty"));
      return;
    }
    try {
      const stamp = summary.reportingDay;
      const path = await save({
        defaultPath: `codexbar-usage-spend-${stamp}.json`,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      await writeUsageSpendExport(path, JSON.stringify(summary, null, 2));
    } catch (cause: unknown) {
      setShareError(cause instanceof Error ? cause.message : String(cause));
    }
  }, [summary, t]);

  return (
    <section className="settings-section">
      <h3 className="settings-section__title settings-section__title--bold">
        {t("UsageSpendTitle")}
      </h3>
      <p className="settings-section__caption">{t("UsageSpendCaption")}</p>

      <div className="settings-section__group" style={{ marginBottom: 12, display: "flex", gap: 8 }}>
        <button
          type="button"
          className="credential-btn credential-btn--secondary"
          disabled={loading}
          onClick={() => load(true)}
        >
          {loading ? t("UsageSpendLoading") : t("UsageSpendRefresh")}
        </button>
        <button
          type="button"
          className="credential-btn credential-btn--secondary"
          disabled={loading || !summary}
          onClick={onShare}
        >
          {t("UsageSpendShare")}
        </button>
        <button
          type="button"
          className="credential-btn credential-btn--secondary"
          disabled={loading || !summary}
          onClick={() => void onCopyJson()}
        >
          {t("UsageSpendCopyJson")}
        </button>
        <button
          type="button"
          className="credential-btn credential-btn--secondary"
          disabled={loading || !summary}
          onClick={() => void onSaveJson()}
        >
          {t("UsageSpendSaveJson")}
        </button>
      </div>

      <div className="settings-section__group" style={{ marginBottom: 12, display: "flex", gap: 8 }}>
        {([7, 30, 0] as const).map((days) => (
          <button
            key={days}
            type="button"
            className="credential-btn credential-btn--secondary"
            aria-pressed={selectedDays === days}
            onClick={() => setSelectedDays(days)}
          >
            {days === 0 ? t("UsageSpendAllTime") : `${days}d`}
          </button>
        ))}
        <label style={{ display: "inline-flex", alignItems: "center", gap: 6, marginLeft: 8 }}>
          <input
            type="checkbox"
            checked={includeOpenCodex}
            onChange={(event) => {
              const enabled = event.target.checked;
              setIncludeOpenCodex(enabled);
              void updateSettings({ openCodexUsageLogsEnabled: enabled }).catch((err: unknown) => {
                setIncludeOpenCodex(!enabled);
                setError(err instanceof Error ? err.message : String(err));
              });
            }}
          />
          {t("UsageSpendOpenCodexImport")}
        </label>
        {includeOpenCodex && (
          <label style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
            <input
              type="checkbox"
              checked={hideNativeCodex}
              onChange={(event) => {
                const enabled = event.target.checked;
                setHideNativeCodex(enabled);
                void updateSettings({ hideNativeCodexCostWhenOpenCodexPresent: enabled }).catch((err: unknown) => {
                  setHideNativeCodex(!enabled);
                  setError(err instanceof Error ? err.message : String(err));
                });
              }}
            />
            {t("UsageSpendHideNativeCodex")}
          </label>
        )}
      </div>

      <CostSummaryStyleControl t={t} />

      {error && <p className="settings-section__error">{error}</p>}
      {shareError && <p className="settings-section__error">{shareError}</p>}

      {!error && (
        <table className="usage-spend-table" ref={tableRef}>
          <thead>
            <tr>
              <th>{t("UsageSpendColProvider")}</th>
              <th>{t("UsageSpendCol7d")}</th>
              <th>{t("UsageSpendCol30d")}</th>
              <th>{t("UsageSpendColCurrency")}</th>
              <th>{t("UsageSpendColSource")}</th>
            </tr>
          </thead>
          <tbody>
            {(summary?.rows ?? []).map((row) => (
              <tr key={row.providerId}>
                <td>{row.displayName}</td>
                <td>{formatSpendDisplayMetric(row.sevenDay, row.sevenDayTokens, row.currency, t("UsageSpendTokens"), format)}</td>
                <td>{formatSpendDisplayMetric(row.thirtyDay, row.thirtyDayTokens, row.currency, t("UsageSpendTokens"), format)}</td>
                <td>{spendDisplayCurrency(row.currency || "USD", preferredCode, rates)}</td>
                <td className="usage-spend-table__source">
                  {row.source}
                  {row.refreshing && (
                    <span className="usage-spend-table__refreshing" title={row.staleUpdatedAt ? `Stale as of ${row.staleUpdatedAt}` : undefined}>
                      {" · "}{t("UsageSpendRefreshing")}
                    </span>
                  )}
                </td>
              </tr>
            ))}
            {!loading && (summary?.rows?.length ?? 0) === 0 && (
              <tr>
                <td colSpan={5}>{t("UsageSpendEmpty")}</td>
              </tr>
            )}
          </tbody>
        </table>
      )}

      {!error && summary && <SpendContractOverview contract={summary.contract} t={t} />}

      {!error && summary && (
        <ContractModelsPanel contract={summary.contract} showAll={showAllModels} onToggleAll={() => setShowAllModels((value) => !value)} t={t} />
      )}

      {!error && summary && (
        <ProjectsPanel
          contract={summary.contract}
          showAll={showAllProjects}
          expanded={expandedProjects}
          onToggleAll={() => setShowAllProjects((value) => !value)}
          t={t}
          onToggleProject={(id) => {
            setExpandedProjects((current) => {
              const next = new Set(current);
              if (next.has(id)) next.delete(id);
              else next.add(id);
              return next;
            });
          }}
        />
      )}
    </section>
  );
}

function formatSpendDisplayMetric(
  cost: number | null | undefined,
  tokens: number | null | undefined,
  currency: string,
  tokenLabel: string,
  format: (amount: number | null | undefined, sourceCode: string, sourceSymbol?: string | null) => string,
): string {
  const parts: string[] = [];
  if (cost != null && Number.isFinite(cost)) parts.push(format(cost, currency || "USD"));
  if (tokens != null && Number.isFinite(tokens)) parts.push(`${Math.max(0, tokens).toLocaleString()} ${tokenLabel}`);
  return parts.length > 0 ? parts.join(" · ") : "—";
}

function spendDisplayCurrency(source: string, preferred: string, rates: Record<string, number>): string {
  const target = normalizePreferredCurrency(preferred);
  if (target !== "AUTO" && convertCurrencyAmount(1, source, target, rates) != null) return target;
  return source;
}



function SpendContractOverview({ contract, t }: { contract: SpendContract; t: (key: LocaleKey) => string }) {
  const { format } = useCurrency();
  const coverage = contract.priceCoverageRatio == null
    ? t("UsageSpendUnknown")
    : `${Math.round(contract.priceCoverageRatio * 100)}%`;
  const provenance = contract.provenance === "listPriceEstimate"
    ? t("UsageSpendApiEstimate")
    : contract.provenance === "vendorMetered"
      ? t("UsageSpendVendorMetered")
      : contract.provenance === "mixed"
        ? t("UsageSpendMixedSources")
        : t("UsageSpendUnknown");
  const total = contract.knownCostUsd == null
    ? "—"
    : `${contract.priceCoverage.unpriced > 0 ? "~" : ""}${format(contract.knownCostUsd, "USD")}`;
  const tokenParts = [
    contract.tokenMix.inputTokens == null ? null : `${contract.tokenMix.inputTokens.toLocaleString()} input`,
    contract.tokenMix.outputTokens == null ? null : `${contract.tokenMix.outputTokens.toLocaleString()} output`,
    contract.tokenMix.cacheReadTokens == null ? null : `${contract.tokenMix.cacheReadTokens.toLocaleString()} cached`,
  ].filter(Boolean).join(" · ");

  return (
    <div className="settings-section__group" style={{ marginTop: 16 }}>
      <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(150px, 1fr))", gap: 8 }}>
        <MetricCard label={t("UsageSpendSpend")} value={total} detail={contract.knownZero ? t("UsageSpendKnownZero") : provenance} />
        <MetricCard label={t("UsageSpendPriceCoverage")} value={coverage} detail={`${contract.priceCoverage.unpriced} ${t("UsageSpendUnpriced")}`} />
        <MetricCard label={t("UsageSpendConversations")} value={contract.conversationCount.toLocaleString()} detail={contract.historyCoverageEstablished ? t("UsageSpendHistoryCovered") : t("UsageSpendPartialHistory")} />
        <MetricCard label={t("UsageSpendTokenMix")} value={tokenParts || "—"} detail={contract.customPricingActive ? t("UsageSpendCustomPricingActive") : t("UsageSpendDefaultPricing")} />
      </div>
      <ActivityHeatmap cells={contract.hourlyActivity} t={t} />
      {contract.imports.map((source) => (
        <p key={source.sourceId} className="settings-section__caption" style={{ marginTop: 8 }}>
          {source.displayName}: {source.requestCount.toLocaleString()} {t("UsageSpendRequests")} · {source.conversationCount.toLocaleString()} {t("UsageSpendConversations").toLowerCase()}
        </p>
      ))}
    </div>
  );
}

function MetricCard({ label, value, detail }: { label: string; value: string; detail: string }) {
  return (
    <div className="provider-detail-section" style={{ padding: "10px 12px" }}>
      <span className="settings-section__caption">{label}</span>
      <strong style={{ display: "block", marginTop: 3 }}>{value}</strong>
      <span className="settings-section__caption">{detail}</span>
    </div>
  );
}

function ActivityHeatmap({ cells, t }: { cells: SpendContract["hourlyActivity"]; t: (key: LocaleKey) => string }) {
  const lookup = new Map(cells.map((cell) => [`${cell.weekday}:${cell.hour}`, cell.conversations]));
  const max = Math.max(1, ...cells.map((cell) => cell.conversations));
  return (
    <div style={{ marginTop: 14 }}>
      <h4 style={{ margin: "0 0 8px" }}>{t("UsageSpendHourlyActivity")}</h4>
      <div
        aria-label={t("UsageSpendHourlyActivity")}
        style={{ display: "grid", gridTemplateColumns: "repeat(24, minmax(7px, 1fr))", gap: 2 }}
      >
        {Array.from({ length: 7 * 24 }, (_, index) => {
          const weekday = Math.floor(index / 24);
          const hour = index % 24;
          const value = lookup.get(`${weekday}:${hour}`) ?? 0;
          const alpha = value === 0 ? 0.08 : 0.18 + (value / max) * 0.72;
          return (
            <span
              key={`${weekday}:${hour}`}
              title={`Day ${weekday + 1}, ${hour}:00 · ${value} conversations`}
              style={{ aspectRatio: "1", borderRadius: 2, background: `rgb(90 160 255 / ${alpha})` }}
            />
          );
        })}
      </div>
    </div>
  );
}

function ContractModelsPanel({ contract, showAll, onToggleAll, t }: { contract: SpendContract; showAll: boolean; onToggleAll: () => void; t: (key: LocaleKey) => string }) {
  const { format } = useCurrency();
  const visible = showAll ? contract.models : contract.models.slice(0, 8);
  return (
    <div className="settings-section__group" style={{ marginTop: 20 }}>
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 12 }}>
        <div>
          <h4 style={{ margin: 0 }}>{t("UsageSpendModels")}</h4>
          <p className="settings-section__caption">{t("UsageSpendAllTimeHistory")} {contract.historyDays} days of local history.</p>
        </div>
        {contract.models.length > 8 && (
          <button type="button" className="credential-btn credential-btn--secondary" onClick={onToggleAll}>
            {showAll ? t("UsageSpendShowLess") : t("UsageSpendShowAll") + " (" + contract.models.length + ")"}
          </button>
        )}
      </div>
      {visible.length === 0 ? (
        <p className="settings-section__caption">{t("UsageSpendNoModels")}</p>
      ) : (
        <div style={{ display: "grid", gap: 6, marginTop: 10 }}>
          {visible.map((model) => (
            <div key={model.model} className="provider-detail-section" style={{ padding: "10px 12px", display: "grid", gridTemplateColumns: "minmax(0, 1fr) auto", gap: 12 }}>
              <span>
                <strong>{model.model}</strong>
                <span className="settings-section__caption" style={{ display: "block" }}>
                  {model.totalTokens.toLocaleString()} {t("UsageSpendTokens")}{model.customPricing ? " · " + t("UsageSpendCustomPricing") : ""}
                </span>
              </span>
              <span>{model.costUsd == null ? t("UsageSpendUnpriced") : format(model.costUsd, "USD")}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function ProjectsPanel({
  contract,
  showAll,
  expanded,
  onToggleAll,
  onToggleProject,
  t,
}: {
  contract: SpendContract;
  showAll: boolean;
  expanded: Set<string>;
  onToggleAll: () => void;
  onToggleProject: (id: string) => void;
  t: (key: LocaleKey) => string;
}) {
  const { format } = useCurrency();
  const projects = showAll ? contract.projects : contract.projects.slice(0, 8);
  const partial = contract.projectSourceStatus != null && contract.projectSourceStatus !== "complete";

  return (
    <div className="settings-section__group" style={{ marginTop: 20 }}>
      <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between", gap: 12 }}>
        <div>
          <h4 style={{ margin: 0 }}>{t("UsageSpendProjects")}</h4>
          <p className="settings-section__caption" style={{ marginTop: 4 }}>
            Ranked Codex local project spend for the last {contract.historyDays} days
            {partial ? ` · ${t("UsageSpendPartialHistory")}` : ""}.
          </p>
        </div>
        {contract.projects.length > 8 && (
          <button type="button" className="credential-btn credential-btn--secondary" onClick={onToggleAll}>
            {showAll ? t("UsageSpendShowLess") : `${t("UsageSpendShowAll")} (${contract.projects.length})`}
          </button>
        )}
      </div>

      {projects.length === 0 ? (
        <p className="settings-section__caption">{t("UsageSpendNoProjects")}</p>
      ) : (
        <div style={{ display: "grid", gap: 6, marginTop: 10 }}>
          {projects.map((project) => {
            const isExpanded = expanded.has(project.id);
            const partialCost = project.costEstimate.unknownTokens > 0;
            return (
              <div key={project.id} className="provider-detail-section" style={{ padding: "10px 12px" }}>
                <button
                  type="button"
                  onClick={() => onToggleProject(project.id)}
                  aria-expanded={isExpanded}
                  style={{
                    width: "100%",
                    display: "grid",
                    gridTemplateColumns: "minmax(0, 1fr) auto auto",
                    gap: 12,
                    alignItems: "center",
                    border: 0,
                    padding: 0,
                    background: "transparent",
                    color: "inherit",
                    textAlign: "left",
                    cursor: "pointer",
                  }}
                >
                  <span style={{ minWidth: 0 }}>
                    <strong>{project.displayName}</strong>
                    <span className="settings-section__caption" style={{ display: "block" }}>
                      {project.sessionCount} {t("UsageSpendConversations")}
                      {project.topModel ? ` · ${project.topModel}` : ""}
                    </span>
                  </span>
                  <span>{partialCost ? "~" : ""}{format(project.costEstimate.knownUsd, "USD")}</span>
                  <span aria-hidden="true">{isExpanded ? "▾" : "▸"}</span>
                </button>

                {isExpanded && (
                  <div style={{ display: "grid", gap: 5, marginTop: 9, paddingTop: 8, borderTop: "1px solid var(--border-subtle)" }}>
                    {contract.conversations
                      .filter((session) => session.projectId === project.id)
                      .map((session) => (
                        <div
                          key={session.id}
                          style={{ display: "grid", gridTemplateColumns: "minmax(0, 1fr) auto", gap: 10 }}
                        >
                          <span style={{ minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                            {session.displayTitle}
                          </span>
                          <span>
                            {session.costEstimate.unknownTokens > 0 ? "~" : ""}
                            {format(session.costEstimate.knownUsd, "USD")}
                          </span>
                        </div>
                      ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function CostSummaryStyleControl({ t }: { t: (key: LocaleKey) => string }) {
  const [style, setStyle] = useState<CostSummaryDisplayStyle>("compact");
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    void getSettingsSnapshot().then((snap: SettingsSnapshot) => {
      setStyle(snap.costSummaryDisplayStyle);
      setLoading(false);
    }).catch(() => setLoading(false));
  }, []);

  const handleChange = useCallback(async (value: CostSummaryDisplayStyle) => {
    setStyle(value);
    try {
      await updateSettings({ costSummaryDisplayStyle: value });
    } catch {
      /* best-effort; revert handled by next settings refresh */
    }
  }, []);

  const options: { value: CostSummaryDisplayStyle; label: string }[] = [
    { value: "compact", label: t("CostSummaryStyleCompact") },
    { value: "detailed", label: t("CostSummaryStyleDetailed") },
    { value: "hidden", label: t("CostSummaryStyleHidden") },
  ];

  return (
    <div className="settings-section__group" style={{ marginBottom: 16 }}>
      <label className="settings-section__label" htmlFor="cost-summary-style">
        {t("CostSummaryDisplayStyle")}
      </label>
      <p className="settings-section__caption">{t("CostSummaryDisplayStyleHelper")}</p>
      <select
        id="cost-summary-style"
        className="settings-select"
        value={style}
        disabled={loading}
        onChange={(e) => void handleChange(e.target.value as CostSummaryDisplayStyle)}
      >
        {options.map((opt) => (
          <option key={opt.value} value={opt.value}>
            {opt.label}
          </option>
        ))}
      </select>
    </div>
  );
}
