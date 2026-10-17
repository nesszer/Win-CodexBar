import { useEffect, useState } from "react";
import type { ProviderDetail, SettingsUpdate } from "../../../../types/bridge";
import type { LocaleKey } from "../../../../i18n/keys";
import { SettingsGroup } from "../../../../components/FormControls";
import { localizeWindowLabel } from "../../../../lib/windowLabels";

interface Props {
  provider: ProviderDetail;
  disabled: boolean;
  t: (key: LocaleKey) => string;
  onChange: (patch: SettingsUpdate) => void;
  /** Cost summary display is Hidden, which also suppresses cost-gated rows. */
  costSummaryHidden?: boolean;
}

/**
 * "Visible usage items" group (macOS `ProviderUsageItemsVisibilitySection`):
 * one checkbox per emitted usage row plus Restore Defaults. Presentation only;
 * hidden rows are dropped from the tray card and Overview.
 */
export function UsageItemVisibilitySection({
  provider,
  disabled,
  t,
  onChange,
  costSummaryHidden = false,
}: Props) {
  const items = provider.usageItems ?? [];
  const [hidden, setHidden] = useState<Set<string>>(
    () => new Set(provider.hiddenUsageItemIds ?? []),
  );

  useEffect(() => {
    setHidden(new Set(provider.hiddenUsageItemIds ?? []));
  }, [provider.id, provider.hiddenUsageItemIds]);

  if (items.length === 0) return null;

  const persist = (next: Set<string>) => {
    const ids = [...next].sort();
    setHidden(next);
    onChange({
      providerHiddenUsageItemIds: {
        [provider.id]: ids,
      },
    });
  };

  const toggle = (id: string, visible: boolean) => {
    const next = new Set(hidden);
    if (visible) next.delete(id);
    else next.add(id);
    persist(next);
  };

  return (
    <SettingsGroup
      className="provider-detail-group provider-usage-items-group"
      title={t("ProviderVisibleUsageItems")}
      footer={<p>{t("ProviderVisibleUsageItemsFooter")}</p>}
    >
      {items.map((item) => (
        <label
          className={`provider-usage-item${disabled ? " provider-usage-item--disabled" : ""}`}
          key={item.id}
        >
          <input
            type="checkbox"
            className="toggle"
            checked={!hidden.has(item.id)}
            disabled={disabled}
            onChange={(event) => toggle(item.id, event.target.checked)}
          />
          <span className="provider-usage-item__text">
            <span className="provider-usage-item__label">
              {item.available ? localizeWindowLabel(item.title, t) || item.title : item.title}
            </span>
            {!item.available && (
              <span className="provider-usage-item__helper">
                {t("ProviderUsageNotFetchedYet")}
              </span>
            )}
            {item.requiresCostSummary && costSummaryHidden && (
              <span className="provider-usage-item__helper">
                {t("UsageItemHiddenByCostSummary")}
              </span>
            )}
          </span>
        </label>
      ))}
      <div className="provider-detail-group__actions">
        <button
          type="button"
          className="credential-btn credential-btn--secondary"
          disabled={disabled || hidden.size === 0}
          onClick={() => persist(new Set())}
        >
          {t("ProviderRestoreDefaults")}
        </button>
      </div>
    </SettingsGroup>
  );
}
