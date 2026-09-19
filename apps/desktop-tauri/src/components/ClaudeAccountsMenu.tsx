import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ClaudeAccount } from "../types/bridge";
import { claudeAccountsList, claudeAccountSwitch } from "../lib/tauri";
import { useLocale } from "../hooks/useLocale";
import { maskEmail } from "./MenuCard";

type ClaudeAccountPhase = "idle" | "activating" | "reconciling" | "settled";

export default function ClaudeAccountsMenu({ hideEmail, onLayoutChange }: {
  hideEmail: boolean;
  onLayoutChange?: () => void;
}) {
  const { t } = useLocale();
  const [accounts, setAccounts] = useState<ClaudeAccount[]>([]);
  const [busy, setBusy] = useState(false);
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
    return () => {
      mounted.current = false;
      window.removeEventListener("focus", reload);
      void unlisten.then(fn => fn()).catch(() => {});
      void unlistenReconciling.then(fn => fn()).catch(() => {});
    };
  }, [load]);
  useEffect(() => {
    onLayoutChange?.();
  }, [accounts.length, error, phase, switched, onLayoutChange]);

  const switchAccount = async (id: string) => {
    setBusy(true);
    setPhase("activating");
    setError(null);
    setSwitched(false);
    try {
      await claudeAccountSwitch(id);
      await load();
      if (mounted.current) {
        setPhase("settled");
        setSwitched(true);
      }
    } catch (e) {
      if (mounted.current) {
        setPhase("idle");
        setError(String(e));
      }
    } finally {
      if (mounted.current) setBusy(false);
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
          const email = hideEmail ? maskEmail(account.email) : account.email;
          return (
            <li key={account.id}>
              <div className={`codex-menu-accounts__row${account.isActive ? " codex-menu-accounts__row--active" : ""}`}>
                <div className="codex-menu-accounts__meta">
                  <span className="codex-menu-accounts__email" title={email}>
                    {email}
                    {account.isActive && <span className="codex-menu-accounts__badge">{t("TokenAccountActive")}</span>}
                  </span>
                  {!hideEmail && account.organization && !account.organization.includes(account.email) && (
                    <span className="codex-menu-accounts__usage">{account.organization}</span>
                  )}
                </div>
                <button
                  type="button"
                  className="codex-menu-accounts__switch"
                  disabled={busy || phase === "activating" || phase === "reconciling" || account.isActive || !account.isSaved}
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
