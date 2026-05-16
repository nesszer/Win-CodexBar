import { useCallback, useEffect, useMemo } from "react";
import type { BootstrapState, ProviderUsageSnapshot } from "../types/bridge";
import { setSurfaceMode, openSettingsWindow, quitApp as quitApplication } from "../lib/tauri";
import { useProviders } from "../hooks/useProviders";
import { useSettings } from "../hooks/useSettings";
import { useUpdateState } from "../hooks/useUpdateState";
import { useLocale } from "../hooks/useLocale";
import MenuCard from "../components/MenuCard";
import MenuSurface, {
  MenuSummary,
  MenuEmpty,
  type MenuFooterRow,
} from "../components/MenuSurface";
import UpdateBanner from "../components/UpdateBanner";

/** Sort: highest primary used% first, then alphabetical by name. */
function sortProviders(
  list: ProviderUsageSnapshot[],
): ProviderUsageSnapshot[] {
  return [...list].sort((a, b) => {
    const diff = b.primary.usedPercent - a.primary.usedPercent;
    if (Math.abs(diff) > 0.01) return diff;
    return a.displayName.localeCompare(b.displayName);
  });
}

/**
 * Pop-out window — same card stack as the tray (per upstream parity),
 * just hosted in a detached resizable window with a slightly wider
 * surface variant. If `providerId` is supplied (deep-link) we scroll
 * that card into view, but every provider card is always rendered.
 */
export default function PopOutPanel({
  state,
  providerId,
}: {
  state: BootstrapState;
  providerId?: string;
}) {
  const {
    providers,
    isRefreshing,
    refresh,
    lastRefresh,
    hasCachedData,
  } = useProviders();
  const { settings } = useSettings(state.settings);
  const { updateState, checkNow, download, apply, dismiss, openRelease } =
    useUpdateState();
  const { t } = useLocale();

  const sorted = useMemo(() => sortProviders(providers), [providers]);
  const errorCount = useMemo(
    () => sorted.filter((p) => p.error !== null).length,
    [sorted],
  );

  const openSettings = useCallback(() => {
    openSettingsWindow("general");
  }, []);
  const goTray = useCallback(() => {
    setSurfaceMode("trayPanel", { kind: "summary" });
  }, []);
  const openAbout = useCallback(() => {
    openSettingsWindow("about");
  }, []);
  const quitApp = useCallback(() => {
    void quitApplication();
  }, []);

  const headerActions = [
    { icon: "⊟", title: t("TooltipBackToTray"), onClick: goTray },
  ];

  const footerRows: MenuFooterRow[] = [
    { icon: "⚙", label: t("TooltipSettings"), shortcut: "Ctrl+,", onClick: openSettings },
    { icon: "ℹ", label: "About CodexBar", onClick: openAbout },
    { icon: "✕", label: "Quit", shortcut: "Ctrl+Q", onClick: quitApp },
  ];

  // Keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!e.ctrlKey || e.shiftKey || e.altKey || e.metaKey) return;
      switch (e.key.toLowerCase()) {
        case "r":
          e.preventDefault();
          refresh();
          break;
        case ",":
          e.preventDefault();
          openSettings();
          break;
        case "q":
          e.preventDefault();
          quitApp();
          break;
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [refresh, openSettings, quitApp]);

  const banner = (
    <UpdateBanner
      updateState={updateState}
      onCheck={checkNow}
      onDownload={download}
      onApply={apply}
      onDismiss={dismiss}
      onOpenRelease={openRelease}
    />
  );

  if (sorted.length === 0) {
    return (
      <MenuSurface
        variant="popout"
        onRefresh={refresh}
        isRefreshing={isRefreshing}
        actions={headerActions}
        banner={banner}
        footerRows={footerRows}
      >
        <MenuEmpty
          isLoading={isRefreshing && !hasCachedData}
          onSettings={openSettings}
        />
      </MenuSurface>
    );
  }

  return (
    <MenuSurface
      variant="popout"
      onRefresh={refresh}
      isRefreshing={isRefreshing}
      actions={headerActions}
      banner={banner}
      footerRows={footerRows}
      summary={
        <MenuSummary
          total={sorted.length}
          errorCount={errorCount}
          isRefreshing={isRefreshing}
          lastRefresh={lastRefresh}
        />
      }
    >
      <div className="menu-stack">
        {sorted.map((p, idx) => (
          <div
            key={p.providerId}
            className="menu-stack__item"
            data-deeplinked={p.providerId === providerId || undefined}
          >
            {idx > 0 && <div className="menu-stack__sep" />}
            <MenuCard
              provider={p}
              hideEmail={settings.hidePersonalInfo}
              resetTimeRelative={settings.resetTimeRelative}
            />
          </div>
        ))}
      </div>
    </MenuSurface>
  );
}
