import { useEffect, useState, type ReactNode } from "react";
import type { GrokAccount, GrokAccountUsage, ProviderUsageSnapshot } from "../types/bridge";
import { grokAccountSwitch, grokAccountReauthenticate, grokAccountCancelLogin } from "../lib/tauri";
import { useLocale } from "../hooks/useLocale";
import GrokAccountUsageDetails from "./GrokAccountUsageDetails";
import { useGrokAccounts } from "../hooks/useGrokAccounts";
import { maskEmail } from "./MenuCard";

export default function GrokAccountsMenu({
  hideEmail,
  resetTimeRelative,
  onLayoutChange,
  cardSnapshot,
  renderHeader,
}: {
  hideEmail: boolean;
  resetTimeRelative: boolean;
  onLayoutChange?: () => void;
  cardSnapshot?: GrokAccountsCardSnapshot;
  renderHeader?: (refreshAction?: ReactNode) => ReactNode;
}) {
  const { t } = useLocale();
  const [switched, setSwitched] = useState(false);
  const [loggingIn, setLoggingIn] = useState(false);
  const [reauthenticated, setReauthenticated] = useState(false);
  const { accounts, usage, busy, error, run, reload, reportError } = useGrokAccounts({ reloadOnFocus: true });
  useEffect(() => {
    onLayoutChange?.();
  }, [accounts.length, usage, cardSnapshot, error, switched, loggingIn, reauthenticated, onLayoutChange]);

  const signInAgain = async (id: string) => {
    setLoggingIn(true);
    setSwitched(false);
    setReauthenticated(false);
    await run(() => grokAccountReauthenticate(id), () => setReauthenticated(true), () => setLoggingIn(false));
  };

  const switchAccount = async (id: string) => {
    setSwitched(false);
    setReauthenticated(false);
    await run(() => grokAccountSwitch(id), () => setSwitched(true));
  };

  if (accounts.length === 0 && !error) return renderHeader?.() ?? null;
  const refreshAction = (
    <button
      type="button"
      className="credential-btn menu-card__refresh-btn"
      disabled={busy || loggingIn}
      onClick={() => void reload()}
    >
      {t("GrokUsageRetry")}
    </button>
  );
  return (
    <>
      {renderHeader ? renderHeader(refreshAction) : refreshAction}
      <details className="codex-menu-accounts" open onToggle={onLayoutChange}>
        <summary className="codex-menu-accounts__summary">
          <span className="codex-menu-accounts__title">{t("GrokAccountsTitle")}</span>
          <span className="codex-menu-accounts__count">{accounts.length}</span>
        </summary>
        {error && (
          <div className="codex-menu-accounts__error" role="alert">
            {error}
          </div>
        )}
        {switched && <p role="status">{t("GrokAccountsSwitched")}</p>}
        {reauthenticated && <p role="status">{t("GrokAccountsReauthenticated")}</p>}
        <p className="settings-section__hint">{t("GrokAccountsSourceHint")}</p>
        {loggingIn && <p role="status">{t("GrokAccountsSigningIn")}</p>}
        {loggingIn && <button type="button" onClick={() => void grokAccountCancelLogin().catch(reportError)}>{t("GrokAccountsCancelLogin")}</button>}
        <ul className="codex-menu-accounts__list">
          {accounts.map((account) => (
            <GrokAccountRow
              key={account.id}
              account={account}
              snapshot={accountUsage(account, usage[account.id], cardSnapshot)}
              status={usage[account.id]?.status}
              hideEmail={hideEmail}
              resetTimeRelative={resetTimeRelative}
              busy={busy}
              onSwitch={switchAccount}
              onSignInAgain={signInAgain}
            />
          ))}
        </ul>
      </details>
    </>
  );
}

function accountUsage(
  account: GrokAccount,
  snapshot: GrokAccountUsage | undefined,
  card: GrokAccountsCardSnapshot | undefined,
): GrokAccountUsage | undefined {
  if (snapshot?.usageAvailable && snapshot.usedPercent != null) return snapshot;
  // The card may still belong to the previous CLI or browser account after a
  // switch. Unknown identity and informational windows cannot prove usage.
  const email = card?.accountEmail?.trim().toLowerCase();
  if (!account.isActive || !card || card.error || !email
    || email !== account.email.trim().toLowerCase()
    || (card.accountOrganization ?? null) !== account.organization
    || card.primary.isInformational
    || !Number.isFinite(card.primary.usedPercent)) return snapshot;
  return {
    status: "ready",
    usageAvailable: true,
    usedPercent: card.primary.usedPercent,
    plan: card.planName,
    windowMinutes: card.primary.windowMinutes,
    resetsAt: card.primary.resetsAt,
  };
}

type GrokAccountsCardSnapshot = Pick<ProviderUsageSnapshot,
  "accountEmail" | "accountOrganization" | "primary" | "planName" | "error"
>;

function GrokAccountRow({
  account,
  snapshot,
  status,
  hideEmail,
  resetTimeRelative,
  busy,
  onSwitch,
  onSignInAgain,
}: {
  account: GrokAccount;
  snapshot: GrokAccountUsage | undefined;
  status: GrokAccountUsage["status"];
  hideEmail: boolean;
  resetTimeRelative: boolean;
  busy: boolean;
  onSwitch: (id: string) => Promise<void>;
  onSignInAgain: (id: string) => Promise<void>;
}) {
  const { t } = useLocale();
  const email = hideEmail ? maskEmail(account.email) : account.email;

  return (
    <li>
      <div
        className={`codex-menu-accounts__row${account.isActive ? " codex-menu-accounts__row--active" : ""}`}
      >
        <div className="codex-menu-accounts__meta">
          <span className="codex-menu-accounts__email" title={email}>
            {email}
            {account.isActive && (
              <span className="codex-menu-accounts__badge">
                {t("TokenAccountActive")}
              </span>
            )}
          </span>
          <GrokAccountUsageDetails snapshot={snapshot} status={status} t={t} relative={resetTimeRelative} />
        </div>
        {status !== "signInRequired" && <button
          type="button"
          className="codex-menu-accounts__switch"
          disabled={busy || account.isActive || !account.isSaved}
          onClick={() => void onSwitch(account.id)}
        >
          {t("CodexAccountsSwitchButton")}
        </button>}
        {status === "signInRequired" && <button type="button" className="codex-menu-accounts__switch" disabled={busy} onClick={() => void onSignInAgain(account.id)}>{t("GrokAccountsSignInAgain")}</button>}
      </div>
    </li>
  );
}
