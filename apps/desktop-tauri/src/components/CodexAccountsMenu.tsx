import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  CodexAccount,
  CodexAccountsStateBridge,
  CodexAccountUsageSnapshot,
} from "../types/bridge";
import { useLocale } from "../hooks/useLocale";
import { useFormattedResetTime } from "../hooks/useFormattedResetTime";
import ProviderAccountsMenu from "./ProviderAccountsMenu";
import { buildCodexAccountSurfaceLabels } from "./codexAccountDisplay";
import {
  codexAccountAdd,
  codexAccountReauthenticate,
  codexAccountSwitch,
  getCodexAccountsState,
  refreshProviders,
} from "../lib/tauri";

/** Visible account overview and login actions for the Codex tray card. */
export default function CodexAccountsMenu({
  hideEmail,
  resetTimeRelative,
  showAsUsed = false,
  needsAuthentication = false,
  onLayoutChange,
}: {
  hideEmail: boolean;
  resetTimeRelative: boolean;
  showAsUsed?: boolean;
  needsAuthentication?: boolean;
  onLayoutChange?: () => void;
}) {
  const { t } = useLocale();
  const [accounts, setAccounts] = useState<CodexAccount[]>([]);
  const [snapshots, setSnapshots] = useState<
    Record<string, CodexAccountUsageSnapshot>
  >({});
  const [displayNames, setDisplayNames] = useState<Record<string, string>>({});
  const [accountOrdinals, setAccountOrdinals] = useState<Record<string, number>>({});
  const [accountNeedsAuthentication, setAccountNeedsAuthentication] = useState<Record<string, boolean>>({});
  const [loading, setLoading] = useState(true);
  const [pending, setPending] = useState(false);
  const [signingIn, setSigningIn] = useState(false);
  const busy = loading || pending;
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next: CodexAccountsStateBridge = await getCodexAccountsState();
      setAccounts(next.accounts);
      setDisplayNames(next.displayNames ?? {});
      setAccountOrdinals(next.accountOrdinals);
      setSnapshots(next.snapshots);
      setAccountNeedsAuthentication(next.needsAuthentication ?? {});
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    onLayoutChange?.();
  }, [accounts, snapshots, error, pending, onLayoutChange]);

  useEffect(() => {
    let cancelled = false;
    const unlistenPromise = listen("codex-accounts-updated", () => {
      if (!cancelled) void load();
    });
    return () => {
      cancelled = true;
      void unlistenPromise.then((fn) => fn());
    };
  }, [load]);

  const run = async (action: () => Promise<unknown>, login = false) => {
    setSigningIn(login);
    setPending(true);
    setError(null);
    try {
      await action();
      await load();
      void refreshProviders().catch(() => {});
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setPending(false);
      setSigningIn(false);
    }
  };

  const handleSwitch = (id: string) => run(() => codexAccountSwitch(id));
  const ambient = accounts.find((account) => account.source === "ambient");

  const accountDisplayNames = buildCodexAccountSurfaceLabels(
    accounts,
    displayNames,
    accountOrdinals,
    hideEmail,
    t("Account"),
  );

  return (
    <ProviderAccountsMenu
      title={t("CodexAccountsTitle")}
      count={accounts.length}
      aria-busy={busy}
      onLayoutChange={onLayoutChange}
      actions={<>
        <button
          type="button"
          className="codex-menu-accounts__action"
          disabled={busy}
          onClick={() => void run(codexAccountAdd, true)}
        >
          {t(needsAuthentication && !ambient ? "CodexAccountsSignInButton" : "CodexAccountsAddButton")}
        </button>
      </>}
    >

      {pending && (
        <div className="codex-menu-accounts__usage" role="status">
          {t(signingIn ? "CodexAccountsSigningIn" : "TrayLoading")}
        </div>
      )}
      {error && (
        <div className="codex-menu-accounts__error" role="alert">
          {error}
        </div>
      )}
      {accounts.length > 0 && (
        <ul className="codex-menu-accounts__list">
          {accounts.map((account) => {
            const label = accountDisplayNames[account.id];
            return (
              <CodexAccountRow
                key={account.id}
                account={account}
                snapshot={snapshots[account.id]}
                displayName={label}
                tooltip={label}
                showAsUsed={showAsUsed}
                resetTimeRelative={resetTimeRelative}
                busy={busy}
                needsAuthentication={accountNeedsAuthentication[account.id] ?? (account.source === "ambient" && needsAuthentication)}
                onSwitch={handleSwitch}
                onReauthenticate={(id) => run(() => codexAccountReauthenticate(id), true)}
              />
            );
          })}
        </ul>
      )}
    </ProviderAccountsMenu>
  );
}

