import { useState } from "react";
import { SpendProviderIcon } from "../../../components/providers/SpendProviderIcon";
import { formatSpendMetric, formatUsd } from "../../../lib/usageSpendSharing";
import {
  PROVIDER_MODEL_DISPLAY_LIMIT,
  type SpendProviderGroup,
  showsSpendModelsLabel,
} from "../../../lib/usageSpendBreakdown";
import type { LocaleKey } from "../../../i18n/keys";

interface Props {
  groups: SpendProviderGroup[];
  /** Names the History window the model rows cover. */
  caption: string;
  t: (key: LocaleKey) => string;
  /** False inside the Providers view, where the picker already names the list. */
  showHeading?: boolean;
}

/**
 * Upstream 0.64.0 (#3353): models grouped by provider, one collapsible group
 * per provider with its spend and its models.
 */
export function UsageSpendProviderGroups({ groups, caption, t, showHeading = true }: Props) {
  const [collapsed, setCollapsed] = useState<Set<string>>(() => new Set());
  const [expandedModels, setExpandedModels] = useState<Set<string>>(() => new Set());
  const toggle = (set: Set<string>, id: string) => {
    const next = new Set(set);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    return next;
  };

  return (
    <div className="settings-section__group" style={{ marginTop: showHeading ? 20 : 0 }}>
      {showHeading && <h4 style={{ margin: 0 }}>{t("UsageSpendModels")}</h4>}
      <p className="settings-section__caption">{caption}</p>
      {groups.length === 0 ? (
        <p className="settings-section__caption">{t("UsageSpendNoModels")}</p>
      ) : (
        <div style={{ display: "grid", gap: 6, marginTop: 10 }}>
          {groups.map((group) => {
            const isCollapsed = collapsed.has(group.providerId);
            const showAll = expandedModels.has(group.providerId);
            const models = showAll ? group.models : group.models.slice(0, PROVIDER_MODEL_DISPLAY_LIMIT);
            return (
              <div key={group.providerId} className="provider-detail-section usage-spend-provider">
                <button
                  type="button"
                  aria-expanded={!isCollapsed}
                  onClick={() => setCollapsed((current) => toggle(current, group.providerId))}
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
                  <span className="usage-spend-provider__name">
                    <SpendProviderIcon providerId={group.providerId} />
                    <strong>{group.displayName}</strong>
                  </span>
                  <span>{groupMetric(group, t("UsageSpendTokens"))}</span>
                  <span aria-hidden="true">{group.models.length === 0 ? "" : isCollapsed ? "▸" : "▾"}</span>
                </button>
                {group.row && <RowDetail row={group.row} t={t} />}
                {!isCollapsed && group.models.length > 0 && (
                  <div style={{ display: "grid", gap: 0, marginTop: 8 }}>
                    {!showHeading && showsSpendModelsLabel(group) && (
                      <span className="settings-group__title usage-spend-provider__subsection">{t("UsageSpendModels")}</span>
                    )}
                    {group.hasPartialModelHistory && (
                      <span className="settings-section__caption">{t("UsageSpendPartialModelBreakdown")}</span>
                    )}
                    {models.map((model) => (
                      <div key={model.model} className="usage-spend-provider__model">
                        <SpendProviderIcon providerId={group.providerId} style="monochrome" />
                        <span style={{ minWidth: 0 }}>
                          {model.model}
                          <span className="settings-section__caption" style={{ display: "block", margin: 0 }}>
                            {model.totalTokens.toLocaleString()} {t("UsageSpendTokens")}
                            {model.customPricing ? " · " + t("UsageSpendCustomPricing") : ""}
                          </span>
                        </span>
                        <span>{model.costUsd == null ? t("UsageSpendUnpriced") : formatUsd(model.costUsd, "USD")}</span>
                      </div>
                    ))}
                    {group.models.length > PROVIDER_MODEL_DISPLAY_LIMIT && (
                      <button
                        type="button"
                        className="credential-btn credential-btn--secondary"
                        style={{ justifySelf: "start" }}
                        onClick={() => setExpandedModels((current) => toggle(current, group.providerId))}
                      >
                        {showAll ? t("UsageSpendShowLess") : `${t("UsageSpendShowAll")} (${group.models.length})`}
                      </button>
                    )}
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

/** Windows detail kept from the old provider table: the fixed 7 and 30 day values and the source. */
function RowDetail({ row, t }: { row: NonNullable<SpendProviderGroup["row"]>; t: (key: LocaleKey) => string }) {
  const tokens = t("UsageSpendTokens");
  const known = t("UsageSpendKnownSubtotal");
  const currency = row.currency || "USD";
  const sevenDay = formatSpendMetric(row.sevenDay, row.sevenDayTokens, currency, tokens, row.sevenDayEstimate?.knownSubtotalUsd, known, row.sevenDayTokensLowerBound);
  const thirtyDay = formatSpendMetric(row.thirtyDay, row.thirtyDayTokens, currency, tokens, row.thirtyDayEstimate?.knownSubtotalUsd, known, row.thirtyDayTokensLowerBound);
  return (
    <span className="settings-section__caption usage-spend-provider__detail">
      {t("UsageSpendCol7d")}: {sevenDay} · {t("UsageSpendCol30d")}: {thirtyDay} · {row.source}
      {row.refreshing && (
        <span title={row.staleUpdatedAt ? `Stale as of ${row.staleUpdatedAt}` : undefined}>
          {" · "}{t("UsageSpendRefreshing")}
        </span>
      )}
    </span>
  );
}

function groupMetric(group: SpendProviderGroup, tokenLabel: string): string {
  const parts: string[] = [];
  if (group.costUsd != null) parts.push(`${group.costIsPartial ? "~" : ""}${formatUsd(group.costUsd, group.currency ?? "USD")}`);
  if (group.totalTokens != null) parts.push(`${group.totalTokens.toLocaleString()} ${tokenLabel}`);
  return parts.length > 0 ? parts.join(" · ") : "—";
}
