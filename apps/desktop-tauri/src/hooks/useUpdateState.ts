import { useCallback, useEffect, useState } from "react";
import type { UpdateStatePayload } from "../types/bridge";
import {
  checkForUpdates,
  downloadUpdate,
  applyUpdate,
  dismissUpdate,
  openReleasePage,
  getUpdateState,
} from "../lib/tauri";
import { useTauriEvent } from "./useTauriEvent";

export interface UseUpdateStateResult {
  /** Current update lifecycle state from the backend. */
  updateState: UpdateStatePayload;
  /** Trigger an update check manually. */
  checkNow: () => void;
  /** Start downloading an available update. */
  download: () => void;
  /** Launch the downloaded installer and exit. */
  apply: () => void;
  /** Dismiss the current update notification. */
  dismiss: () => void;
  /** Open the release page in the default browser. */
  openRelease: () => void;
}

const IDLE_PAYLOAD: UpdateStatePayload = {
  status: "idle",
  version: null,
  error: null,
  progress: null,
  releaseUrl: null,
  canDownload: false,
  canApply: false,
  lastCheckedAt: null,
};

/**
 * Subscribe to the backend update-state lifecycle.
 *
 * On mount the hook loads the current state, then listens for
 * `update-state-changed` events emitted by the Rust backend.
 */
export function useUpdateState(): UseUpdateStateResult {
  const [updateState, setUpdateState] =
    useState<UpdateStatePayload>(IDLE_PAYLOAD);

  const checkNow = useCallback(() => {
    checkForUpdates()
      .then(setUpdateState)
      .catch(() => {});
  }, []);

  const download = useCallback(() => {
    downloadUpdate()
      .then(setUpdateState)
      .catch(() => {});
  }, []);

  const apply = useCallback(() => {
    applyUpdate().catch(() => {});
  }, []);

  const dismiss = useCallback(() => {
    dismissUpdate()
      .then(setUpdateState)
      .catch(() => {});
  }, []);

  const openRelease = useCallback(() => {
    openReleasePage().catch(() => {});
  }, []);

  useEffect(() => {
    let cancelled = false;

    getUpdateState().then((s) => {
      if (!cancelled) setUpdateState(s);
    });

    return () => {
      cancelled = true;
    };
  }, []);

  useTauriEvent<UpdateStatePayload>("update-state-changed", (event) => setUpdateState(event.payload), []);

  return { updateState, checkNow, download, apply, dismiss, openRelease };
}
