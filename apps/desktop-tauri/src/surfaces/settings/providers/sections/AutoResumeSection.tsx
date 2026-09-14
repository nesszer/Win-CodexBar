import { useState } from "react";
import type { LocaleKey } from "../../../../i18n/keys";
import { setProviderAutoResumeAfterQuotaReset } from "../../../../lib/tauri";

interface Props {
  providerId: string;
  enabled: boolean;
  available: boolean;
  disabled: boolean;
  t: (key: LocaleKey) => string;
  onChanged: () => void;
}

/** Opt-in control for reopening the exact local Codex or Claude CLI session. */
export function AutoResumeSection({
  providerId,
  enabled,
  available,
  disabled,
  t,
  onChanged,
}: Props) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!available) return null;

  const handleChange = async (next: boolean) => {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await setProviderAutoResumeAfterQuotaReset(providerId, next);
      onChanged();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="provider-detail-section">
      <h4>{t("ProviderAutoResumeTitle")}</h4>
      <label className="provider-detail-toggle">
        <input
          type="checkbox"
          checked={enabled}
          disabled={disabled || busy}
          onChange={(event) => void handleChange(event.target.checked)}
        />
        <span>
          <span className="provider-detail-toggle__label">
            {t("ProviderAutoResumeAfterQuotaReset")}
          </span>
          <span className="provider-detail-toggle__helper">
            {t("ProviderAutoResumeAfterQuotaResetHelper")}
          </span>
        </span>
      </label>
      {error && <div className="provider-detail-error">{error}</div>}
    </section>
  );
}
