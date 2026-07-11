import { useEffect, useState } from "react";
import type { LocaleKey } from "../../../../../i18n/keys";
import { getSettingsSnapshot, updateSettings } from "../../../../../lib/tauri";

interface Props {
  t: (key: LocaleKey) => string;
}

export function CodexUsageOptions({ t }: Props) {
  const [value, setValue] = useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    getSettingsSnapshot()
      .then(
        (settings) =>
          !cancelled && setValue(settings.codexSparkUsageVisible),
      )
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, []);

  const toggle = async (next: boolean) => {
    setSaving(true);
    try {
      const updated = await updateSettings({ codexSparkUsageVisible: next });
      setValue(updated.codexSparkUsageVisible);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  if (value === null) return null;

  return (
    <section className="provider-detail-section">
      <h4>{t("ProviderOptionsTitle")}</h4>
      <label className="provider-detail-toggle">
        <input
          type="checkbox"
          checked={value}
          disabled={saving}
          onChange={(e) => void toggle(e.target.checked)}
        />
        <span>
          <span className="provider-detail-toggle__label">
            {t("ProviderCodexSparkUsage")}
          </span>
          <span className="provider-detail-toggle__helper">
            {t("ProviderCodexSparkUsageHelp")}
          </span>
        </span>
      </label>
      {error && <div className="provider-detail-error">{error}</div>}
    </section>
  );
}
