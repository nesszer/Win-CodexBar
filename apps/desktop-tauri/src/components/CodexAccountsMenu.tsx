import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  CodexAccount,
  CodexAccountsStateBridge,
  CodexAccountUsageSnapshot,
} from "../types/bridge";
import { useLocale } from "../hooks/useLocale";
import { useFormattedResetTime } from "../hooks/useFormattedResetTime";
import { buildCodexAccountSurfaceLabels } from "./codexAccountDisplay";
import {
  codexAccountSwitch,
  getCodexAccountsState,
  refreshProviders,
} from "../lib/tauri";

/**
 * Multi-account lane surface for the Codex tray menu card (ADR 0003,
 * option A). Renders only when more than one Codex account exists, so the
 * common single-account menu stays unchanged (single-account fallback).
 *
 * Shows every account (ambient + managed) with a compact usage bar and a
 * Switch action. Switching updates the ambient identity and triggers a
 * provider refresh so the tray icon/menu reflect the now-active account.
 */
export default function CodexAccountsMenu({
  hideEmail,
  resetTimeRelative,
  onLayoutChange,
}: {
  hideEmail: boolean;
  resetTimeRelative: boolean;
  onLayoutChange?: () => void;
}) {
  const { t } = useLocale();
  const [accounts, setAccounts] = useState<CodexAccount[]>([]);
  const [snapshots, setSnapshots] = useState<
    Record<string, CodexAccountUsageSnapshot>
  >({});
  const [displayNames, setDisplayNames] = useState<Record<string, string>>({});
  const [accountOrdinals, setAccountOrdinals] = useState<Record<string, number>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const next: CodexAccountsStateBridge = await getCodexAccountsState();
      setAccounts(next.accounts);
      setDisplayNames(next.displayNames ?? {});
      setAccountOrdinals(next.accountOrdinals);
      setSnapshots(next.snapshots);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    onLayoutChange?.();
  }, [accounts.length, error, onLayoutChange]);

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

  const handleSwitch = async (id: string) => {
    setBusy(true);
    setError(null);
    try {
      await codexAccountSwitch(id);
      await load();
      // Make the tray icon/menu reflect the newly active ambient identity.
      void refreshProviders().catch(() => {});
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  if (accounts.length <= 1) {
    return null;
  }

  const accountDisplayNames = buildCodexAccountSurfaceLabels(
    accounts,
    displayNames,
    accountOrdinals,
    hideEmail,
    t("Account"),
  );

  return (
    <details className="codex-menu-accounts" onToggle={onLayoutChange}>
      <summary className="codex-menu-accounts__summary">
        <span className="codex-menu-accounts__title">{t("CodexAccountsTitle")}</span>
        <span className="codex-menu-accounts__count">{accounts.length}</span>
      </summary>
      {error && (
        <div className="codex-menu-accounts__error" role="alert">
          {error}
        </div>
      )}
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
              resetTimeRelative={resetTimeRelative}
              busy={busy}
              onSwitch={handleSwitch}
            />
          );
        })}
      </ul>
    </details>
  );
}

function CodexAccountRow({
  account,
  snapshot,
  displayName,
  tooltip,
  resetTimeRelative,
  busy,
  onSwitch,
}: {
  account: CodexAccount;
  snapshot: CodexAccountUsageSnapshot | undefined;
  displayName: string;
  tooltip: string;
  resetTimeRelative: boolean;
  busy: boolean;
  onSwitch: (id: string) => Promise<void>;
}) {
  const { t } = useLocale();
  // Prefer the primary (normally five-hour) window. Accounts whose backend
  // only returns a weekly window have primaryWindow: null, so keep the
  // existing secondary-window fallback for their bar and reset detail.
  const usageWindow =
    snapshot?.primaryWindow ?? snapshot?.secondaryWindow ?? null;
  const pct = usageWindow ? Math.round(usageWindow.usedPercent) : null;
  const resetText = useFormattedResetTime(
    usageWindow?.resetAt ?? null,
    null,
    resetTimeRelative,
  );
  const resetLabel = resetText
    ? resetTimeRelative
      ? resetText
      : `${t("MetricResetsIn")} ${resetText}`
    : null;
  const windowLabel = formatWindowLabel(usageWindow?.limitWindowSeconds);
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
          {(pct !== null || resetLabel) && (
            <span className="codex-menu-accounts__usage">
              {windowLabel && <span>{windowLabel}</span>}
              {pct !== null && (
                <span>{pct}% {t("PanelUsedSuffix")}</span>
              )}
              {resetLabel && <span>{resetLabel}</span>}
            </span>
          )}
          {pct !== null && (
            <span className="codex-menu-accounts__bar" aria-hidden>
              <span
                className="codex-menu-accounts__bar-fill"
                style={{ width: `${Math.max(2, Math.min(100, pct))}%` }}
              />
            </span>
          )}
        </div>
        <button
          type="button"
          className="codex-menu-accounts__switch"
          disabled={busy || isAmbient}
          onClick={() => void onSwitch(account.id)}
        >
          {t("CodexAccountsSwitchButton")}
        </button>
      </div>
    </li>
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
