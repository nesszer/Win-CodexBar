import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  CodexAccount,
  CodexAccountsStateBridge,
  CodexAccountUsageSnapshot,
} from "../types/bridge";
import { useLocale } from "../hooks/useLocale";
import { useFormattedResetTime } from "../hooks/useFormattedResetTime";
import { buildCodexAccountDisplayNames } from "./codexAccountDisplay";
import {
  codexAccountSwitch,
  getCodexAccountsState,
  refreshProviders,
} from "../lib/tauri";

export interface PrivateCodexAccountLabel {
  label: string;
  tooltip: string;
}

/**
 * Project a tray account label while keeping the privacy setting scoped to
 * this switcher surface. The shared display-name builder remains unchanged so
 * settings and other account-facing surfaces keep their existing behavior.
 */
export function buildPrivateCodexAccountLabel(
  account: CodexAccount,
  displayName: string,
  ordinal: number,
  hidePersonalInfo: boolean,
): PrivateCodexAccountLabel {
  if (hidePersonalInfo) {
    const label = `Account ${ordinal}`;
    return { label, tooltip: label };
  }

  const label = displayName || account.nickname || "Workspace";
  return { label, tooltip: label };
}

/**
 * Assign ordinals from the opaque stable account id rather than the current
 * discovery order. This keeps hidden labels stable when the backend refreshes
 * or reorders account rows.
 */
export function buildCodexAccountOrdinals(
  accounts: readonly CodexAccount[],
): Record<string, number> {
  const ordered = accounts
    .map((account, index) => ({ account, index }))
    .sort((left, right) => {
      const leftId = left.account.id.trim().toLowerCase();
      const rightId = right.account.id.trim().toLowerCase();
      if (leftId < rightId) return -1;
      if (leftId > rightId) return 1;
      return left.index - right.index;
    });

  const ordinals: Record<string, number> = {};
  ordered.forEach(({ account }, index) => {
    ordinals[account.id] = index + 1;
  });
  return ordinals;
}

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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const next: CodexAccountsStateBridge = await getCodexAccountsState();
      setAccounts(next.accounts);
      setDisplayNames(next.displayNames ?? {});
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

  const accountDisplayNames = buildCodexAccountDisplayNames(
    accounts,
    displayNames,
  );
  const accountOrdinals = buildCodexAccountOrdinals(accounts);

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
        {accounts.map((account, index) => {
          const privateLabel = buildPrivateCodexAccountLabel(
            account,
            accountDisplayNames[account.id] ?? "",
            accountOrdinals[account.id] ?? index + 1,
            hideEmail,
          );
          return (
            <CodexAccountRow
              key={account.id}
              account={account}
              snapshot={snapshots[account.id]}
              displayName={privateLabel.label}
              tooltip={privateLabel.tooltip}
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