function CodexAccountRow({
  account,
  snapshot,
  displayName,
  tooltip,
  showAsUsed,
  resetTimeRelative,
  busy,
  needsAuthentication,
  onSwitch,
  onReauthenticate,
}: {
  account: CodexAccount;
  snapshot: CodexAccountUsageSnapshot | undefined;
  displayName: string;
  tooltip: string;
  showAsUsed: boolean;
  resetTimeRelative: boolean;
  busy: boolean;
  needsAuthentication: boolean;
  onSwitch: (id: string) => Promise<void>;
  onReauthenticate: (id: string) => Promise<void>;
}) {
  const { t } = useLocale();
  const isAmbient = account.source === "ambient";

  return (
    <li>
      <div
        className={`codex-menu-accounts__row${isAmbient ? " codex-menu-accounts__row--active" : ""}`}
      >
        <div className="codex-menu-accounts__meta">
          <span className="codex-menu-accounts__email" title={tooltip}>
            {displayName}
            {isAmbient && (
              <span className="codex-menu-accounts__badge">
                {t("CodexAccountsSourceAmbient")}
              </span>
            )}
          </span>
          {snapshot?.plan && (
            <span className="codex-menu-accounts__usage">{snapshot.plan}</span>
          )}
          {snapshot?.primaryWindow && (
            <AccountWindow window={snapshot.primaryWindow} showAsUsed={showAsUsed} resetTimeRelative={resetTimeRelative} />
          )}
          {snapshot?.secondaryWindow && (
            <AccountWindow window={snapshot.secondaryWindow} showAsUsed={showAsUsed} resetTimeRelative={resetTimeRelative} />
          )}
          {!snapshot?.primaryWindow && !snapshot?.secondaryWindow && (
            <span className="codex-menu-accounts__usage">{t("CodexAccountsUsageUnavailable")}</span>
          )}
        </div>
        <div className="codex-menu-accounts__row-actions">
          {needsAuthentication && (
            <button
              type="button"
              className="codex-menu-accounts__switch"
              disabled={busy}
              aria-label={`${t("CodexAccountsReauthenticateButton")}: ${displayName}`}
              onClick={() => void onReauthenticate(account.id)}
            >
              {t("CodexAccountsReauthenticateButton")}
            </button>
          )}
          <button
            type="button"
            className="codex-menu-accounts__switch"
            disabled={busy || isAmbient}
            onClick={() => void onSwitch(account.id)}
          >
            {t("CodexAccountsSwitchButton")}
          </button>
        </div>
      </div>
    </li>
  );
}

function AccountWindow({
  window,
  resetTimeRelative,
  showAsUsed,
}: {
  showAsUsed: boolean;
  window: NonNullable<CodexAccountUsageSnapshot["primaryWindow"]>;
  resetTimeRelative: boolean;
}) {
  const { t } = useLocale();
  const known = Number.isFinite(window.usedPercent);
  const used = Math.max(0, Math.min(100, Math.round(window.usedPercent)));
  const remaining = 100 - used;
  const resetText = useFormattedResetTime(window.resetAt, null, resetTimeRelative);
  const resetLabel = resetText && (
    resetTimeRelative ? resetText : `${t("MetricResetsIn")} ${resetText}`
  );
  return (
    <div>
      <span className="codex-menu-accounts__usage">
        <span>{formatWindowLabel(window.limitWindowSeconds) ?? t("DetailWindowPrimary")}</span>
        <span>
          {known ? `${remaining}% ${t("PanelLeftSuffix")}` : t("CodexAccountsUsageUnavailable")}
        </span>
        {known && showAsUsed && <span>{used}% {t("PanelUsedSuffix")}</span>}
        {resetLabel && <span>{resetLabel}</span>}
      </span>
      <span className="codex-menu-accounts__bar" aria-hidden>
        {known && (
          <span className="codex-menu-accounts__bar-fill" style={{ width: `${remaining}%` }} />
        )}
      </span>
    </div>
  );
}
function formatWindowLabel(
  limitWindowSeconds: number | null | undefined,
): string | null {
  if (!limitWindowSeconds || limitWindowSeconds <= 0) return null;
  if (limitWindowSeconds % 86_400 === 0) {
    return `${limitWindowSeconds / 86_400}d`;
  }
  if (limitWindowSeconds % 3_600 === 0) {
    return `${limitWindowSeconds / 3_600}h`;
  }
  return null;
}
