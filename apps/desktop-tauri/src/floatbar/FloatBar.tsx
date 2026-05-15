import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useProviders } from "../hooks/useProviders";
import { getSettingsSnapshot } from "../lib/tauri";
import { ProviderIcon } from "../components/providers/ProviderIcon";
import { getProviderIcon } from "../components/providers/providerIcons";
import type {
  BootstrapState,
  ProviderUsageSnapshot,
  SettingsSnapshot,
} from "../types/bridge";
import { FLOAT_BAR_CONFIG_CHANGED_EVENT } from "./api";
import "./FloatBar.css";

/**
 * The capacity pill shown for a single provider.
 *
 * Color follows usage: green default, amber when remaining drops below the
 * high-usage threshold, red when remaining is below the critical threshold
 * or the provider is exhausted.
 */
function ProviderPill({
  provider,
  highRemaining,
  critRemaining,
}: {
  provider: ProviderUsageSnapshot;
  highRemaining: number;
  critRemaining: number;
}) {
  const remaining = Math.max(0, Math.min(100, provider.primary.remainingPercent));
  const exhausted = provider.primary.isExhausted || provider.error;
  let tone: "ok" | "warn" | "crit" = "ok";
  if (exhausted || remaining <= critRemaining) tone = "crit";
  else if (remaining <= highRemaining) tone = "warn";

  const brand = getProviderIcon(provider.providerId).brandColor;
  const label = provider.error ? "—" : `${Math.round(remaining)}%`;

  return (
    <div
      className={`floatbar__pill floatbar__pill--${tone}`}
      title={`${provider.displayName}: ${label} remaining`}
      style={{ "--brand": brand } as React.CSSProperties}
    >
      <ProviderIcon providerId={provider.providerId} size={11} />
      <span className="floatbar__pct">{label}</span>
    </div>
  );
}

/**
 * The always-on-top floating capacity bar.
 *
 * Renders a tiny strip of provider pills. Listens to the same provider
 * refresh cycle as the rest of the app via `useProviders`, and reacts to
 * setting changes (filter list, orientation) live without a reload.
 */
export default function FloatBar({ state }: { state: BootstrapState }) {
  const { providers } = useProviders();

  // Mark the body so our CSS can strip the dark theme background — the
  // floatbar window is meant to be fully transparent around the pills.
  useEffect(() => {
    document.body.classList.add("floatbar-window");
    return () => {
      document.body.classList.remove("floatbar-window");
    };
  }, []);

  // The floatbar window is detached, so it doesn't share React state
  // with the Settings tab. Listen for the Rust-side config-changed event
  // and re-pull the snapshot when fired.
  const [settings, setSettings] = useState<SettingsSnapshot>(state.settings);
  useEffect(() => {
    const unlisten = listen(FLOAT_BAR_CONFIG_CHANGED_EVENT, () => {
      void getSettingsSnapshot().then(setSettings).catch(() => {});
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  // Orientation flips re-lay-out the bar without recreating the window.
  const orientation: "horizontal" | "vertical" =
    settings.floatBarOrientation === "vertical" ? "vertical" : "horizontal";
  const filterIds = settings.floatBarProviderIds;
  const visible = useMemo(() => {
    const enabled = new Set(settings.enabledProviders);
    let list = providers.filter((p) => enabled.size === 0 || enabled.has(p.providerId));
    if (filterIds && filterIds.length > 0) {
      const wanted = new Set(filterIds);
      list = list.filter((p) => wanted.has(p.providerId));
    }
    return [...list].sort((a, b) => b.primary.usedPercent - a.primary.usedPercent);
  }, [providers, settings.enabledProviders, filterIds]);

  // Resize the window to fit content when the visible set or orientation changes.
  useEffect(() => {
    const win = getCurrentWindow();
    const el = document.querySelector<HTMLElement>(".floatbar");
    if (!el) return;
    requestAnimationFrame(() => {
      const rect = el.getBoundingClientRect();
      const padding = 8;
      const w = Math.ceil(rect.width + padding);
      const h = Math.ceil(rect.height + padding);
      void win.setSize({ type: "Logical", width: w, height: h } as never).catch(() => {});
    });
  }, [visible.length, orientation]);

  const highRemaining = 100 - settings.highUsageThreshold;
  const critRemaining = 100 - settings.criticalUsageThreshold;

  return (
    <div
      className={`floatbar floatbar--${orientation}`}
      data-tauri-drag-region
    >
      <div className="floatbar__handle" data-tauri-drag-region aria-hidden />
      {visible.length === 0 ? (
        <div className="floatbar__empty">No providers</div>
      ) : (
        visible.map((p) => (
          <ProviderPill
            key={p.providerId}
            provider={p}
            highRemaining={highRemaining}
            critRemaining={critRemaining}
          />
        ))
      )}
    </div>
  );
}
