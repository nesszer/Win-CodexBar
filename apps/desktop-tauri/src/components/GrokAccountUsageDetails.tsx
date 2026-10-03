import type { GrokAccountUsage } from "../types/bridge";
import type { LocaleKey } from "../i18n/keys";
import { useFormattedResetTime } from "../hooks/useFormattedResetTime";

export default function GrokAccountUsageDetails({ snapshot, status = snapshot?.status, t, relative = true }: {
  snapshot?: GrokAccountUsage;
  status?: GrokAccountUsage["status"];
  t: (key: LocaleKey) => string;
  relative?: boolean;
}) {
  const percent = snapshot?.usageAvailable && snapshot.usedPercent != null && Number.isFinite(snapshot.usedPercent)
    ? Math.round(snapshot.usedPercent) : null;
  const reset = useFormattedResetTime(snapshot?.resetsAt ?? null, null, relative);
  const stateKey: LocaleKey | null = status === "loading" ? "GrokUsageLoading"
    : status === "signInRequired" ? "GrokUsageSignInRequired"
    : status === "failed" ? "GrokUsageFailed"
    : percent === null ? "GrokUsageUnavailable" : null;
  const minutes = snapshot?.windowMinutes;
  const windowLabel = minutes && minutes > 0
    ? minutes % 1440 === 0 ? `${minutes / 1440}d` : minutes % 60 === 0 ? `${minutes / 60}h` : `${minutes}m`
    : null;
  return <>
    {stateKey && <span className="codex-menu-accounts__usage codex-menu-accounts__usage-status" role="status">{t(stateKey)}</span>}
    {percent !== null && <>
      <span className="codex-menu-accounts__usage">
        {windowLabel && <span>{windowLabel}</span>}
        <span>{percent}% {t("PanelUsedSuffix")}</span>
        <span>{reset ? relative ? reset : `${t("MetricResetsIn")} ${reset}` : t("GrokUsageResetUnavailable")}</span>
      </span>
      <span className="codex-menu-accounts__bar" aria-label={`${percent}% ${t("PanelUsedSuffix")}`}>
        <span className="codex-menu-accounts__bar-fill" data-level={percent >= 100 ? "exhausted" : percent >= 90 ? "critical" : undefined}
          style={{ width: `${Math.max(2, Math.min(100, percent))}%` }} />
      </span>
    </>}
  </>;
}
