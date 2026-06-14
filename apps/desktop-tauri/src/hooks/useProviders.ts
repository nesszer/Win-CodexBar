import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  ProviderUsageSnapshot,
  RefreshCompletePayload,
} from "../types/bridge";
import {
  getCachedProviders,
  refreshProviders,
  refreshProvidersIfStale,
} from "../lib/tauri";

export interface UseProvidersOptions {
  /**
   * Delay the automatic stale-aware refresh on mount. Tray/menu surfaces use
   * this so opening the UI can paint and accept input before provider work
   * starts.
   */
  initialRefreshDelayMs?: number;
  /**
   * Whether mounting this hook should ask the backend for a stale-aware refresh.
   * Passive surfaces can turn this off when another timer already drives
   * freshness, while still receiving cached data and live provider events.
   */
  refreshOnMount?: boolean;
}

export interface UseProvidersResult {
  /** Current provider snapshots (updated live as each provider completes). */
  providers: ProviderUsageSnapshot[];
  /** True while a refresh cycle is in progress. */
  isRefreshing: boolean;
  /** Trigger a manual refresh. No-op if already refreshing. */
  refresh: () => void;
  /** Summary from the last completed refresh cycle, if any. */
  lastRefresh: RefreshCompletePayload | null;
  /** True when the hook has provider data that can stay visible during refresh. */
  hasCachedData: boolean;
  /** True after the initial cached-provider read has completed. */
  hasLoadedCache: boolean;
}

/**
 * Subscribe to live provider usage data.
 *
 * On mount the hook:
 *  1. Loads any cached providers already in AppState.
 *  2. Fires `refresh_providers` to kick off a fresh fetch cycle.
 *  3. Listens for `provider-updated` events and merges each snapshot
 *     into the local array (upsert by providerId).
 *  4. Listens for `refresh-started` / `refresh-complete` to track loading.
 */
export function useProviders(options: UseProvidersOptions = {}): UseProvidersResult {
  const [providers, setProviders] = useState<ProviderUsageSnapshot[]>([]);
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [lastRefresh, setLastRefresh] = useState<RefreshCompletePayload | null>(
    null,
  );
  const [hasLoadedCache, setHasLoadedCache] = useState(false);
  const refreshingRef = useRef(false);
  const pendingSnapshotsRef = useRef<Map<string, ProviderUsageSnapshot>>(new Map());
  const flushTimerRef = useRef<number | undefined>(undefined);

  const mergeSnapshots = useCallback((snapshots: ProviderUsageSnapshot[]) => {
    if (snapshots.length === 0) return;
    setProviders((prev) => {
      const next = [...prev];
      const byId = new Map(next.map((provider, index) => [provider.providerId, index]));
      for (const snapshot of snapshots) {
        const idx = byId.get(snapshot.providerId);
        if (idx !== undefined) {
          next[idx] = snapshot;
        } else {
          byId.set(snapshot.providerId, next.length);
          next.push(snapshot);
        }
      }
      return next;
    });
  }, []);

  const flushPendingSnapshots = useCallback(() => {
    if (flushTimerRef.current !== undefined) {
      window.clearTimeout(flushTimerRef.current);
      flushTimerRef.current = undefined;
    }
    const snapshots = Array.from(pendingSnapshotsRef.current.values());
    pendingSnapshotsRef.current.clear();
    mergeSnapshots(snapshots);
  }, [mergeSnapshots]);

  const queueSnapshot = useCallback((snapshot: ProviderUsageSnapshot) => {
    pendingSnapshotsRef.current.set(snapshot.providerId, snapshot);
    if (flushTimerRef.current !== undefined) return;
    flushTimerRef.current = window.setTimeout(flushPendingSnapshots, 80);
  }, [flushPendingSnapshots]);

  const refresh = useCallback(() => {
    if (refreshingRef.current) return;
    refreshingRef.current = true;
    setIsRefreshing(true);
    refreshProviders().catch(() => {
      refreshingRef.current = false;
      setIsRefreshing(false);
    });
  }, []);

  useEffect(() => {
    let cancelled = false;

    // Load existing cache first.
    getCachedProviders()
      .then((cached) => {
        if (!cancelled && cached.length > 0) {
          mergeSnapshots(cached);
        }
      })
      .finally(() => {
        if (!cancelled) {
          setHasLoadedCache(true);
        }
      });

    // Event listeners.
    const unlistenUpdated = listen<ProviderUsageSnapshot>(
      "provider-updated",
      (event) => {
        if (!cancelled) queueSnapshot(event.payload);
      },
    );

    const unlistenStarted = listen("refresh-started", () => {
      if (!cancelled) {
        refreshingRef.current = true;
        setIsRefreshing(true);
      }
    });

    const unlistenComplete = listen<RefreshCompletePayload>(
      "refresh-complete",
      (event) => {
        if (!cancelled) {
          flushPendingSnapshots();
          refreshingRef.current = false;
          setIsRefreshing(false);
          setLastRefresh(event.payload);
        }
      },
    );

    let initialRefreshTimer: number | undefined;

    const runInitialRefresh = () => {
      refreshProvidersIfStale().catch(() => {
        if (!cancelled) {
          refreshingRef.current = false;
          setIsRefreshing(false);
        }
      });
    };

    // Kick off the initial refresh, but let the backend reuse fresh cache.
    if (options.refreshOnMount !== false) {
      const delay = Math.max(0, options.initialRefreshDelayMs ?? 0);
      if (delay > 0) {
        initialRefreshTimer = window.setTimeout(runInitialRefresh, delay);
      } else {
        runInitialRefresh();
      }
    }

    return () => {
      cancelled = true;
      if (initialRefreshTimer !== undefined) {
        window.clearTimeout(initialRefreshTimer);
      }
      if (flushTimerRef.current !== undefined) {
        window.clearTimeout(flushTimerRef.current);
        flushTimerRef.current = undefined;
      }
      pendingSnapshotsRef.current.clear();
      unlistenUpdated.then((fn) => fn());
      unlistenStarted.then((fn) => fn());
      unlistenComplete.then((fn) => fn());
    };
  }, [
    options.initialRefreshDelayMs,
    options.refreshOnMount,
    flushPendingSnapshots,
    mergeSnapshots,
    queueSnapshot,
    refresh,
  ]);

  return {
    providers,
    isRefreshing,
    refresh,
    lastRefresh,
    hasCachedData: providers.length > 0,
    hasLoadedCache,
  };
}
