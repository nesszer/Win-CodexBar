import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => key, language: "english" }),
}));
// The FloatBar section pulls in its own bridge dependencies; it is irrelevant
// to the display controls under test.
vi.mock("../../../floatbar/SettingsSection", () => ({
  default: () => null,
}));

import DisplayTab from "./DisplayTab";
import type { SettingsSnapshot } from "../../../types/bridge";

const baseSettings = {
  enabledProviders: ["codex", "claude"],
  trayIconMode: "single",
  stackedTrayTopProvider: null,
  stackedTrayBottomProvider: null,
  trayPanelAlwaysOnTop: false,
  switcherShowsIcons: false,
  menuBarShowsHighestUsage: false,
  menuBarShowsPercent: false,
  menuBarColorPace: false,
  menuBarDisplayMode: "detailed",
  overviewLayout: "detailed",
  windowScalePercent: 100,
  showAsUsed: false,
  showAllTokenAccountsInMenu: false,
  resetTimeRelative: false,
  showResetWhenExhausted: false,
  showPace: false,
} as unknown as SettingsSnapshot;

function renderTab(
  set: (patch: Record<string, unknown>) => void,
  mode: "menuBar" | "menu" = "menu",
) {
  return render(
    <DisplayTab
      mode={mode}
      settings={baseSettings}
      set={set as never}
      saving={false}
    />,
  );
}

describe("DisplayTab menu settings", () => {
  it("offers the tray Panel scale as its only slider, with no PopOut window scale", () => {
    const { container } = renderTab(vi.fn());

    expect(
      Array.from(container.querySelectorAll('input[type="range"]')).map((input) =>
        input.getAttribute("aria-label"),
      ),
    ).toEqual(["PanelScaleLabel"]);
  });

  it("shows the saved Panel scale and saves the new one when the slider is released", async () => {
    const set = vi.fn();
    // Settles the tab's tray-visibility read before the slider re-renders it.
    await act(async () => {
      render(
        <DisplayTab
          mode="menu"
          settings={{ ...baseSettings, trayScalePercent: 150 } as SettingsSnapshot}
          set={set}
          saving={false}
        />,
      );
    });

    const slider = screen.getByRole("slider", { name: "PanelScaleLabel" });
    expect(slider).toHaveValue("150");
    expect(screen.getByText("PanelScaleLabel (150%)")).toBeInTheDocument();

    fireEvent.change(slider, { target: { value: "125" } });
    expect(screen.getByText("PanelScaleLabel (125%)")).toBeInTheDocument();
    expect(set).not.toHaveBeenCalled();
    fireEvent.pointerUp(slider);

    expect(set).toHaveBeenCalledTimes(1);
    expect(set).toHaveBeenCalledWith({ trayScalePercent: 125 });
  });

  it("updates the exhausted reset display preference", () => {
    const set = vi.fn();
    renderTab(set);

    fireEvent.click(screen.getByRole("checkbox", { name: "ShowResetWhenExhausted" }));

    expect(set).toHaveBeenCalledWith({ showResetWhenExhausted: true });
  });

  it("updates the show pace preference", () => {
    const set = vi.fn();
    renderTab(set);

    fireEvent.click(screen.getByRole("checkbox", { name: "ShowPace" }));

    expect(set).toHaveBeenCalledWith({ showPace: true });
  });

  it("updates the Overview layout preference", () => {
    const set = vi.fn();
    renderTab(set);

    fireEvent.change(screen.getByRole("combobox"), {
      target: { value: "compact" },
    });

    expect(set).toHaveBeenCalledWith({ overviewLayout: "compact" });
  });

  it("updates the tray panel always-on-top preference", () => {
    const set = vi.fn();
    renderTab(set);

    fireEvent.click(
      screen.getByRole("checkbox", { name: "TrayPanelAlwaysOnTopLabel" }),
    );

    expect(set).toHaveBeenCalledWith({ trayPanelAlwaysOnTop: true });
  });

  it("updates the tray pace color preference", () => {
    const set = vi.fn();
    renderTab(set, "menuBar");

    fireEvent.click(screen.getByRole("checkbox", { name: "ColorPaceInTray" }));

    expect(set).toHaveBeenCalledWith({ menuBarColorPace: true });
  });
});

describe("DisplayTab stacked tray providers", () => {
  it("persists explicit top and bottom provider choices", () => {
    const set = vi.fn();
    render(
      <DisplayTab
        mode="menuBar"
        settings={{ ...baseSettings, trayIconMode: "stacked" } as SettingsSnapshot}
        providers={[
          { id: "codex", displayName: "Codex", cookieDomain: null },
          { id: "claude", displayName: "Claude", cookieDomain: null },
        ]}
        set={set}
        saving={false}
      />,
    );
    const selects = screen.getAllByRole("combobox");

    fireEvent.change(selects[1], { target: { value: "claude" } });
    fireEvent.change(selects[2], { target: { value: "codex" } });

    expect(set).toHaveBeenCalledWith({ stackedTrayTopProvider: "claude" });
    expect(set).toHaveBeenCalledWith({ stackedTrayBottomProvider: "codex" });
  });
});
