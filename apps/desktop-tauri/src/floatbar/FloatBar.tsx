import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent,
} from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useFormattedResetTime } from "../hooks/useFormattedResetTime";
import { useCurrency } from "../hooks/CurrencyProvider";
import { useLocale } from "../hooks/useLocale";
import { useProviders } from "../hooks/useProviders";
import {
  getProviderLocalUsageSummary,
  getSettingsSnapshot,
  refreshProvidersIfStale,
} from "../lib/tauri";
import { ProviderIcon } from "../components/providers/ProviderIcon";
import { getProviderIcon } from "../components/providers/providerIcons";
import { costPeriodShortLabel } from "../lib/costPeriod";
import { describeProviderState } from "../lib/providerState";
import { resetDescriptionFallback, windowDetailText } from "../lib/usageWindows";
import type {
  BootstrapState,
  ProviderLocalUsageSummary,
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

function inlineResetTime(
  resetText: string,
  resetsAt: string | null,
  relative: boolean,
): string {
  if (relative && resetsAt) {
    const target = Date.parse(resetsAt);
    if (!Number.isNaN(target)) {
      const diffMs = target - Date.now();
      if (diffMs <= 0) return "now";

      const totalMinutes = Math.max(1, Math.floor(diffMs / 60_000));
      const days = Math.floor(totalMinutes / 1440);
      const hours = Math.floor((totalMinutes % 1440) / 60);
      const minutes = totalMinutes % 60;
      if (days > 0) return `${days}d ${hours}h`;
      if (hours > 0) return `${hours}h ${minutes}m`;
      return `${minutes}m`;
    }
  }

  const normalized = resetText.trim();
  if (/^reset(?:s|ting)?(?:\s+due)?\s*(?:now)?$/i.test(normalized)) {
    return "now";
  }
  return normalized
    .replace(/^resets?\s+in\s+/i, "")
    .replace(/^resets?\s+/i, "")
    .trim();
}

type FloatBarCostSummary = {
  key: string;
  providerId: string;
  displayName: string;
  todayCost: number | null;
  /** Cost over the selected History window. */
  periodCost: number | null;
  /** Raw History window `periodCost` covers. */
  period: string;
};

type FloatBarCostTarget = {
  key: string;
  providerId: string;
  displayName: string;
};

function providerCostKey(provider: ProviderUsageSnapshot): string {
  return `${provider.providerId}:${provider.accountEmail ?? ""}`;
}

function hasLocalCost(summary: ProviderLocalUsageSummary | null): summary is ProviderLocalUsageSummary {
  return summary?.todayCost != null || summary?.periodCost != null;
}

function CostPill({
  summary,
  scale,
  todayLabel,
  periodLabel,
  estimateLabel,
}: {
  summary: FloatBarCostSummary;
  scale: number;
  todayLabel: string;
  periodLabel: string;
  estimateLabel: string;
}) {
  const { format } = useCurrency();
  const today = summary.todayCost == null ? null : format(summary.todayCost, "USD");
  const periodCost = summary.periodCost == null ? null : format(summary.periodCost, "USD");
  const iconSize = Math.round(10 * scale);
  const brand = getProviderIcon(summary.providerId).brandColor;
  const title = [
    today ? `${todayLabel} ${today}` : null,
    periodCost ? `${periodLabel} ${periodCost}` : null,
  ]
    .filter(Boolean)
    .join(" / ");

  return (
    <div
      className="floatbar__cost-pill"
      title={`${summary.displayName}: ${title} (${estimateLabel})`}
      data-tauri-drag-region
      style={{ "--brand": brand } as CSSProperties}
    >
      <span className="floatbar__provider-icon" data-tauri-drag-region>
        <ProviderIcon providerId={summary.providerId} size={iconSize} />
      </span>
      <span className="floatbar__cost-items" data-tauri-drag-region>
        {today && (
          <span className="floatbar__cost-item" data-tauri-drag-region>
            <span className="floatbar__cost-label" data-tauri-drag-region>
              {todayLabel}
            </span>
            <span className="floatbar__cost-value" data-tauri-drag-region>
              {today}
            </span>
          </span>
        )}
        {periodCost && (
          <span className="floatbar__cost-item" data-tauri-drag-region>
            <span className="floatbar__cost-label" data-tauri-drag-region>
              {periodLabel}
            </span>
            <span className="floatbar__cost-value" data-tauri-drag-region>
              {periodCost}
            </span>
          </span>
        )}
      </span>
      <span className="floatbar__cost-estimate" data-tauri-drag-region>
        {estimateLabel}
      </span>
    </div>
  );
}
/**
 * The capacity pill shown for a single provider.
 *
 * Color follows usage: green default, amber when remaining drops below the
 * high-usage threshold, red when remaining is below the critical threshold
 * or the provider is exhausted.
 *
 * An informational metric (no budget set, a balance line, no active session)
 * has no quota percentage. Like the tray card and the tray tooltip, the pill
 * shows its text instead, keeps the neutral tone, and never turns that text
 * into reset wording.
 */
function ProviderPill({
  provider,
  highRemaining,
  critRemaining,
  showAsUsed,
  scale,
  showResetInline,
  resetRelative,
  usedSuffix,
  remainingSuffix,
  stateLabel,
}: {
  provider: ProviderUsageSnapshot;
  highRemaining: number;
  critRemaining: number;
  showAsUsed: boolean;
  scale: number;
  showResetInline: boolean;
  resetRelative: boolean;
  usedSuffix: string;
  remainingSuffix: string;
  stateLabel: string;
}) {
  const rateWindow = provider.selectedMetric;
  const informational = rateWindow.isInformational === true;
  const remaining = Math.max(0, Math.min(100, rateWindow.remainingPercent));
  const used = Math.max(0, Math.min(100, rateWindow.usedPercent));
  const displayPercent = showAsUsed ? used : remaining;
  const displaySuffix = showAsUsed ? usedSuffix : remainingSuffix;
  const state = describeProviderState(provider.errorState);
  let tone: "ok" | "warn" | "crit" = "ok";
  if (state.isProblem) tone = "crit";
  else if (!informational) {
    if (rateWindow.isExhausted || remaining <= critRemaining) tone = "crit";
    else if (remaining <= highRemaining) tone = "warn";
  }

  const brand = getProviderIcon(provider.providerId).brandColor;
  const infoText = rateWindow.resetDescription?.trim() || "—";
  const label = state.isProblem
    ? stateLabel
    : informational
      ? infoText
      : `${Math.round(displayPercent)}%`;
  const lastPercentageLabel = useRef("0%");
  if (!state.isProblem) lastPercentageLabel.current = label;
  const resetText = useFormattedResetTime(
    rateWindow.resetsAt,
    informational ? null : resetDescriptionFallback(rateWindow),
    resetRelative,
  );
  const detailText = windowDetailText(rateWindow);
  const resetSuffix = resetText ? `\n${resetText}` : "";
  const detailSuffix = detailText ? `\n${detailText}` : "";
  const inlineReset = resetText
    ? inlineResetTime(resetText, rateWindow.resetsAt, resetRelative)
    : null;
  const iconSize = Math.round(11 * scale);
  const resetIconSize = Math.round(10 * scale);

  return (
    <div
      className={`floatbar__pill floatbar__pill--${tone}`}
      title={
        state.isProblem
          ? `${provider.displayName}: ${stateLabel}`
          : informational
            ? `${provider.displayName}: ${infoText}${resetSuffix}`
            : `${provider.displayName}: ${label} ${displaySuffix}${resetSuffix}${detailSuffix}`
      }
      data-tauri-drag-region
      style={{ "--brand": brand } as CSSProperties}
    >
      <span className="floatbar__provider-icon" data-tauri-drag-region>
        <ProviderIcon providerId={provider.providerId} size={iconSize} />
      </span>
      <span className="floatbar__text" data-tauri-drag-region>
        <span className="floatbar__pct" aria-label={state.isProblem ? stateLabel : undefined} data-tauri-drag-region>
          {state.isProblem ? (
            <>
              <span aria-hidden="true" style={{ visibility: "hidden" }}>{lastPercentageLabel.current}</span>
              <span className="floatbar__warning" aria-hidden="true">!</span>
            </>
          ) : label}
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
  const { t } = useLocale();
  const { providers } = useProviders({
    refreshOnMount: false,
  });
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

  // Local settings: event stream is source of truth after mount; re-seed
  // if the bootstrap prop identity changes (rare parent remount path).
  const [settings, setSettings] = useState(state.settings);
  const [settingsSeed, setSettingsSeed] = useState(state.settings);
  if (state.settings !== settingsSeed) {
    setSettingsSeed(state.settings);
    setSettings(state.settings);
  }
  const [localCosts, setLocalCosts] = useState<Record<string, FloatBarCostSummary>>({});

  // The detached floatbar should keep usage fresh, but it must not open or
  // focus any other surface. Refresh data only; provider-updated events feed
  // this window when the backend completes. Respect Low Power Mode's 30-min
  // floor for automatic ticks (manual refresh stays elsewhere/immediate).
  useEffect(() => {
    const baseMs = Math.max(60_000, settings.refreshIntervalSecs * 1000);
    const intervalMs = settings.lowPowerMode
      ? Math.max(baseMs, 30 * 60 * 1000)
      : baseMs;
    const tick = () => {
      void refreshProvidersIfStale().catch(() => {});
    };
    tick();
    const id = setInterval(tick, intervalMs);
    return () => clearInterval(id);
  }, [settings.refreshIntervalSecs, settings.lowPowerMode]);

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
  const showCost = settings.floatBarShowCost;
  const visible = useMemo(() => {
    const enabled = new Set(settings.enabledProviders);
    let list = providers.filter((p) => enabled.has(p.providerId));
    if (filterIds && filterIds.length > 0) {
      const wanted = new Set(filterIds);
      list = list.filter((p) => wanted.has(p.providerId));
    }
    return [...list].sort(
      (a, b) =>
        b.selectedMetric.usedPercent - a.selectedMetric.usedPercent,
    );
  }, [providers, settings.enabledProviders, filterIds]);

  const visibleCostTargets = useMemo<FloatBarCostTarget[]>(
    () =>
      showCost
        ? visible.map((provider) => ({
            key: providerCostKey(provider),
            providerId: provider.providerId,
            displayName: provider.displayName,
          }))
        : [],
    [showCost, visible],
  );

  useEffect(() => {
    let cancelled = false;
    const targets = visibleCostTargets;

    if (targets.length === 0) {
      setLocalCosts({});
      return () => {
        cancelled = true;
      };
    }

    Promise.allSettled(
      targets.map(async (target) => {
        const localUsage = await getProviderLocalUsageSummary(target.providerId);
        if (!hasLocalCost(localUsage)) return null;
        return {
          key: target.key,
          providerId: target.providerId,
          displayName: target.displayName,
          todayCost: localUsage.todayCost,
          periodCost: localUsage.periodCost,
          period: localUsage.reportingPeriod,
        } satisfies FloatBarCostSummary;
      }),
    )
      .then((results) => {
        if (cancelled) return;
        const next: Record<string, FloatBarCostSummary> = {};
        for (const result of results) {
          if (result.status === "fulfilled" && result.value) {
            next[result.value.key] = result.value;
          }
        }
        setLocalCosts(next);
      })
      .catch(() => {
        if (!cancelled) setLocalCosts({});
      });

    return () => {
      cancelled = true;
    };
    // A History window change re-reads the local usage summaries.
  }, [visibleCostTargets, settings.costReportingPeriod]);

  const visibleCosts = visible
    .map((provider) => localCosts[providerCostKey(provider)])
    .filter((summary): summary is FloatBarCostSummary => Boolean(summary));
  const visibleCostValuesKey = visibleCosts
    .map((summary) => `${summary.key}:${summary.todayCost ?? ""}:${summary.period ?? ""}:${summary.periodCost ?? ""}`)
    .join("|");
  // Keep the native floatbar window fitted when late data/fonts/icons change layout.
  const lastResizeRef = useRef<{ w: number; h: number } | null>(null);
  const resizeRafRef = useRef<number | null>(null);
  const resizeToContent = useCallback(() => {
    const el = document.querySelector<HTMLElement>(".floatbar");
    if (!el) return;
    if (resizeRafRef.current !== null) {
      cancelAnimationFrame(resizeRafRef.current);
    }
    resizeRafRef.current = requestAnimationFrame(() => {
      resizeRafRef.current = null;
      const rect = el.getBoundingClientRect();
      const padding = 8;
      const dpr =
        Number.isFinite(window.devicePixelRatio) && window.devicePixelRatio > 0
          ? window.devicePixelRatio
          : 1;
      // DOM measurements are CSS pixels; the native command accepts physical
      // pixels so the window remains correctly sized on scaled displays.
      const w = Math.ceil(Math.ceil(rect.width + padding) * dpr);
      const h = Math.ceil(Math.ceil(rect.height + padding) * dpr);
      const last = lastResizeRef.current;
      if (last && Math.abs(last.w - w) <= 1 && Math.abs(last.h - h) <= 1) return;
      lastResizeRef.current = { w, h };
      void resizeFloatBar(w, h).catch(() => {});
    });
  }, []);

  useEffect(() => {
    resizeToContent();
  }, [
    resizeToContent,
    visible.length,
    visibleCostValuesKey,
    orientation,
    style,
    scale,
    showResetInline,
    settings.resetTimeRelative,
  ]);

  useEffect(() => {
    const el = document.querySelector<HTMLElement>(".floatbar");
    if (!el || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(resizeToContent);
    observer.observe(el);
    return () => observer.disconnect();
  }, [resizeToContent]);

  useEffect(() => {
    // Re-measure after moving onto a monitor with a different scale factor.
    window.addEventListener("resize", resizeToContent);
    return () => window.removeEventListener("resize", resizeToContent);
  }, [resizeToContent]);

  useEffect(
    () => () => {
      if (resizeRafRef.current !== null) {
        cancelAnimationFrame(resizeRafRef.current);
      }
    },
    [],
  );

  const highRemaining = 100 - settings.highUsageThreshold;
  const critRemaining = 100 - settings.criticalUsageThreshold;
  const opacityFraction = Math.max(0.3, Math.min(1, settings.floatBarOpacity / 100));

  return (
    <div
      role="button"
      tabIndex={-1}
      aria-label={t("AppName")}
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
          {t("FloatBarNoProviders")}
        </div>
      ) : (
        <>
          {visible.map((p) => (
            <ProviderPill
              key={providerCostKey(p)}
              provider={p}
              highRemaining={highRemaining}
              critRemaining={critRemaining}
              showAsUsed={settings.showAsUsed}
              scale={scale}
              showResetInline={showResetInline}
              resetRelative={settings.resetTimeRelative}
              usedSuffix={t("PanelUsedSuffix")}
              remainingSuffix={t("FloatBarRemainingSuffix")}
              stateLabel={t(describeProviderState(p.errorState).labelKey)}
            />
          ))}
          {visibleCosts.map((summary) => (
            <CostPill
              key={`cost:${summary.key}`}
              summary={summary}
              scale={scale}
              todayLabel={t("PanelToday")}
              periodLabel={costPeriodShortLabel(summary.period, t)}
              estimateLabel={t("OverviewSpendEstimate")}
            />
          ))}
        </>
      )}
    </div>
  );
}
