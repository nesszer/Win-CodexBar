import { useEffect, useState } from "react";
import type { LocaleKey } from "../../../../../i18n/keys";
import type { SettingsSnapshot, SettingsUpdate } from "../../../../../types/bridge";

interface Props {
  value: SettingsSnapshot["copilotSeatCreditEntitlement"];
  disabled: boolean;
  t: (key: LocaleKey) => string;
  onChange: (patch: SettingsUpdate) => void;
}

function formatValue(value: number | null | undefined): string {
  return value == null ? "" : String(value);
}

export function CopilotSeatCreditOptions({
  value,
  disabled,
  t,
  onChange,
}: Props) {
  const [draft, setDraft] = useState(() => formatValue(value));
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setDraft(formatValue(value));
    setError(null);
  }, [value]);

  const commit = () => {
    const trimmed = draft.trim();
    if (trimmed === "") {
      setError(null);
      onChange({ copilotSeatCreditEntitlement: null });
      return;
    }

    const next = Number(trimmed);
    if (!Number.isFinite(next) || next <= 0) {
      setError(t("CopilotSeatCreditInvalid"));
      return;
    }

    setError(null);
    onChange({ copilotSeatCreditEntitlement: next });
  };

  return (
    <section className="provider-detail-section">
      <h4>{t("ProviderOptionsTitle")}</h4>
      <label className="provider-detail-field">
        <span className="provider-detail-field__label">
          {t("CopilotSeatCreditTitle")}
        </span>
        <input
          className="provider-detail-field__input"
          type="number"
          min="0"
          step="any"
          value={draft}
          disabled={disabled}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={commit}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.currentTarget.blur();
            }
          }}
        />
        <span className="provider-detail-section__helper">
          {t("CopilotSeatCreditHelper")}
        </span>
      </label>
      {error && <div className="provider-detail-error">{error}</div>}
    </section>
  );
}
