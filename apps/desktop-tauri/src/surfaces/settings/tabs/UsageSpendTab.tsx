import { useCallback, useEffect, useState } from "react";
import { useLocale } from "../../../hooks/useLocale";
import { getUsageSpendSummary } from "../../../lib/tauri";
import type { UsageSpendSummary } from "../../../types/bridge";
import type { TabProps } from "../../Settings";

function formatUsd(value: number | null | undefined, currency: string): string {
  if (value == null || !Number.isFinite(value)) return "—";
  try {
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: currency || "USD",
      maximumFractionDigits: 2,
    }).format(value);
  } catch {
    return `$${value.toFixed(2)}`;
  }
}

export default function UsageSpendTab(_props: TabProps) {
  const { t } = useLocale();
  const [summary, setSummary] = useState<UsageSpendSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    setLoading(true);
    setError(null);
    void getUsageSpendSummary()
      .then((data) => {
        setSummary(data);
        setLoading(false);
      })
      .catch((err: unknown) => {
        setError(err instanceof Error ? err.message : String(err));
        setLoading(false);
      });
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  return (
    <section className="settings-section">
      <h3 className="settings-section__title settings-section__title--bold">
        {t("UsageSpendTitle")}
      </h3>
      <p className="settings-section__caption">{t("UsageSpendCaption")}</p>

      <div className="settings-section__group" style={{ marginBottom: 12 }}>
        <button
          type="button"
          className="credential-btn credential-btn--secondary"
          disabled={loading}
          onClick={load}
        >
          {loading ? t("UsageSpendLoading") : t("UsageSpendRefresh")}
        </button>
      </div>

      {error && <p className="settings-section__error">{error}</p>}

      {!error && (
        <table className="usage-spend-table">
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
                <td>{formatUsd(row.sevenDay, row.currency)}</td>
                <td>{formatUsd(row.thirtyDay, row.currency)}</td>
                <td>{row.currency || "USD"}</td>
                <td className="usage-spend-table__source">{row.source}</td>
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
    </section>
  );
}
