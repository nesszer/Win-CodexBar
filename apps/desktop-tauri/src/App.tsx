import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { checkForUpdates, getBootstrapState, getSettingsSnapshot, setSurfaceMode } from "./lib/tauri";
import { useSurfaceSnapshot } from "./hooks/useSurfaceSnapshot";
import { useTheme } from "./hooks/useTheme";
import Settings from "./surfaces/Settings";
import TrayPanel from "./surfaces/TrayPanel";
import PopOutPanel from "./surfaces/PopOutPanel";
import { LocaleProvider } from "./i18n/LocaleProvider";
import type { BootstrapState, ThemePreference } from "./types/bridge";
import type { SurfaceSnapshot } from "./hooks/useSurfaceSnapshot";

/** True when running inside the detached Settings window. */
function isSettingsWindow(): boolean {
  return getCurrentWebviewWindow().label === "settings";
}

/** Parse the initial Settings tab from the URL query string. */
function initialSettingsTab(): string {
  const params = new URLSearchParams(window.location.search);
  return params.get("tab") || "general";
}

export default function App() {
  return (
    <LocaleProvider>
      <AppInner />
    </LocaleProvider>
  );
}

function AppInner() {
  const surface = useSurfaceSnapshot();
  const [state, setState] = useState<BootstrapState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [themePreference, setThemePreference] = useState<ThemePreference>("dark");

  useTheme(themePreference);

  const reloadBootstrapState = useCallback(
    () => getBootstrapState(),
    [],
  );

  useEffect(() => {
    let cancelled = false;

    reloadBootstrapState()
      .then((bootstrap) => {
        if (cancelled) {
          return;
        }
        setState(bootstrap);
        setThemePreference(bootstrap.settings.theme);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      });

    // Fire-and-forget initial update check; results flow via events.
    checkForUpdates().catch(() => {});

    // Listen for user-registered global shortcut events from the
    // `register_global_shortcut` command. The persistent shortcut (bound via
    // shortcut_bridge::plugin) already toggles the tray panel natively, so
    // this listener is a no-op fallback for ad-hoc capture-mode registrations.
    const unlistenPromise = listen<string>("global-shortcut-triggered", () => {
      void setSurfaceMode("trayPanel", { kind: "summary" }).catch(() => {});
    });

    const unlistenSettingsChangePromise = isSettingsWindow()
      ? listen<string>("settings-change-tab", () => {
          void reloadBootstrapState()
            .then((bootstrap) => {
              setState(bootstrap);
              setThemePreference(bootstrap.settings.theme);
              setError(null);
            })
            .catch(() => {});
        })
      : Promise.resolve(null);

    // Keep the theme in sync when mutations happen inside other surfaces
    // (e.g., Settings → Appearance). `useSettings` dispatches this event
    // after every successful `updateSettings` call.
    const onSettingsUpdated = (evt: Event) => {
      const detail = (evt as CustomEvent<BootstrapState["settings"]>).detail;
      if (detail) {
        setThemePreference(detail.theme);
      } else {
        getSettingsSnapshot()
          .then((fresh) => setThemePreference(fresh.theme))
          .catch(() => {});
      }
    };
    window.addEventListener("codexbar:settings-updated", onSettingsUpdated);

    return () => {
      cancelled = true;
      void unlistenPromise.then((unlisten) => unlisten()).catch(() => {});
      void unlistenSettingsChangePromise
        .then((unlisten) => unlisten?.())
        .catch(() => {});
      window.removeEventListener("codexbar:settings-updated", onSettingsUpdated);
    };
  }, [reloadBootstrapState]);

  if (error) {
    return (
      <main className="shell">
        <section className="panel error">
          <h2>Bootstrap failed</h2>
          <p>{error}</p>
        </section>
      </main>
    );
  }

  if (!state) {
    return (
      <main className="shell">
        <section className="panel">
          <h2>Loading shell contract…</h2>
          <p>Waiting for the Rust bridge to describe providers, surfaces, and settings.</p>
        </section>
      </main>
    );
  }

  // Detached settings window — render Settings directly, skip SurfaceRouter.
  if (isSettingsWindow()) {
    return <DetachedSettingsApp state={state} />;
  }

  return <SurfaceRouter surface={surface} state={state} />;
}

function SurfaceRouter({
  surface,
  state,
}: {
  surface: SurfaceSnapshot;
  state: BootstrapState;
}) {
  switch (surface.mode) {
    case "hidden":
      return null;
    case "trayPanel":
      return <TrayPanel state={state} />;
    case "popOut": {
      const providerId =
        surface.target.kind === "provider"
          ? surface.target.providerId
          : undefined;
      return <PopOutPanel state={state} providerId={providerId} />;
    }
    case "settings":
      return <SettingsLayout state={state} />;
    default:
      return <TrayPanel state={state} />;
  }
}

function SettingsLayout({ state }: { state: BootstrapState }) {
  return (
    <main className="settings-surface settings-surface--full">
      <Settings state={state} />
    </main>
  );
}

function DetachedSettingsApp({ state }: { state: BootstrapState }) {
  const [tab, setTab] = useState(initialSettingsTab);

  useEffect(() => {
    // Listen for tab-change events from Rust (when the window is re-focused
    // with a different tab request).
    const unlisten = listen<string>("settings-change-tab", (event) => {
      setTab(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <main className="settings-surface settings-surface--full">
      <Settings state={state} initialTab={tab} />
    </main>
  );
}
