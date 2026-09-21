import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ClaudeAccount } from "../../../../../types/bridge";
import type { Language } from "../../../../../types/bridge";
import type { LocaleKey } from "../../../../../i18n/keys";
import {
  claudeAccountsList,
  claudeAccountAdd,
  claudeAccountCancelLogin,
  claudeAccountSaveCurrent,
  claudeAccountRemove,
  claudeAccountSwitch,
} from "../../../../../lib/tauri";
import { ClaudeSwapAccountsSection } from "./ClaudeSwapAccountsSection";

export function ClaudeAccountsSection({
  t,
  language = "english",
}: {
  t: (key: LocaleKey) => string;
  language?: Language;
}) {
  const [accounts, setAccounts] = useState<ClaudeAccount[]>([]);
  const [busy, setBusy] = useState(false);
  const [loggingIn, setLoggingIn] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [reconciling, setReconciling] = useState(false);
  const mounted = useRef(false);
  const load = useCallback(async () => {
    const next = await claudeAccountsList();
    if (mounted.current) setAccounts(next);
  }, []);
  useEffect(() => {
    mounted.current = true;
    const reload = () => {
      void load().catch(e => {
        if (mounted.current) setError(String(e));
      });
    };
    reload();
    const unlisten = listen("claude-accounts-updated", reload);
    const unlistenReconciling = listen("claude-accounts-reconciling", () => {
      if (mounted.current) setReconciling(true);
    });
    const unlistenReconciled = listen("claude-accounts-reconciled", () => {
      if (mounted.current) setReconciling(false);
    });
    return () => {
      mounted.current = false;
      void unlisten.then(fn => fn());
      void unlistenReconciling.then(fn => fn()).catch(() => {});
      void unlistenReconciled.then(fn => fn()).catch(() => {});
    };
  }, [load]);
  const run = async (operation: () => Promise<void>, success?: LocaleKey) => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      await operation();
      await load();
      if (mounted.current && success) setMessage(t(success));
    } catch (e) {
      if (mounted.current) setError(String(e));
    } finally {
      if (mounted.current) {
        setBusy(false);
        setLoggingIn(false);
      }
    }
  };
  return (
    <>
      <section className="provider-detail-section codex-accounts">
        <h4>{t("ClaudeAccountsTitle")}</h4>
        <p className="settings-section__hint">{t("ClaudeAccountsHint")}</p>
        {error && <div className="provider-detail-error" role="alert">{error}</div>}
        {message && <div className="provider-detail-note" role="status">{message}</div>}
        {reconciling && <p role="status">{t("ClaudeAccountsReconciling")}</p>}
        {loggingIn && <p role="status">{t("ClaudeAccountsSigningIn")}</p>}
        {accounts.length === 0 && <p>{t("ClaudeAccountsEmpty")}</p>}
        <ul className="credential-list">
          {accounts.map(account => (
            <li className="credential-card" key={account.id}>
              <div className="credential-card__header">
                <div className="credential-card__info">
                  <strong>{account.email}</strong>
                  <span className="credential-card__meta">
                    {[
                      account.organization?.includes(account.email) ? null : account.organization,
                      account.plan,
                    ].filter(Boolean).join(" · ")}
                  </span>
                  {account.isActive && (
                    <span className="credential-card__badge credential-card__badge--set">
                      {t("TokenAccountActive")}
                    </span>
                  )}
                </div>
                <div className="credential-card__actions">
                  {!account.isActive && account.isSaved && (
                    <button
                      className="credential-btn credential-btn--primary"
                      disabled={busy || reconciling}
                      onClick={() => void run(() => claudeAccountSwitch(account.id), "ClaudeAccountsSwitched")}
                    >
                      {t("CodexAccountsSwitchButton")}
                    </button>
                  )}
                  {!account.isSaved && (
                    <button
                      className="credential-btn credential-btn--secondary"
                      disabled={busy || reconciling}
                      onClick={() => void run(claudeAccountSaveCurrent)}
                    >
                      {t("ClaudeAccountsSaveCurrent")}
                    </button>
                  )}
                  {account.isSaved && (
                    <button
                      className="credential-btn credential-btn--danger"
                      disabled={busy || reconciling}
                      onClick={() => void run(() => claudeAccountRemove(account.id))}
                    >
                      {t("CodexAccountsRemoveButton")}
                    </button>
                  )}
                </div>
              </div>
            </li>
          ))}
        </ul>
        <button
          className="credential-btn credential-btn--primary"
          disabled={busy || reconciling}
          onClick={() => {
            setLoggingIn(true);
            void run(claudeAccountAdd, "ClaudeAccountsAdded");
          }}
        >
          {t("CodexAccountsAddButton")}
        </button>
        {loggingIn && (
          <button
            className="credential-btn credential-btn--secondary"
            onClick={() => void claudeAccountCancelLogin().catch(e => setError(String(e)))}
          >
            {t("ClaudeAccountsCancelLogin")}
          </button>
        )}
      </section>
      <ClaudeSwapAccountsSection t={t} language={language} />
    </>
  );
}
