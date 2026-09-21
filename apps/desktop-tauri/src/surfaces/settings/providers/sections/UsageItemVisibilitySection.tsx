import { useEffect, useState } from "react";
import type { ProviderDetail, SettingsUpdate } from "../../../../types/bridge";
import type { LocaleKey } from "../../../../i18n/keys";

interface Props {
  provider: ProviderDetail;
  disabled: boolean;
  t: (key: LocaleKey) => string;
  onChange: (patch: SettingsUpdate) => void;
}

/** Presentation-only visibility controls for the provider's emitted metric rows. */
export function UsageItemVisibilitySection({
  provider,
  disabled,
  t,
  onChange,
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
    <section className="provider-detail-section">
      <div className="provider-detail-section__header">
        <h4>{t("ProviderOptionsTitle")}</h4>
        <button
          type="button"
          className="credential-btn"
          disabled={disabled || hidden.size === 0}
          onClick={() => persist(new Set())}
        >
          {t("WindowRestore")}
        </button>
      </div>
      {items.map((item) => (
        <label className="provider-detail-toggle" key={item.id}>
          <input
            type="checkbox"
            checked={!hidden.has(item.id)}
            disabled={disabled}
            onChange={(event) => toggle(item.id, event.target.checked)}
          />
          <span>
            <span className="provider-detail-toggle__label">{item.title}</span>
            {!item.available && (
              <span className="provider-detail-toggle__helper">
                {t("ProviderUsageNotFetchedYet")}
              </span>
            )}
          </span>
        </label>
      ))}
    </section>
  );
}
