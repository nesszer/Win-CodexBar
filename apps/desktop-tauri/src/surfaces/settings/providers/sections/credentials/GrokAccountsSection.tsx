import { useState } from "react";
import type { LocaleKey } from "../../../../../i18n/keys";
import {
  grokAccountAdd,
  grokAccountCancelLogin,
  grokAccountSaveCurrent,
  grokAccountRemove,
  grokAccountSwitch,
  grokAccountReauthenticate,
} from "../../../../../lib/tauri";
import { useGrokAccounts } from "../../../../../hooks/useGrokAccounts";
import GrokAccountUsageDetails from "../../../../../components/GrokAccountUsageDetails";

export function GrokAccountsSection({ t }: { t: (key: LocaleKey) => string }) {
  const [loggingIn, setLoggingIn] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const { accounts, usage, busy, error, reportError, run, reload } = useGrokAccounts();
  const runOperation = (operation: () => Promise<void>, success?: LocaleKey) => {
    setMessage(null);
    void run(
      operation,
      success ? () => setMessage(t(success)) : undefined,
      () => setLoggingIn(false),
    );
  };
  return (
    <section className="provider-detail-section codex-accounts">
      <h4>{t("GrokAccountsTitle")}</h4>
      <p className="settings-section__hint">{t("GrokAccountsHint")}</p>
      <p className="settings-section__hint">{t("GrokAccountsSourceHint")}</p>
      {error && (
        <div className="provider-detail-error" role="alert">
          {error}
        </div>
      )}
      {message && (
        <div className="provider-detail-note" role="status">
          {message}
        </div>
      )}
      {loggingIn && <p role="status">{t("GrokAccountsSigningIn")}</p>}
      {accounts.length === 0 && <p>{t("GrokAccountsEmpty")}</p>}
      <ul className="credential-list">
        {accounts.map((account) => (
          <li className="credential-card" key={account.id}>
            <div className="credential-card__header">
              <div className="credential-card__info">
                <strong>{account.email}</strong>
                <span className="credential-card__meta">
                  {usage[account.id]?.plan || account.plan}
                </span>
                <GrokAccountUsageDetails snapshot={usage[account.id]} t={t} />
                {account.isActive && (
                  <span className="credential-card__badge credential-card__badge--set">
                    {t("TokenAccountActive")}
                  </span>
                )}
              </div>
              <div className="credential-card__actions">
                {!account.isActive && account.isSaved && usage[account.id]?.status !== "signInRequired" && (
                  <button
                    className="credential-btn credential-btn--primary"
                    disabled={busy || usage[account.id]?.status === "signInRequired" || usage[account.id]?.status === "loading"}
                    onClick={() =>
                      runOperation(
                        () => grokAccountSwitch(account.id),
                        "GrokAccountsSwitched",
                      )
                    }
                  >
                    {t("CodexAccountsSwitchButton")}
                  </button>
                )}
                {usage[account.id]?.status === "signInRequired" && <button
                  className="credential-btn credential-btn--primary" disabled={busy}
                  onClick={() => {
                    setMessage(null);
                    setLoggingIn(true);
                    runOperation(() => grokAccountReauthenticate(account.id), "GrokAccountsReauthenticated");
                  }}>
                  {t("GrokAccountsSignInAgain")}
                </button>}
                {!account.isSaved && (
                  <button
                    className="credential-btn credential-btn--secondary"
                    disabled={busy}
                    onClick={() => runOperation(grokAccountSaveCurrent)}
                  >
                    {t("GrokAccountsSaveCurrent")}
                  </button>
                )}
                {account.isSaved && (
                  <button
                    className="credential-btn credential-btn--danger"
                    disabled={busy}
                    onClick={() => runOperation(() => grokAccountRemove(account.id))}
                  >
                    {t("CodexAccountsRemoveButton")}
                  </button>
                )}
              </div>
            </div>
          </li>
        ))}
      </ul>
      <button className="credential-btn credential-btn--secondary" disabled={busy} onClick={() => void reload()}>{t("GrokUsageRetry")}</button>
      <button
        className="credential-btn credential-btn--primary"
        disabled={busy}
        onClick={() => {
          setLoggingIn(true);
          runOperation(grokAccountAdd, "GrokAccountsAdded");
        }}
      >
        {t("CodexAccountsAddButton")}
      </button>
      {loggingIn && (
        <button
          className="credential-btn credential-btn--secondary"
          onClick={() =>
            void grokAccountCancelLogin().catch(reportError)
          }
        >
          {t("GrokAccountsCancelLogin")}
        </button>
      )}
    </section>
  );
}
