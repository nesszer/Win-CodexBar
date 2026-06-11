import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { SettingsSnapshot, SettingsUpdate } from "../types/bridge";
import { getSettingsSnapshot, updateSettings } from "../lib/tauri";

interface UseSettingsReturn {
  settings: SettingsSnapshot;
  saving: boolean;
  error: string | null;
  update: (patch: SettingsUpdate) => Promise<void>;
}

/**
 * Manages the current settings state and exposes a mutation helper that
 * persists changes through the Tauri bridge and refreshes the local copy.
 */
export function useSettings(initial: SettingsSnapshot): UseSettingsReturn {
  const [settings, setSettings] = useState<SettingsSnapshot>(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;

    setSettings(initial);

    getSettingsSnapshot()
      .then((fresh) => {
        if (!cancelled) {
          setSettings(fresh);
        }
      })
      .catch(() => {
        // Keep the bootstrap snapshot if the background sync fails.
      });

    const onDomSettingsUpdated = (event: Event) => {
      if (cancelled) return;
      const next = (event as CustomEvent<SettingsSnapshot>).detail;
      if (next) {
        setSettings(next);
      }
    };

    window.addEventListener("codexbar:settings-updated", onDomSettingsUpdated);

    const unlistenSettingsUpdated = listen<SettingsSnapshot>(
      "settings-updated",
      (event) => {
        if (!cancelled) {
          setSettings(event.payload);
        }
      },
    );

    return () => {
      cancelled = true;
      window.removeEventListener("codexbar:settings-updated", onDomSettingsUpdated);
      unlistenSettingsUpdated.then((fn) => fn());
    };
  }, [initial]);

  const update = useCallback(async (patch: SettingsUpdate) => {
    setSaving(true);
    setError(null);
    try {
      const next = await updateSettings(patch);
      setSettings(next);
      if (typeof window !== "undefined") {
        window.dispatchEvent(
          new CustomEvent<SettingsSnapshot>("codexbar:settings-updated", {
            detail: next,
          }),
        );
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(msg);
      // Re-fetch to stay in sync with disk state on failure
      try {
        const fresh = await getSettingsSnapshot();
        setSettings(fresh);
      } catch {
        // ignore secondary failure
      }
    } finally {
      setSaving(false);
    }
  }, []);

  return { settings, saving, error, update };
}
