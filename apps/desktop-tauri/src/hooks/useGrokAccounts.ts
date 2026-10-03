import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { GrokAccount, GrokAccountUsage, ProviderUsageSnapshot } from "../types/bridge";
import {
  grokAccountFetch,
  grokAccountsList,
} from "../lib/tauri";

export function useGrokAccounts({ reloadOnFocus = false }: { reloadOnFocus?: boolean } = {}) {
  const [accounts, setAccounts] = useState<GrokAccount[]>([]);
  const [usage, setUsage] = useState<Record<string, GrokAccountUsage>>({});
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(false);
  const reloadSequence = useRef(0);
  const mutationInFlight = useRef(false);

  const reportError = useCallback((value: unknown, sequence = reloadSequence.current) => {
    if (mounted.current && sequence === reloadSequence.current) setError(String(value));
  }, []);

  const reload = useCallback(async () => {
    const sequence = ++reloadSequence.current;
    try {
      const next = await grokAccountsList();
      if (!mounted.current || sequence !== reloadSequence.current) return false;
      setAccounts(next);
      setError(null);
      setUsage((previous) => Object.fromEntries(next.map((account) => [account.id, {
        ...(previous[account.id] ?? { usageAvailable: false, usedPercent: null, plan: null, windowMinutes: null, resetsAt: null }), status: "loading",
      }])));
      await Promise.all(
        next.map(async (account) => {
          let snapshot: GrokAccountUsage;
          try {
            snapshot = await grokAccountFetch(account.id);
          } catch {
            snapshot = { status: "failed", usageAvailable: false, usedPercent: null, plan: null, windowMinutes: null, resetsAt: null };
          }
          if (mounted.current && sequence === reloadSequence.current) {
            setUsage((previous) => ({ ...previous, [account.id]: snapshot }));
          }
        }),
      );
      if (!mounted.current || sequence !== reloadSequence.current) return false;
      return true;
    } catch (value) {
      reportError(value, sequence);
      return false;
    }
  }, [reportError]);

  useEffect(() => {
    mounted.current = true;
    const refresh = () => void reload();
    refresh();
    const unlisten = listen("grok-accounts-updated", refresh);
    const unlistenProvider = listen<ProviderUsageSnapshot>("provider-updated", ({ payload }) => {
      if (payload.providerId === "grok") refresh();
    });
    if (reloadOnFocus) window.addEventListener("focus", refresh);
    return () => {
      mounted.current = false;
      if (reloadOnFocus) window.removeEventListener("focus", refresh);
      void unlisten.then((dispose) => dispose()).catch(() => {});
      void unlistenProvider.then((dispose) => dispose()).catch(() => {});
    };
  }, [reload, reloadOnFocus, reportError]);

  const run = useCallback(
    async (
      operation: () => Promise<void>,
      onSuccess?: () => void,
      onFinally?: () => void,
    ) => {
      if (mutationInFlight.current) return false;
      mutationInFlight.current = true;
      setBusy(true);
      setError(null);
      try {
        await operation();
        if (!(await reload())) return false;
        if (mounted.current) onSuccess?.();
        return true;
      } catch (value) {
        reportError(value);
        return false;
      } finally {
        mutationInFlight.current = false;
        if (mounted.current) {
          setBusy(false);
          onFinally?.();
        }
      }
    },
    [reload, reportError],
  );

  return { accounts, usage, busy, error, reportError, reload, run };
}
