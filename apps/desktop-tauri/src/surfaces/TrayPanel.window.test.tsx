import { act, fireEvent, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { TEST_PROVIDER_CATALOG } from "../test/providerCatalog";
import { tauriMocks, windowMocks } from "../test/trayPanelMocks";
import { provider, renderTrayPanel, emitEvent, setupTrayPanelTests } from "../test/trayPanelHarness";

vi.mock("../lib/tauri", async () => (await import("../test/trayPanelMocks")).tauriMocks);
vi.mock("@tauri-apps/api/event", async () => (await import("../test/trayPanelMocks")).eventMocks);
vi.mock("@tauri-apps/api/window", async () => (await import("../test/trayPanelMocks")).windowMocks);

describe("TrayPanel provider grid", () => {
  setupTrayPanelTests();

  it("renders the default tray panel layout with no legacy window chrome", async () => {
    // Pins the one dashboard layout: tray-variant surface, icon-first
    // provider switcher, and the Zoom / Refresh / Settings... / About / Quit
    // footer. The retired PopOut layout had a "CodexBar" title bar with
    // window controls and a Settings / About / Quit footer without Zoom or
    // Refresh.
    const { container } = renderTrayPanel([
      provider("claude", "Claude", 35),
      provider("codex", "Codex", 20),
    ]);

    await waitFor(() => {
      expect(container.querySelector(".menu-surface__footer-zoom")).not.toBeNull();
      expect(container.querySelector(".provider-grid")).not.toBeNull();
    });

    const surface = container.querySelector(".menu-surface");
    expect(surface?.classList.contains("menu-surface--tray")).toBe(true);
    expect(container.querySelector(".menu-surface--popout")).toBeNull();
    expect(container.querySelector(".popout-titlebar")).toBeNull();
    expect(container.querySelector(".popout-scale-shell")).toBeNull();
    expect(container.querySelector(".provider-grid")).not.toBeNull();

    const footerLabels = Array.from(
      container.querySelectorAll(".menu-surface__footer > *"),
    ).map((el) => el.textContent ?? "");
    expect(footerLabels[0]).toContain("Zoom");
    expect(footerLabels.slice(1).map((label) => label.replace(/Ctrl\+.*/, ""))).toEqual([
      "↻Refresh",
      "⚙Settings...",
      "ⓘAbout CodexBar",
      "⌧Quit",
    ]);
    expect(tauriMocks.setSurfaceMode).not.toHaveBeenCalled();
  });

  it("offers a move strip and resize grips on every edge and corner", async () => {
    const { container, unmount } = renderTrayPanel([provider("cursor", "Cursor", 20)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-move-handle")).not.toBeNull();
    });

    const edges = Array.from(container.querySelectorAll(".tray-resize")).map((grip) =>
      Array.from(grip.classList).find((name) => name.startsWith("tray-resize--")),
    );
    expect(edges.sort()).toEqual([
      "tray-resize--bottom",
      "tray-resize--bottomleft",
      "tray-resize--bottomright",
      "tray-resize--left",
      "tray-resize--right",
      "tray-resize--top",
      "tray-resize--topleft",
      "tray-resize--topright",
    ]);
    // Unmount while the mocks still resolve; the shared afterEach resets
    // them before the automatic cleanup runs.
    unmount();
  });

  it("starts a native resize toward the grabbed corner without arming the blur guard", async () => {
    const { container, unmount } = renderTrayPanel([provider("cursor", "Cursor", 20)]);
    await waitFor(() => {
      expect(container.querySelector(".tray-resize--bottomright")).not.toBeNull();
    });

    fireEvent.mouseDown(container.querySelector(".tray-resize--bottomright")!);

    await waitFor(() => {
      expect(windowMocks.startResizeDragging).toHaveBeenCalledWith("SouthEast");
    });
    // The backend keeps the panel open while the button is held on it; a
    // guard would also swallow the next real outside click.
    expect(tauriMocks.beginFlyoutGesture).not.toHaveBeenCalled();
    // Unmount while the mocks still resolve; the shared afterEach resets
    // them before the automatic cleanup runs.
    unmount();
  });

  it("moves the flyout from the strip and puts it back by the tray on double-click", async () => {
    const { container, unmount } = renderTrayPanel([provider("cursor", "Cursor", 20)]);
    await waitFor(() => {
      expect(container.querySelector(".tray-move-handle")).not.toBeNull();
    });
    const strip = container.querySelector(".tray-move-handle")!;

    fireEvent.mouseDown(strip, { button: 0, detail: 1 });
    await waitFor(() => {
      expect(windowMocks.startDragging).toHaveBeenCalledTimes(1);
    });
    expect(tauriMocks.beginFlyoutGesture).not.toHaveBeenCalled();

    fireEvent.mouseDown(strip, { button: 0, detail: 2 });
    await waitFor(() => {
      expect(tauriMocks.resetFlyoutPosition).toHaveBeenCalledTimes(1);
    });
    expect(windowMocks.startDragging).toHaveBeenCalledTimes(1);
    // Unmount while the mocks still resolve; the shared afterEach resets
    // them before the automatic cleanup runs.
    unmount();
  });

  function captureWindowEvents(scale: number) {
    const handlers: {
      resized?: (event: { payload: { width: number; height: number } }) => void;
      scaleChanged?: (event: {
        payload: { scaleFactor: number; size: { width: number; height: number } };
      }) => void;
    } = {};
    windowMocks.getCurrentWindow.mockReturnValue({
      startDragging: windowMocks.startDragging,
      startResizeDragging: windowMocks.startResizeDragging,
      setSize: vi.fn().mockResolvedValue(undefined),
      close: vi.fn().mockResolvedValue(undefined),
      scaleFactor: vi.fn().mockResolvedValue(scale),
      innerSize: vi.fn().mockResolvedValue({ width: 656, height: 400 }),
      onResized: vi.fn((handler) => {
        handlers.resized = handler;
        return Promise.resolve(() => {});
      }),
      onScaleChanged: vi.fn((handler) => {
        handlers.scaleChanged = handler;
        return Promise.resolve(() => {});
      }),
    });
    return handlers;
  }

  it("remembers a user resize in logical px and re-anchors at the new size", async () => {
    const handlers = captureWindowEvents(2);
    const { unmount } = renderTrayPanel([provider("cursor", "Cursor", 20)]);
    await waitFor(() => expect(handlers.resized).toBeDefined());
    // Let the first auto-fit pass and its trailing guard settle.
    await new Promise((resolve) => setTimeout(resolve, 500));
    tauriMocks.reanchorTrayPanel.mockClear();

    act(() => {
      handlers.resized!({ payload: { width: 900, height: 1500 } });
    });

    await waitFor(() => {
      expect(tauriMocks.setFlyoutSize).toHaveBeenCalledWith(450, 750);
    });
    expect(tauriMocks.reanchorTrayPanel).toHaveBeenCalled();
    // Unmount while the mocks still resolve; the shared afterEach resets
    // them before the automatic cleanup runs.
    unmount();
  });

  it("does not treat a DPI rescale as a user resize", async () => {
    const handlers = captureWindowEvents(2);
    const { unmount } = renderTrayPanel([provider("cursor", "Cursor", 20)]);
    await waitFor(() => expect(handlers.scaleChanged).toBeDefined());
    // Let the first auto-fit pass and its trailing guard settle.
    await new Promise((resolve) => setTimeout(resolve, 500));

    act(() => {
      handlers.scaleChanged!({
        payload: { scaleFactor: 2.25, size: { width: 738, height: 1755 } },
      });
      handlers.resized!({ payload: { width: 738, height: 1755 } });
    });
    await new Promise((resolve) => setTimeout(resolve, 400));

    expect(tauriMocks.setFlyoutSize).not.toHaveBeenCalled();
    // Unmount while the mocks still resolve; the shared afterEach resets
    // them before the automatic cleanup runs.
    unmount();
  });

  it("ignores non-primary presses on the move strip", async () => {
    const { container, unmount } = renderTrayPanel([provider("cursor", "Cursor", 20)]);
    await waitFor(() => {
      expect(container.querySelector(".tray-move-handle")).not.toBeNull();
    });

    fireEvent.mouseDown(container.querySelector(".tray-move-handle")!, { button: 2, detail: 1 });

    expect(windowMocks.startDragging).not.toHaveBeenCalled();
    expect(tauriMocks.resetFlyoutPosition).not.toHaveBeenCalled();
    // Unmount while the mocks still resolve; the shared afterEach resets
    // them before the automatic cleanup runs.
    unmount();
  });

  it("renders the tray footer zoom slider above Refresh and persists trayScalePercent after the debounce", async () => {
    const { container } = renderTrayPanel(
      [provider("claude", "Claude", 35)],
      { trayScalePercent: 120 },
    );

    await waitFor(() => {
      expect(container.querySelector(".menu-surface__footer-zoom")).not.toBeNull();
    });

    const footerChildren = Array.from(
      container.querySelectorAll(".menu-surface__footer > *"),
    );
    const zoomIndex = footerChildren.findIndex((el) =>
      el.classList.contains("menu-surface__footer-zoom"),
    );
    const refreshIndex = footerChildren.findIndex(
      (el) => el.textContent?.includes("Refresh"),
    );
    expect(zoomIndex).toBeGreaterThanOrEqual(0);
    expect(refreshIndex).toBeGreaterThan(zoomIndex);

    // Slider reflects the persisted settings value.
    const slider = container.querySelector<HTMLInputElement>(
      ".menu-surface__footer-zoom-slider",
    )!;
    expect(slider).not.toBeNull();
    expect(slider.value).toBe("120");
    expect(slider.min).toBe("100");
    expect(slider.max).toBe("200");
    expect(slider.step).toBe("5");
    expect(
      container.querySelector(".menu-surface__footer-zoom-value")?.textContent,
    ).toBe("120%");

    fireEvent.change(slider, { target: { value: "150" } });

    // Live preview: thumb and readout update immediately from local state…
    expect(slider.value).toBe("150");
    expect(
      container.querySelector(".menu-surface__footer-zoom-value")?.textContent,
    ).toBe("150%");

    // …while persistence trails the ~250ms debounce (not synchronous).
    expect(tauriMocks.updateSettings).not.toHaveBeenCalled();
    await waitFor(() => {
      expect(tauriMocks.updateSettings).toHaveBeenCalledWith({
        trayScalePercent: 150,
      });
    });
    expect(tauriMocks.updateSettings).toHaveBeenCalledTimes(1);
  });

  it("reveals the tray panel if the native resize pass fails", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    windowMocks.getCurrentWindow.mockReturnValue({
      setSize: vi.fn().mockRejectedValue(new Error("resize failed")),
      close: vi.fn().mockResolvedValue(undefined),
      scaleFactor: vi.fn().mockResolvedValue(1),
      onResized: vi.fn().mockResolvedValue(() => {}),
      innerSize: vi.fn().mockResolvedValue({ width: 328, height: 200 }),
    });

    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });

    warn.mockRestore();
  });

  it("does not resize the native tray window for usage-only provider updates", async () => {
    const setSize = vi.fn().mockResolvedValue(undefined);
    windowMocks.getCurrentWindow.mockReturnValue({
      setSize,
      close: vi.fn().mockResolvedValue(undefined),
      scaleFactor: vi.fn().mockResolvedValue(1),
      onResized: vi.fn().mockResolvedValue(() => {}),
      innerSize: vi.fn().mockResolvedValue({ width: 328, height: 200 }),
    });

    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
    setSize.mockClear();
    tauriMocks.reanchorTrayPanel.mockClear();

    act(() => {
      emitEvent("provider-updated", provider("claude", "Claude", 52));
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 200));
    });

    expect(setSize).not.toHaveBeenCalled();
    expect(tauriMocks.reanchorTrayPanel).not.toHaveBeenCalled();
  });

  it("reserves dense all-provider height on first layout", async () => {
    const setSize = vi.fn().mockResolvedValue(undefined);
    windowMocks.getCurrentWindow.mockReturnValue({
      setSize,
      close: vi.fn().mockResolvedValue(undefined),
      scaleFactor: vi.fn().mockResolvedValue(1),
      onResized: vi.fn().mockResolvedValue(() => {}),
      innerSize: vi.fn().mockResolvedValue({ width: 328, height: 200 }),
    });
    const denseProviders = TEST_PROVIDER_CATALOG.slice(0, 36).map(([id, displayName]) =>
      provider(id, displayName),
    );

    renderTrayPanel(denseProviders, {
      enabledProviders: denseProviders.map((snapshot) => snapshot.providerId),
    });

    await waitFor(() => {
      expect(setSize).toHaveBeenCalledWith(
        expect.objectContaining({ width: 328, height: 776 }),
      );
    });
  });

  it("keeps provider detail mode tall enough for context actions and footer", async () => {
    const setSize = vi.fn().mockResolvedValue(undefined);
    windowMocks.getCurrentWindow.mockReturnValue({
      setSize,
      close: vi.fn().mockResolvedValue(undefined),
      scaleFactor: vi.fn().mockResolvedValue(1),
      onResized: vi.fn().mockResolvedValue(() => {}),
      innerSize: vi.fn().mockResolvedValue({ width: 328, height: 200 }),
    });
    const errorProvider = {
      ...provider("abacus", "Abacus AI", 0),
      error: "Source mode `Cli` not supported for this provider",
    };

    const { container } = renderTrayPanel([errorProvider]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
    setSize.mockClear();

    fireEvent.click(
      container.querySelector<HTMLButtonElement>(
        '.provider-grid__item[aria-label="Abacus AI"]',
      )!,
    );

    await waitFor(() => {
      expect(setSize).toHaveBeenCalledWith(
        expect.objectContaining({ width: 328, height: 420 }),
      );
    });
  });

  it("scales the auto-fit measure by the active tray zoom before clamping (#265)", async () => {
    const setSize = vi.fn().mockResolvedValue(undefined);
    windowMocks.getCurrentWindow.mockReturnValue({
      setSize,
      close: vi.fn().mockResolvedValue(undefined),
      scaleFactor: vi.fn().mockResolvedValue(1),
      onResized: vi.fn().mockResolvedValue(() => {}),
      innerSize: vi.fn().mockResolvedValue({ width: 328, height: 200 }),
    });
    // jsdom has no layout engine (scrollHeight always reads 0), so pin it
    // globally to a deterministic PRE-zoom content height: TrayPanel applies
    // `zoom: trayScale` via CSS and the hook must size the window in POST-zoom
    // px or tall cards clip below the fold (#265).
    const scrollHeight = vi
      .spyOn(Element.prototype, "scrollHeight", "get")
      .mockReturnValue(505);

    const first = renderTrayPanel([provider("codex", "Codex", 61)], {
      trayScalePercent: 150,
    });

    // 505 raw × 1.5 zoom = 757.5 → 758 rounded, + the 4px fudge = 762.
    await waitFor(() => {
      expect(setSize).toHaveBeenCalledWith(
        expect.objectContaining({ width: 328, height: 762 }),
      );
    });
    first.unmount();
    setSize.mockClear();

    // Same zoom, taller content: round(700 × 1.5) + 4 = 1054 exceeds the
    // mocked work-area cap (900 - 16 = 884), so the clamp still wins.
    scrollHeight.mockReturnValue(700);
    renderTrayPanel([provider("codex", "Codex", 61)], {
      trayScalePercent: 150,
    });

    await waitFor(() => {
      expect(setSize).toHaveBeenCalledWith(
        expect.objectContaining({ width: 328, height: 884 }),
      );
    });
  });
});
