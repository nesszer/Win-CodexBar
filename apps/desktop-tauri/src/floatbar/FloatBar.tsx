import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type CSSProperties,
  type MouseEvent,
} from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useFormattedResetTime } from "../hooks/useFormattedResetTime";
import { useProviders } from "../hooks/useProviders";
import { getSettingsSnapshot, refreshProvidersIfStale } from "../lib/tauri";
import { ProviderIcon } from "../components/providers/ProviderIcon";
import { getProviderIcon } from "../components/providers/providerIcons";
import type {
  BootstrapState,
  ProviderUsageSnapshot,
  SettingsSnapshot,
} from "../types/bridge";
import { FLOAT_BAR_CONFIG_CHANGED_EVENT, resizeFloatBar } from "./api";
import "./FloatBar.css";

function ResetIcon({ size }: { size: number }) {
  return (
    <svg
      className="floatbar__reset-icon-svg"
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      aria-hidden="true"
    >
      <path
        d="M12.9 7.1a5 5 0 1 0-1.2 3.9"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
      />
      <path
        d="M12.9 3.8v3.3H9.6"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function inlineResetTime(resetText: string): string {
  const normalized = resetText.trim();
  if (/^reset(?:s|ting)?(?:\s+due)?\s*(?:now)?$/i.test(normalized)) {
    return "now";
  }
  return normalized
    .replace(/^resets?\s+in\s+/i, "")
    .replace(/^resets?\s+/i, "")
    .trim();
}

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
  showAsUsed,
  scale,
  showResetInline,
  resetRelative,
}: {
  provider: ProviderUsageSnapshot;
  highRemaining: number;
  critRemaining: number;
  showAsUsed: boolean;
  scale: number;
  showResetInline: boolean;
  resetRelative: boolean;
}) {
  const remaining = Math.max(0, Math.min(100, provider.primary.remainingPercent));
  const used = Math.max(0, Math.min(100, provider.primary.usedPercent));
  const displayPercent = showAsUsed ? used : remaining;
  const displaySuffix = showAsUsed ? "used" : "remaining";
  const exhausted = provider.primary.isExhausted || provider.error;
  let tone: "ok" | "warn" | "crit" = "ok";
  if (exhausted || remaining <= critRemaining) tone = "crit";
  else if (remaining <= highRemaining) tone = "warn";

  const brand = getProviderIcon(provider.providerId).brandColor;
  const label = provider.error ? "—" : `${Math.round(displayPercent)}%`;
  const resetText = useFormattedResetTime(
    provider.primary.resetsAt,
    provider.primary.resetDescription,
    resetRelative,
  );
  const resetSuffix = resetText ? `\n${resetText}` : "";
  const inlineReset = resetText ? inlineResetTime(resetText) : null;
  const iconSize = Math.round(11 * scale);
  const resetIconSize = Math.round(10 * scale);

  return (
    <div
      className={`floatbar__pill floatbar__pill--${tone}`}
      title={`${provider.displayName}: ${label} ${displaySuffix}${resetSuffix}`}
      data-tauri-drag-region
      style={{ "--brand": brand } as CSSProperties}
    >
      <span className="floatbar__provider-icon" data-tauri-drag-region>
        <ProviderIcon providerId={provider.providerId} size={iconSize} />
      </span>
      <span className="floatbar__text" data-tauri-drag-region>
        <span className="floatbar__pct" data-tauri-drag-region>
          {label}
        </span>
        {showResetInline && resetText && inlineReset && (
          <span
            className="floatbar__reset"
            title={resetText}
            aria-label={resetText}
            data-tauri-drag-region
          >
            <ResetIcon size={resetIconSize} />
            <span className="floatbar__reset-time" data-tauri-drag-region>
              {inlineReset}
            </span>
          </span>
        )}
      </span>
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
  const { providers } = useProviders({ refreshOnMount: false });
  const startDrag = useCallback((event: MouseEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    void getCurrentWindow().startDragging().catch(() => {});
  }, []);

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

  // The Tauri shell has no global refresh timer — providers only update
  // when something explicitly asks for it. Drive our own tick here so the
  // bar reflects fresh data even when the tray panel is closed.
  // `refreshProvidersIfStale` is a no-op when the backend cache is fresh,
  // so this is safe to call frequently.
  useEffect(() => {
    const intervalMs = Math.max(60_000, settings.refreshIntervalSecs * 1000);
    const tick = () => {
      void refreshProvidersIfStale().catch(() => {});
    };
    tick();
    const id = setInterval(tick, intervalMs);
    return () => clearInterval(id);
  }, [settings.refreshIntervalSecs]);
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
  const style = settings.floatBarStyle === "taskbar" ? "taskbar" : "floating";
  const filterIds = settings.floatBarProviderIds;
  const scale = Math.max(0.75, Math.min(2, settings.floatBarScale / 100));
  const showResetInline = settings.floatBarShowResetInline;
  const visible = useMemo(() => {
    const enabled = new Set(settings.enabledProviders);
    let list = providers.filter((p) => enabled.has(p.providerId));
    if (filterIds && filterIds.length > 0) {
      const wanted = new Set(filterIds);
      list = list.filter((p) => wanted.has(p.providerId));
    }
    return [...list].sort((a, b) => b.primary.usedPercent - a.primary.usedPercent);
  }, [providers, settings.enabledProviders, filterIds]);

  // Resize the window to fit content when the visible set or orientation
  // changes. The native `resize_float_bar` command owns both the size change
  // and the Win32 interaction state, so the webview only reports a target size.
  useEffect(() => {
    const el = document.querySelector<HTMLElement>(".floatbar");
    if (!el) return;
    requestAnimationFrame(() => {
      const rect = el.getBoundingClientRect();
      const padding = 8;
      const w = Math.ceil(rect.width + padding);
      const h = Math.ceil(rect.height + padding);
      void resizeFloatBar(w, h).catch(() => {});
    });
  }, [
    visible.length,
    orientation,
    style,
    scale,
    showResetInline,
    settings.resetTimeRelative,
  ]);

  const highRemaining = 100 - settings.highUsageThreshold;
  const critRemaining = 100 - settings.criticalUsageThreshold;
  const opacityFraction = Math.max(0.3, Math.min(1, settings.floatBarOpacity / 100));

  return (
    <div
      className={`floatbar floatbar--${orientation} floatbar--${style}${settings.floatBarDarkText ? " floatbar--light-bg" : ""}`}
      data-tauri-drag-region
      onMouseDown={startDrag}
      style={
        {
          opacity: opacityFraction,
          "--floatbar-scale": scale,
        } as CSSProperties
      }
    >
      <div className="floatbar__handle" data-tauri-drag-region aria-hidden />
      {visible.length === 0 ? (
        <div className="floatbar__empty" data-tauri-drag-region>
          No providers
        </div>
      ) : (
        visible.map((p) => (
          <ProviderPill
            key={p.providerId}
            provider={p}
            highRemaining={highRemaining}
            critRemaining={critRemaining}
            showAsUsed={settings.showAsUsed}
            scale={scale}
            showResetInline={showResetInline}
            resetRelative={settings.resetTimeRelative}
          />
        ))
      )}
    </div>
  );
}
