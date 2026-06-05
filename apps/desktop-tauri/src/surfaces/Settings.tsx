import { useCallback, useEffect, useState, type ReactElement, type ReactNode } from "react";
import { getCurrentWindow, LogicalPosition, LogicalSize } from "@tauri-apps/api/window";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type {
  BootstrapState,
  SettingsTabId,
  SettingsUpdate,
} from "../types/bridge";
import { useSettings } from "../hooks/useSettings";
import { useSurfaceTarget } from "../hooks/useSurfaceMode";
import { useLocale } from "../hooks/useLocale";
import type { LocaleKey } from "../i18n/keys";
import { closeSettingsWindow, getWorkAreaRect, setSurfaceMode } from "../lib/tauri";
import GeneralTab from "./settings/tabs/GeneralTab";
import DisplayTab from "./settings/tabs/DisplayTab";
import AdvancedTab from "./settings/tabs/AdvancedTab";
import AboutTab from "./settings/tabs/AboutTab";
import ProvidersTab from "./settings/tabs/ProvidersTab";

// ── tab types ────────────────────────────────────────────────────────

type SettingsTab = SettingsTabId;

// Inline monochrome SVG icons stand in for the upstream macOS SF Symbols
// (gearshape / square.grid.2x2 / eye / slider.horizontal.3 / info.circle).
// They render in `currentColor` so they pick up the same secondary/accent
// text color as the tab label.
const ICON_SIZE = 16;

function Svg({ children }: { children: ReactNode }) {
  return (
    <svg
      width={ICON_SIZE}
      height={ICON_SIZE}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.4}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden
    >
      {children}
    </svg>
  );
}

const TabIcons: Record<SettingsTab, ReactElement> = {
  general: (
    <Svg>
      <circle cx="8" cy="8" r="2" />
      <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" />
    </Svg>
  ),
  providers: (
    <Svg>
      <rect x="2" y="2" width="5" height="5" rx="1" />
      <rect x="9" y="2" width="5" height="5" rx="1" />
      <rect x="2" y="9" width="5" height="5" rx="1" />
      <rect x="9" y="9" width="5" height="5" rx="1" />
    </Svg>
  ),
  display: (
    <Svg>
      <path d="M1.5 8c1.6-3 4-4.5 6.5-4.5S13 5 14.5 8c-1.5 3-4 4.5-6.5 4.5S3.1 11 1.5 8Z" />
      <circle cx="8" cy="8" r="2" />
    </Svg>
  ),
  advanced: (
    <Svg>
      <path d="M2 4h8M2 8h5M2 12h10" />
      <circle cx="11.5" cy="4" r="1.4" />
      <circle cx="8.5" cy="8" r="1.4" />
      <circle cx="13" cy="12" r="1.4" />
    </Svg>
  ),
  about: (
    <Svg>
      <circle cx="8" cy="8" r="6.25" />
      <path d="M8 7v4" />
      <circle cx="8" cy="5" r="0.6" fill="currentColor" stroke="none" />
    </Svg>
  ),
};

// Tab order mirrors upstream PreferencesView (General, Providers, Display,
// Advanced, About). Per-provider credential management (API keys, cookies,
// token accounts) is handled inside the Providers tab.
const TAB_META: { id: SettingsTab; labelKey: LocaleKey }[] = [
  { id: "general", labelKey: "TabGeneral" },
  { id: "providers", labelKey: "TabProviders" },
  { id: "display", labelKey: "TabDisplay" },
  { id: "advanced", labelKey: "TabAdvanced" },
  { id: "about", labelKey: "TabAbout" },
];

function isSettingsTab(value: string): value is SettingsTab {
  return TAB_META.some((t) => t.id === value);
}

const SETTINGS_WINDOW_HEIGHT = 580;
const SETTINGS_WINDOW_DEFAULT_WIDTH = 496;
const SETTINGS_WINDOW_PROVIDERS_WIDTH = 600;

async function applySettingsWindowSize(tab: SettingsTab) {
  const requestedWidth =
    tab === "providers"
      ? SETTINGS_WINDOW_PROVIDERS_WIDTH
      : SETTINGS_WINDOW_DEFAULT_WIDTH;
  const workArea = await getWorkAreaRect().catch(() => null);
  const screenWidth = window.screen.availWidth || window.innerWidth || requestedWidth;
  const screenHeight = window.screen.availHeight || window.innerHeight || SETTINGS_WINDOW_HEIGHT;
  const maxWidth = Math.min(workArea?.width ?? screenWidth, screenWidth);
  const maxHeight = Math.min(workArea?.height ?? screenHeight, screenHeight);
  const width = Math.max(
    360,
    Math.min(requestedWidth, maxWidth - 16),
  );
  const height = Math.max(
    360,
    Math.min(SETTINGS_WINDOW_HEIGHT, maxHeight - 16),
  );
  const win = getCurrentWindow();
  await win.setSize(new LogicalSize(width, height)).catch(() => {});
  const screenOrigin = window.screen as Screen & {
    availLeft?: number;
    availTop?: number;
  };
  const left = screenOrigin.availLeft ?? workArea?.x ?? 0;
  const top = screenOrigin.availTop ?? workArea?.y ?? 0;
  await win
    .setPosition(
      new LogicalPosition(
        left + Math.max(8, Math.round((screenWidth - width) / 2)),
        top + Math.max(8, Math.round((screenHeight - height) / 2)),
      ),
    )
    .catch(() => {});
}

