import type { ProviderInventoryItem } from "../types/bridge";
import { useFormattedResetTime } from "../hooks/useFormattedResetTime";
import { useLocale } from "../hooks/useLocale";

/**
 * One discrete-inventory line: "{title}: {count} available [expiry]".
 *
 * Shared by the tray menu card (`MenuCardDetails`) and the settings provider
 * detail (`UsageSection`); each surface passes its own layout classes.
 */
export function InventoryItemRow({
  item,
  resetTimeRelative,
  lineClassName,
  expiryClassName,
}: {
  item: ProviderInventoryItem;
  resetTimeRelative: boolean;
  lineClassName: string;
  expiryClassName?: string;
}) {
  const { t } = useLocale();
  const formattedExpiry = useFormattedResetTime(
    item.nextExpiresAt,
    null,
    resetTimeRelative,
    "expires",
  );

  return (
    <div className={lineClassName}>
      <span>{`${item.title}: ${t("InventoryAvailableCount").replace("{}", String(item.availableCount))}`}</span>
      {formattedExpiry && expiryClassName && (
        <span className={expiryClassName}>{formattedExpiry}</span>
      )}
    </div>
  );
}
