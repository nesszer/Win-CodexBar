import { render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// App.tsx routes by `getCurrentWebviewWindow().label` before falling through
// to the shared-window SurfaceRouter (which reads the surface-mode snapshot
// instead). These tests focus on that routing decision — which top-level
// surface App mounts for a given window label — not on any individual
// surface's internal behavior (those have their own dedicated test files).

const webviewWindowMocks = vi.hoisted(() => ({
  label: "main",
}));

const surfaceMocks = vi.hoisted(() => ({
  snapshot: {
    mode: "hidden",
    target: { kind: "summary" },
  } as { mode: string; target: Record<string, unknown> },
}));

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({ label: webviewWindowMocks.label }),
}));

const tauriMocks = vi.hoisted(() => ({
  getBootstrapState: vi.fn(),
  getSettingsSnapshot: vi.fn(),
  checkForUpdates: vi.fn(),
  downloadUpdate: vi.fn(),
  setSurfaceMode: vi.fn(),
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
  getCurrentSurfaceState: vi.fn(),
  getDeepSeekPricingStatus: vi.fn().mockResolvedValue(null),
}));

vi.mock("./lib/tauri", () => tauriMocks);

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => eventMocks);

// Stand-in surfaces: assert routing, not each surface's own rendering.
vi.mock("./surfaces/TrayPanel", () => ({
  default: () => <div data-testid="surface-tray-panel" />,
}));
vi.mock("./surfaces/Settings", () => ({
  default: () => <div data-testid="surface-settings" />,
}));
vi.mock("./floatbar/FloatBar", () => ({
  default: () => <div data-testid="surface-float-bar" />,
}));

vi.mock("./hooks/useSurfaceSnapshot", () => ({
  useSurfaceSnapshot: () => surfaceMocks.snapshot,
}));

import App from "./App";
import { buildBundle } from "./test/localeHarness";
import type { BootstrapState } from "./types/bridge";
import { makeSettings } from "./test/fixtures";


function bootstrap(): BootstrapState {
  return {
    contractVersion: "v1",
    providers: [],
    settings: makeSettings(),
  };
}

describe("App window-label routing", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    webviewWindowMocks.label = "main";
    surfaceMocks.snapshot = { mode: "hidden", target: { kind: "summary" } };
    tauriMocks.getBootstrapState.mockResolvedValue(bootstrap());
    tauriMocks.getSettingsSnapshot.mockResolvedValue(makeSettings());
    tauriMocks.checkForUpdates.mockResolvedValue({
      status: "idle",
      version: null,
      error: null,
      progress: null,
      releaseUrl: null,
      canDownload: false,
      canApply: false,
      lastCheckedAt: null,
    });
    tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle());
    tauriMocks.getCurrentSurfaceState.mockResolvedValue({
      mode: "hidden",
      target: { kind: "summary" },
    });
    eventMocks.listen.mockResolvedValue(() => {});
  });

  it("routes the dedicated flyout window to TrayPanel", async () => {
    webviewWindowMocks.label = "flyout";

    const { queryByTestId } = render(<App />);

    await waitFor(() => {
      expect(queryByTestId("surface-tray-panel")).not.toBeNull();
    });
    expect(queryByTestId("surface-settings")).toBeNull();
    expect(queryByTestId("surface-float-bar")).toBeNull();
  });

  it("routes the detached settings window to Settings, not TrayPanel", async () => {
    webviewWindowMocks.label = "settings";

    const { queryByTestId } = render(<App />);

    await waitFor(() => {
      expect(queryByTestId("surface-settings")).not.toBeNull();
    });
    expect(queryByTestId("surface-tray-panel")).toBeNull();
  });

  it("routes the detached floatbar window to FloatBar, not TrayPanel", async () => {
    webviewWindowMocks.label = "floatbar";

    const { queryByTestId } = render(<App />);

    await waitFor(() => {
      expect(queryByTestId("surface-float-bar")).not.toBeNull();
    });
    expect(queryByTestId("surface-tray-panel")).toBeNull();
  });

  it("does not route the shared main window to TrayPanel while hidden", async () => {
    // main's surface-mode machine only holds Hidden/Settings outside proof
    // mode — it never reports "trayPanel" — so the
    // isFlyoutWindow()/isSettingsWindow()/isFloatBarWindow() checks all miss
    // and control falls through to SurfaceRouter, which renders nothing for
    // "hidden".
    webviewWindowMocks.label = "main";

    const { container, queryByTestId } = render(<App />);

    // Wait for BOTH the bootstrap state AND the locale bundle to resolve —
    // AppInner only reaches the isFlyoutWindow()/SurfaceRouter branch after
    // `state` is set, and LocaleProvider only renders children after its own
    // bundle loads. Waiting for both pushes past every loading-placeholder
    // return path, so a null firstChild here reflects the settled "hidden"
    // SurfaceRouter branch, not an earlier loading state.
    await waitFor(() => {
      expect(tauriMocks.getBootstrapState).toHaveBeenCalled();
      expect(tauriMocks.getLocaleStrings).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(container.querySelector("main.shell")).toBeNull();
    });
    expect(queryByTestId("surface-tray-panel")).toBeNull();
    expect(container.firstChild).toBeNull();
  });

  it("renders the tray panel, never a legacy layout, for a stale popOut mode", async () => {
    // The legacy PopOut layout is retired: the backend rejects popOut, and
    // the router falls back to the tray panel if a stale snapshot reports it.
    webviewWindowMocks.label = "main";
    surfaceMocks.snapshot = { mode: "popOut", target: { kind: "dashboard" } };

    const { queryByTestId } = render(<App />);

    await waitFor(() => {
      expect(queryByTestId("surface-tray-panel")).not.toBeNull();
    });
    expect(queryByTestId("surface-settings")).toBeNull();
  });

  it("does not open a second window from the global-shortcut event", async () => {
    // shortcut_bridge::plugin's native handler toggles the tray-panel flyout
    // for every registered shortcut; the frontend no longer listens.
    webviewWindowMocks.label = "main";

    render(<App />);

    await waitFor(() => {
      expect(tauriMocks.getBootstrapState).toHaveBeenCalled();
    });
    const events = eventMocks.listen.mock.calls.map(([event]) => event);
    expect(events).not.toContain("global-shortcut-triggered");
    expect(tauriMocks.setSurfaceMode).not.toHaveBeenCalled();
  });
});
