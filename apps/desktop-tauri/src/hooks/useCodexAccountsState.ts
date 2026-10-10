import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { CodexAccountsStateBridge, CodexAccountUsageSnapshot } from "../types/bridge";
import { getCodexAccountsState } from "../lib/tauri";

type CodexAccountsView = Required<
  Pick<
    CodexAccountsStateBridge,
    "accounts" | "snapshots" | "displayNames" | "accountOrdinals" | "needsAuthentication"
  >
>;

// Module constants so effects keyed on these fields stay quiet until the first load lands.
const EMPTY: CodexAccountsView = {
  accounts: [],
  snapshots: {},
  displayNames: {},
  accountOrdinals: {},
  needsAuthentication: {},
};

/**
 * The shared Codex account store (`get_codex_accounts_state`), reloaded on
 * `codex-accounts-updated`. `error` and `setError` are shared with the caller's
 * own actions so one alert line shows the latest failure.
 */
export function useCodexAccountsState() {
  const [view, setView] = useState<CodexAccountsView | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await getCodexAccountsState();
      setView({
        accounts: next.accounts,
        snapshots: next.snapshots,
        displayNames: next.displayNames ?? {},
        accountOrdinals: next.accountOrdinals,
        needsAuthentication: next.needsAuthentication ?? {},
      });
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
    let cancelled = false;
    const unlistenPromise = listen("codex-accounts-updated", () => {
      if (!cancelled) void load();
    });
    return () => {
      cancelled = true;
      void unlistenPromise.then((fn) => fn());
    };
  }, [load]);

  const setSnapshot = useCallback((id: string, snapshot: CodexAccountUsageSnapshot) => {
    setView((prev) => prev && { ...prev, snapshots: { ...prev.snapshots, [id]: snapshot } });
  }, []);

  return { ...(view ?? EMPTY), loaded: view !== null, loading, error, setError, load, setSnapshot };
}