export default function Settings({ state, initialTab: propTab }: { state: BootstrapState; initialTab?: string }) {
  const { settings, saving, error, update } = useSettings(state.settings);
  const { t } = useLocale();
  const shellTarget = useSurfaceTarget("settings");
  const initialTab: SettingsTab =
    propTab && isSettingsTab(propTab)
      ? propTab
      : shellTarget?.kind === "settings" && isSettingsTab(shellTarget.tab)
        ? shellTarget.tab
        : "general";
  const [activeTab, setActiveTab] = useState<SettingsTab>(initialTab);

  useEffect(() => {
    void applySettingsWindowSize(initialTab);
    // initialTab is captured once on mount; subsequent tab changes are
    // handled by handleTabClick / shellTarget effect below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Respond to prop-driven tab changes (detached window re-focus events).
  useEffect(() => {
    if (propTab && isSettingsTab(propTab)) {
      setActiveTab((current) => {
        if (current === propTab) return current;
        void applySettingsWindowSize(propTab);
        return propTab;
      });
    }
  }, [propTab]);

  useEffect(() => {
    if (shellTarget?.kind !== "settings" || !isSettingsTab(shellTarget.tab)) {
      return;
    }

    const nextTab: SettingsTab = shellTarget.tab;
    setActiveTab((current) => {
      if (current === nextTab) return current;
      void applySettingsWindowSize(nextTab);
      return nextTab;
    });
  }, [shellTarget]);

  const set = (patch: SettingsUpdate) => void update(patch);
  const handleTabClick = useCallback((tab: SettingsTab) => {
    setActiveTab(tab);
    void applySettingsWindowSize(tab);
    // Only transition the main window if we're NOT in the detached settings window
    if (getCurrentWebviewWindow().label !== "settings") {
      void setSurfaceMode("settings", { kind: "settings", tab });
    }
  }, []);

  return (
    <div
      className={`settings${activeTab === "providers" ? " settings--providers-active" : ""}`}
    >
      {/* custom title bar (decorations disabled for guaranteed dark theme) */}
      <div className="settings-titlebar" data-tauri-drag-region>
        <span className="settings-titlebar__title" data-tauri-drag-region>CodexBar Settings</span>
        <div className="settings-titlebar__controls">
          <button
            className="settings-titlebar__control settings-titlebar__control--minimize"
            onClick={() => void getCurrentWindow().minimize()}
            aria-label="Minimize"
            title="Minimize"
          />
          <button
            className="settings-titlebar__control settings-titlebar__control--close"
            onClick={() => void closeSettingsWindow()}
            aria-label="Close"
            title="Close"
          >
            <svg aria-hidden viewBox="0 0 16 16" focusable="false">
              <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" />
            </svg>
          </button>
        </div>
      </div>

      {/* tab bar */}
      <nav className="settings-tabs" role="tablist">
        {TAB_META.map((tab) => (
          <button
            key={tab.id}
            role="tab"
            aria-selected={activeTab === tab.id}
            className={`settings-tab ${activeTab === tab.id ? "settings-tab--active" : ""}`}
            onClick={() => handleTabClick(tab.id)}
          >
            <span className="settings-tab__icon">{TabIcons[tab.id]}</span>
            <span className="settings-tab__label">{t(tab.labelKey)}</span>
          </button>
        ))}
      </nav>

      {/* status bar */}
      {(saving || error) && (
        <div
          className={`settings-status ${error ? "settings-status--error" : ""}`}
        >
          {saving ? t("SettingsStatusSaving") : error}
        </div>
      )}

      {/* tab panels */}
      <div className={`settings-body${activeTab === "providers" ? " settings-body--providers" : ""}`}>
        {activeTab === "general" && (
          <GeneralTab settings={settings} set={set} saving={saving} />
        )}
        {activeTab === "providers" && (
          <ProvidersTab
            settings={settings}
            providers={state.providers}
            set={set}
            saving={saving}
          />
        )}
        {activeTab === "display" && (
          <DisplayTab settings={settings} set={set} saving={saving} />
        )}
        {activeTab === "advanced" && (
          <AdvancedTab settings={settings} set={set} saving={saving} />
        )}
        {activeTab === "about" && (
          <AboutTab settings={settings} set={set} saving={saving} />
        )}
      </div>
    </div>
  );
}

// ── Tab props shared with extracted tab components ──────────────────

export interface TabProps {
  settings: BootstrapState["settings"];
  set: (p: SettingsUpdate) => void;
  saving: boolean;
}
