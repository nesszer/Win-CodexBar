import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ClaudeAccount } from "../types/bridge";
import { claudeAccountsList, claudeAccountAdd, claudeAccountReauthenticate, claudeAccountCancelLogin, claudeAccountSwitch } from "../lib/tauri";
import { useLocale } from "../hooks/useLocale";
import ClaudeAccountUsage from "./ClaudeAccountUsage";
import ProviderAccountsMenu from "./ProviderAccountsMenu";
import {
  localClaudeReconciliationOutcome,
  useClaudeReconciliation,
} from "../hooks/useClaudeReconciliation";
import {
  buildClaudeAccountOrdinals,
  buildPrivateClaudeAccountLabel,
} from "./claudeAccountDisplay";

export default function ClaudeAccountsMenu({ hideEmail, onLayoutChange, showAsUsed = false, resetTimeRelative = true }: {
  hideEmail: boolean;
  onLayoutChange?: () => void;
  showAsUsed?: boolean;
  resetTimeRelative?: boolean;
}) {
  const { t } = useLocale();
  const [accounts, setAccounts] = useState<ClaudeAccount[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [switched, setSwitched] = useState(false);
  const [activating, setActivating] = useState(false);
  const [signingIn, setSigningIn] = useState(false);
  const [loading, setLoading] = useState(true);
  const [operationGeneration, setOperationGeneration] = useState<number | null>(null);
  const { snapshot, accept, reconciling } = useClaudeReconciliation();
  const mounted = useRef(false);
  const load = useCallback(async () => {
    try {
      const next = await claudeAccountsList();
      if (mounted.current) {
        setAccounts(next);
        setError(null);
      }
    } finally {
      if (mounted.current) setLoading(false);
    }
  }, []);
  useEffect(() => {
    mounted.current = true;
    const reload = () => {
      void load().catch(e => { if (mounted.current) setError(String(e)); });
    };
    reload();
    window.addEventListener("focus", reload);
    const unlisten = listen("claude-accounts-updated", reload);
    return () => {
      mounted.current = false;
      window.removeEventListener("focus", reload);
      void unlisten.then(fn => fn()).catch(() => {});
    };
  }, [load]);
  useEffect(() => {
    const outcome = localClaudeReconciliationOutcome(snapshot, operationGeneration);
    if (!outcome) return;
    if (outcome.status === "failed") {
      setSwitched(false);
      setError(outcome.detail);
    } else {
      setSwitched(true);
      setError(null);
    }
  }, [operationGeneration, snapshot]);
  const phase = reconciling
    ? "reconciling"
    : activating
      ? "activating"
      : snapshot
        ? "settled"
        : "idle";
  useEffect(() => {
    onLayoutChange?.();
  }, [accounts, error, phase, switched, signingIn, onLayoutChange]);

  const accountOrdinals = buildClaudeAccountOrdinals(accounts);

  const switchAccount = async (id: string) => {
    setActivating(true);
    setOperationGeneration(null);
    setError(null);
    setSwitched(false);
    try {
      const result = await claudeAccountSwitch(id);
      setOperationGeneration(result.generation);
      accept(result);
      await load();
    } catch (e) {
      if (mounted.current) {
        setError(String(e));
      }
    } finally {
      if (mounted.current) setActivating(false);
    }
  };

  const addAccount = async (id?: string) => {
    setSigningIn(true);
    setError(null);
    setSwitched(false);
    try {
      await (id ? claudeAccountReauthenticate(id) : claudeAccountAdd());
      await load();
    } catch (e) {
      if (mounted.current) setError(String(e));
    } finally {
      if (mounted.current) setSigningIn(false);
    }
  };
  const busy = loading || activating || reconciling || signingIn;
  return (
    <ProviderAccountsMenu
      title={t("ClaudeAccountsTitle")}
      count={accounts.length}
      data-claude-account-phase={phase}
      aria-busy={busy}
      onLayoutChange={onLayoutChange}
      actions={<>
        <button type="button" className="codex-menu-accounts__action" disabled={busy} onClick={() => void addAccount()}>
          {t("CodexAccountsAddButton")}
        </button>
        {signingIn && <button type="button" className="codex-menu-accounts__action" onClick={() => void claudeAccountCancelLogin().catch(e => { if (mounted.current) setError(String(e)); })}>
          {t("ClaudeAccountsCancelLogin")}
        </button>}
      </>}
    >
      {error && <div className="codex-menu-accounts__error" role="alert">{error}</div>}
      {switched && <p role="status">{t("ClaudeAccountsSwitched")}</p>}
      {signingIn && <p className="codex-menu-accounts__usage" role="status">{t("ClaudeAccountsSigningIn")}</p>}
      <ul className="codex-menu-accounts__list">
        {accounts.map(account => {
          const privateLabel = buildPrivateClaudeAccountLabel(
            account,
            accountOrdinals[account.id],
            hideEmail,
            t("Account"),
          );
          return (
            <li key={account.id}>
              <div className={`codex-menu-accounts__row${account.isActive ? " codex-menu-accounts__row--active" : ""}`}>
                <div className="codex-menu-accounts__meta">
                  <span className="codex-menu-accounts__email" title={privateLabel.tooltip}>
                    {privateLabel.label}
                    {account.isActive && <span className="codex-menu-accounts__badge">{t("TokenAccountActive")}</span>}
                  </span>
                  {!hideEmail && account.organization && !account.organization.includes(account.email) && (
                    <span className="codex-menu-accounts__usage">{account.organization}</span>
                  )}
                  <ClaudeAccountUsage account={account} t={t} showAsUsed={showAsUsed} resetTimeRelative={resetTimeRelative} />
                </div>
                <div className="codex-menu-accounts__row-actions">
                  {account.needsAuthentication && (
                    <button
                      type="button"
                      className="codex-menu-accounts__switch"
                      disabled={busy}
                      aria-label={`${t("CodexAccountsReauthenticateButton")}: ${privateLabel.label}`}
                      onClick={() => void addAccount(account.id)}
                    >
                      {t("CodexAccountsReauthenticateButton")}
                    </button>
                  )}
                  <button
                    type="button"
                    className="codex-menu-accounts__switch"
                    disabled={busy || account.isActive || !account.isSaved}
                    onClick={() => void switchAccount(account.id)}
                  >
                    {t("CodexAccountsSwitchButton")}
                  </button>
                </div>
              </div>
            </li>
          );
        })}
      </ul>
    </ProviderAccountsMenu>
  );
}
