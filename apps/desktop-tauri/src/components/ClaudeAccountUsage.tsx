import type { ClaudeAccount } from "../types/bridge";
import type { LocaleKey } from "../i18n/keys";
import { useFormattedResetTime } from "../hooks/useFormattedResetTime";

export default function ClaudeAccountUsage({ account, t, showAsUsed = false, resetTimeRelative = true }: {
  account: ClaudeAccount;
  t: (key: LocaleKey) => string;
  showAsUsed?: boolean;
  resetTimeRelative?: boolean;
}) {
  const windows = account.usage;
  return <div className="claude-account-usage" data-stale={Boolean(account.usageError)}>
    {windows?.fiveHour && <UsageWindow window={windows.fiveHour} label={t("ProviderSession")} t={t} showAsUsed={showAsUsed} relative={resetTimeRelative} />}
    {windows?.sevenDay && <UsageWindow window={windows.sevenDay} label={t("ProviderWeekly")} t={t} showAsUsed={showAsUsed} relative={resetTimeRelative} />}
    {!windows?.fiveHour && !windows?.sevenDay && !account.usageError && <span className="codex-menu-accounts__usage">{t("CodexAccountsUsageUnavailable")}</span>}
    {account.usageError && <span className="codex-menu-accounts__error" role="status">
      {account.usageError}{windows && ` · ${t("DetailUpdatedPrefix")} ${new Date(windows.updatedAt).toLocaleString()}`}
    </span>}
  </div>;
}

function UsageWindow({ window, label, t, showAsUsed, relative }: {
  window: NonNullable<NonNullable<ClaudeAccount["usage"]>["fiveHour"]>;
  label: string;
  t: (key: LocaleKey) => string;
  showAsUsed: boolean;
  relative: boolean;
}) {
  const reset = useFormattedResetTime(window.resetsAt, null, relative);
  const known = Number.isFinite(window.usedPercent);
  const used = Math.max(0, Math.min(100, Math.round(window.usedPercent)));
  const remaining = 100 - used;
  return <div>
    <span className="codex-menu-accounts__usage">
      <span>{label}</span>
      <span>{known ? `${remaining}% ${t("PanelLeftSuffix")}` : t("CodexAccountsUsageUnavailable")}</span>
      {known && showAsUsed && <span>{used}% {t("PanelUsedSuffix")}</span>}
      {reset && <span>{reset}</span>}
    </span>
    <span className="codex-menu-accounts__bar" aria-hidden>
      {known && <span className="codex-menu-accounts__bar-fill" style={{ width: `${remaining}%` }} />}
    </span>
  </div>;
}
