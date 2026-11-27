import { Fragment } from "react";
import type { ProviderDisplayDetailSnapshot, ProviderInventoryItemSnapshot } from "../../types/bridge";
import { useLocale } from "../../hooks/useLocale";
import { useFormattedResetTime } from "../../hooks/useFormattedResetTime";
import { groupProviderDisplayDetails } from "../ProviderDisplayRow";
import { localizeProviderText } from "../../lib/providerText";
import { localizeProviderLabel } from "../../lib/windowLabels";
import { isDetailSectionVisible } from "../../lib/usageItemVisibility";

function progressPercent(detail: ProviderDisplayDetailSnapshot): number | null {
  const progress = detail.progress;
  if (
    !progress ||
    !Number.isFinite(progress.used) ||
    !Number.isFinite(progress.total) ||
    progress.total <= 0
  ) {
    return null;
  }
  return Math.max(0, Math.min(100, (progress.used / progress.total) * 100));
}

function KeyValueRow({
  label,
  value,
  secondary,
  percent,
}: {
  label: string;
  value: string;
  secondary: string | null;
  percent: number | null;
}) {
  return (
    <div className="card-kv__item">
      <div className="card-kv__row">
        <span className="card-kv__label">{label}</span>
        <span className="card-kv__values">
          <span className="card-kv__value">{value}</span>
          {secondary && <span className="card-kv__secondary">{secondary}</span>}
        </span>
      </div>
      {percent != null && (
        <div
          className="card-kv__track"
          role="progressbar"
          aria-label={label}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
        >
          <div className="card-kv__fill" style={{ width: `${percent}%` }} />
        </div>
      )}
    </div>
  );
}

function InventoryRow({
  item,
  resetTimeRelative,
}: {
  item: ProviderInventoryItemSnapshot;
  resetTimeRelative: boolean;
}) {
  const { t } = useLocale();
  const expiry = useFormattedResetTime(item.nextExpiresAt, null, resetTimeRelative, "expires");
  return (
    <KeyValueRow
      label={localizeProviderLabel(item.title, t)}
      value={t("InventoryAvailableCount").replace("{}", String(item.availableCount))}
      secondary={expiry}
      percent={null}
    />
  );
}

/**
 * Key-value sections (upstream `ProviderDetailSectionsContent.swift`): an
 * uppercase caption title, then rows with the label on the left and the
 * value, plus an optional secondary value, on the right.
 */
export default function DetailSections({
  details,
  inventory,
  hiddenUsageItemIds,
  resetTimeRelative,
}: {
  details: ProviderDisplayDetailSnapshot[];
  inventory: ProviderInventoryItemSnapshot[];
  hiddenUsageItemIds: string[] | null | undefined;
  resetTimeRelative: boolean;
}) {
  const { t } = useLocale();
  const groups = groupProviderDisplayDetails(details)
    .map((group) => ({
      ...group,
      rows: group.rows.filter((detail) => isDetailSectionVisible(hiddenUsageItemIds ?? undefined, detail.title)),
    }))
    .filter((group) => group.rows.length > 0);
  if (inventory.length === 0 && groups.length === 0) return null;

  return (
    <div className="card-kv">
      {inventory.length > 0 && (
        <section className="card-kv__section">
          {inventory.map((item) => (
            <InventoryRow key={item.id} item={item} resetTimeRelative={resetTimeRelative} />
          ))}
        </section>
      )}
      {groups.map((group, index) => (
        <Fragment key={group.id}>
          {(index > 0 || inventory.length > 0) && <div className="card-kv__divider" />}
          <section className="card-kv__section">
            {group.title && (
              <div className="card-kv__title" role="heading" aria-level={4}>
                {localizeProviderLabel(group.title, t)}
              </div>
            )}
            {group.rows.map((detail, rowIndex) => (
              <KeyValueRow
                key={`${detail.id}-${rowIndex}`}
                label={localizeProviderLabel(detail.title, t)}
                value={localizeProviderText(detail.value, t)}
                secondary={
                  detail.secondaryValue ? localizeProviderText(detail.secondaryValue, t) : null
                }
                percent={progressPercent(detail)}
              />
            ))}
          </section>
        </Fragment>
      ))}
    </div>
  );
}
