import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow, LogicalPosition, LogicalSize } from "@tauri-apps/api/window";
import type { BootstrapState, ProviderUsageSnapshot } from "../types/bridge";
import { setSurfaceMode, openSettingsWindow, quitApp as quitApplication } from "../lib/tauri";
import { useProviders } from "../hooks/useProviders";
import { useSettings } from "../hooks/useSettings";
import { useUpdateState } from "../hooks/useUpdateState";
import { useLocale } from "../hooks/useLocale";
import MenuCard from "../components/MenuCard";
import MenuSurface, {
  MenuEmpty,
  type MenuFooterRow,
} from "../components/MenuSurface";
import UpdateBanner from "../components/UpdateBanner";
import ProviderGrid, { prioritizeProviders } from "../components/ProviderGrid";
import { DEMO_ENABLED, DEMO_PROVIDERS } from "../lib/demoProviders";
import { orderProviderSnapshots } from "../lib/providerOrder";

/**
 * Pop-out window — dashboard and provider deep-links both keep the full card
 * stack. A provider target only scrolls/focuses the requested card so the
 * layout stays consistent with the tray/menu surface.
 */
export default function PopOutPanel({
  state,
  providerId,
}: {
  state: BootstrapState;
  providerId?: string;
}) {
  const {
    providers: realProviders,
    isRefreshing,
    refresh,
    hasCachedData,
  } = useProviders();
  const providers = DEMO_ENABLED ? DEMO_PROVIDERS : realProviders;
  const { settings } = useSettings(state.settings);
  const { updateState, checkNow, download, apply, dismiss, openRelease } =
    useUpdateState();
  const { t } = useLocale();

  const sorted = useMemo(() => {
    return orderProviderSnapshots(
      providers,
      state.providers,
      settings.enabledProviders,
      settings.providerOrder,
    );
  }, [providers, settings.enabledProviders, settings.providerOrder, state.providers]);
  const [selectedProviderId, setSelectedProviderId] = useState<string | null>(
    providerId ?? null,
  );
  const [gridExpanded, setGridExpanded] = useState(false);
  const cardRefs = useRef(new Map<string, HTMLDivElement>());

  useEffect(() => {
    setSelectedProviderId(providerId ?? null);
  }, [providerId]);

  const visibleProviders = useMemo(
    () => {
      if (selectedProviderId === null) {
        if (sorted.length + 1 > 32 && !gridExpanded) {
          return prioritizeProviders(sorted, null).slice(0, 4);
        }
        return sorted;
      }
      const match = sorted.find((p) => p.providerId === selectedProviderId);
      return match ? [match] : sorted;
    },
    [sorted, selectedProviderId, gridExpanded],
  );
  const providerOrderKey = useMemo(
    () => sorted.map((provider) => provider.providerId).join(","),
    [sorted],
  );

  const handleGridClick = useCallback((nextProviderId: string | null) => {
    setSelectedProviderId(nextProviderId);
  }, []);

  useEffect(() => {
    const win = getCurrentWindow();
    const screenWidth = window.screen.availWidth || window.innerWidth || 420;
    const screenHeight = window.screen.availHeight || window.innerHeight || 680;
    const width = Math.max(320, Math.min(420, screenWidth - 16));
    // Leave room for native borders/title bars on Windows; the body scrolls.
    const height = Math.max(320, Math.min(680, screenHeight - 88));
    const screenOrigin = window.screen as Screen & {
      availLeft?: number;
      availTop?: number;
    };
    const left = screenOrigin.availLeft ?? 0;
    const top = screenOrigin.availTop ?? 0;

    void win.setSize(new LogicalSize(width, height)).then(() =>
      win.setPosition(
        new LogicalPosition(
          left + Math.max(8, screenWidth - width - 8),
          top + 8,
        ),
      ),
    ).catch(() => {});
  }, []);

  useEffect(() => {
    if (!providerId || selectedProviderId !== providerId || providerOrderKey.length === 0) return;

    let cancelled = false;
    const scrollToProvider = () => {
      if (cancelled) return;
      const target = cardRefs.current.get(providerId);
      if (!target) return;

      window.scrollTo(0, 0);
      if (document.scrollingElement) {
        document.scrollingElement.scrollTop = 0;
      }
      document.documentElement.scrollTop = 0;
      document.body.scrollTop = 0;

      for (const selector of [".menu-stack", ".menu-surface__body"]) {
        const container = target.closest<HTMLElement>(selector);
        if (!container) continue;
        container.scrollTop = 0;
        const targetRect = target.getBoundingClientRect();
        const containerRect = container.getBoundingClientRect();
        container.scrollTop += targetRect.top - containerRect.top;
      }
    };

    requestAnimationFrame(() => {
      requestAnimationFrame(scrollToProvider);
    });
    const timer = window.setTimeout(scrollToProvider, 100);
    const lateTimer = window.setTimeout(scrollToProvider, 350);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      window.clearTimeout(lateTimer);
    };
  }, [providerId, selectedProviderId, providerOrderKey]);

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
    >
      <ProviderGrid
        providers={sorted}
        selectedProviderId={selectedProviderId}
        showAsUsed={settings.showAsUsed}
        showProviderIcons={settings.switcherShowsIcons}
        expanded={gridExpanded}
        onExpandedChange={setGridExpanded}
        onSelect={handleGridClick}
      />
      <div className="provider-grid__divider" />
      <div className="menu-stack">
        {visibleProviders.map((p, idx) => (
          <Fragment key={p.providerId}>
            {idx > 0 && <div className="menu-stack__sep" />}
            <div
              className={`menu-stack__item${selectedProviderId === p.providerId ? " menu-stack__item--selected" : ""}`}
              ref={(node) => {
                if (node) {
                  cardRefs.current.set(p.providerId, node);
                } else {
                  cardRefs.current.delete(p.providerId);
                }
              }}
            >
              <MenuCard
                provider={p}
                hideEmail={settings.hidePersonalInfo}
                resetTimeRelative={settings.resetTimeRelative}
                showAsUsed={settings.showAsUsed}
                compactMetrics={selectedProviderId === null}
              />
            </div>
          </Fragment>
        ))}
      </div>
    </MenuSurface>
  );
}
