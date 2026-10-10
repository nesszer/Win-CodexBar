import { useCallback, useEffect, useRef, useState } from "react";
import type { SettingsSnapshot, SettingsUpdate } from "../types/bridge";
import { getSettingsSnapshot, updateSettings } from "../lib/tauri";
import { useTauriEvent } from "./useTauriEvent";

interface UseSettingsReturn {
  settings: SettingsSnapshot;
  saving: boolean;
  error: string | null;
  update: (patch: SettingsUpdate) => Promise<void>;
}

const SAVING_INDICATOR_DELAY_MS = 300;

/**
 * Patch fields whose snapshot value is not the patch value: the shortcut patch
 * holds overrides only while the snapshot holds the resolved map. They stay as
 * they are until the save response supplies the resolved value.
 */
const NOT_OPTIMISTIC: ReadonlySet<string> = new Set(["switcherShortcuts"]);

/** Copies the patch fields that also exist in the snapshot (write-only fields are skipped). */
function applyPatch(current: SettingsSnapshot, patch: SettingsUpdate): SettingsSnapshot {
  const next: Record<string, unknown> = { ...current };
  for (const [key, value] of Object.entries(patch)) {
    if (value !== undefined && key in current && !NOT_OPTIMISTIC.has(key)) next[key] = value;
  }
  // Accent colors are sent as a per-provider merge patch (`null` clears one
  // provider), not as the full map.
  if (patch.providerAccentColors) {
    const colors: Record<string, string> = { ...current.providerAccentColors };
    for (const [id, color] of Object.entries(patch.providerAccentColors)) {
      if (color === null) delete colors[id];
      else colors[id] = color;
    }
    next.providerAccentColors = colors;
  }
  // Metric preferences are merged per provider as well.
  if (patch.providerMetrics) {
    next.providerMetrics = { ...current.providerMetrics, ...patch.providerMetrics };
  }
  return next as unknown as SettingsSnapshot;
}

/**
 * Manages the current settings state and exposes a mutation helper that
 * persists changes through the Tauri bridge and refreshes the local copy.
 */
export function useSettings(initial: SettingsSnapshot): UseSettingsReturn {
  const [settings, setSettings] = useState<SettingsSnapshot>(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Saves started from this window that have not answered yet.
  const pendingSaves = useRef(0);
  const [anyPending, setAnyPending] = useState(false);

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

    return () => {
      cancelled = true;
    };
  }, [initial]);

  // Live-sync when settings change in ANOTHER window. The detached Settings
  // window and the tray-panel flyout are separate webviews with separate
  // React state, so the in-window CustomEvent below never reaches them. Rust
  // broadcasts "settings-changed" after every persisted update; re-fetch the
  // snapshot so this surface (e.g. the tray zoom) re-renders live.
  useTauriEvent("settings-changed", () => {
    getSettingsSnapshot()
      .then((fresh) => {
        // While a save from this window is pending, its response is the
        // newer state; an earlier save's broadcast must not undo it.
        if (pendingSaves.current === 0) setSettings(fresh);
      })
      .catch(() => {
        // Keep the current copy if the refresh fails.
      });
  }, []);

  // `saving` disables every control on the tab, which dims them to 50%
  // opacity. A local save finishes in a few milliseconds, so raising the flag
  // immediately made the whole tab blink on every checkbox click. Only report
  // `saving` once a save has been pending for a noticeable time.
  // Keyed on whether any save is pending, not on how many, so an overlapping
  // save does not restart the delay.
  useEffect(() => {
    if (!anyPending) {
      setSaving(false);
      return;
    }
    const timer = window.setTimeout(() => setSaving(true), SAVING_INDICATOR_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [anyPending]);

  // Responses of overlapping saves may arrive out of order; only the latest
  // one is applied so an older snapshot cannot undo a newer change.
  const latestRequest = useRef(0);

  const update = useCallback(async (patch: SettingsUpdate) => {
    const request = ++latestRequest.current;
    pendingSaves.current += 1;
    setAnyPending(true);
    setError(null);
    // Show the change right away instead of waiting for the round trip: the
    // controls are controlled, so without this a checkbox flips only after
    // the shell answers.
    setSettings((current) => applyPatch(current, patch));
    try {
      const next = await updateSettings(patch);
      if (request !== latestRequest.current) return;
      setSettings(next);
      if (typeof window !== "undefined") {
        window.dispatchEvent(
          new CustomEvent<SettingsSnapshot>("codexbar:settings-updated", {
            detail: next,
          }),
        );
      }
    } catch (err: unknown) {
      // A newer save is in flight; its outcome decides what is shown.
      if (request !== latestRequest.current) return;
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
      pendingSaves.current -= 1;
      if (pendingSaves.current === 0) setAnyPending(false);
    }
  }, []);

  return { settings, saving, error, update };
}
