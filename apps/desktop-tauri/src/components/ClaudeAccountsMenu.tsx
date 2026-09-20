import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ClaudeAccount } from "../types/bridge";
import { claudeAccountsList, claudeAccountSwitch } from "../lib/tauri";
import { useLocale } from "../hooks/useLocale";
import {
  buildClaudeAccountOrdinals,
  buildPrivateClaudeAccountLabel,
} from "./claudeAccountDisplay";

type ClaudeAccountPhase = "idle" | "activating" | "reconciling" | "settled";

export default function ClaudeAccountsMenu({ hideEmail, onLayoutChange }: {
  hideEmail: boolean;
  onLayoutChange?: () => void;
}) {
  const { t } = useLocale();
  const [accounts, setAccounts] = useState<ClaudeAccount[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [switched, setSwitched] = useState(false);
  const [phase, setPhase] = useState<ClaudeAccountPhase>("idle");
  const mounted = useRef(false);
  const load = useCallback(async () => {
    const next = await claudeAccountsList();
    if (mounted.current) {
      setAccounts(next);
      setError(null);
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
    const unlistenReconciling = listen("claude-accounts-reconciling", () => {
      if (mounted.current) {
        setPhase("reconciling");
        setSwitched(false);
      }
    });
    const unlistenReconciled = listen("claude-accounts-reconciled", () => {
      if (mounted.current) {
        setPhase("settled");
      }
    });
    return () => {
      mounted.current = false;
      window.removeEventListener("focus", reload);
      void unlisten.then(fn => fn()).catch(() => {});
      void unlistenReconciling.then(fn => fn()).catch(() => {});
      void unlistenReconciled.then(fn => fn()).catch(() => {});
    };
  }, [load]);
  useEffect(() => {
    onLayoutChange?.();
  }, [accounts.length, error, phase, switched, onLayoutChange]);

  const accountOrdinals = buildClaudeAccountOrdinals(accounts);

  const switchAccount = async (id: string) => {
    setPhase("activating");
    setError(null);
    setSwitched(false);
    try {
      await claudeAccountSwitch(id);
      await load();
      // Settling is event-driven: the backend emits claude-accounts-reconciled
      // after the awaited refresh. The promise resolving does not settle.
      if (mounted.current) setSwitched(true);
    } catch (e) {
      if (mounted.current) {
        setPhase("idle");
        setError(String(e));
      }
    }
  };

  const hasSwitchableAccount = accounts.some(account => account.isSaved && !account.isActive);
  if (accounts.length <= 1 && !hasSwitchableAccount && !error) return null;
  return (
    <details
      className="codex-menu-accounts"
      data-claude-account-phase={phase}
      aria-busy={phase === "activating" || phase === "reconciling"}
      onToggle={onLayoutChange}
    >
      <summary className="codex-menu-accounts__summary">
        <span className="codex-menu-accounts__title">{t("ClaudeAccountsTitle")}</span>
        <span className="codex-menu-accounts__count">{accounts.length}</span>
      </summary>
      {error && <div className="codex-menu-accounts__error" role="alert">{error}</div>}
      {switched && <p role="status">{t("ClaudeAccountsSwitched")}</p>}
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
                </div>
                <button
                  type="button"
                  className="codex-menu-accounts__switch"
                  disabled={phase === "activating" || phase === "reconciling" || account.isActive || !account.isSaved}
                  onClick={() => void switchAccount(account.id)}
                >
                  {t("CodexAccountsSwitchButton")}
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </details>
  );
}
