import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getTrayVisibilityStatus: vi.fn(),
}));
vi.mock("../../../lib/tauri", () => tauriMocks);
vi.mock("../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => key, language: "english" }),
}));

import MenuBarTab from "./MenuBarTab";
import type { ProviderCatalogEntry, SettingsSnapshot } from "../../../types/bridge";

const catalog: ProviderCatalogEntry[] = [
  "codex",
  "claude",
  "gemini",
  "cursor",
  "copilot",
  "zai",
  "kimi",
  "amp",
].map((id) => ({ id, displayName: id.toUpperCase(), cookieDomain: null }));

const baseSettings = {
  enabledProviders: ["codex", "claude", "gemini", "cursor", "copilot", "zai", "kimi"],
  providerOrder: [],
  trayIconMode: "single",
  stackedTrayTopProvider: null,
  stackedTrayBottomProvider: null,
  switcherShowsIcons: true,
  menuBarShowsHighestUsage: false,
  menuBarShowsPercent: false,
  menuBarIconStyle: "bars",
  menuBarColorPace: false,
  menuBarColorByProvider: false,
  promoteTrayIcon: false,
  mergedOverviewSelectedProviders: [],
} as unknown as SettingsSnapshot;

function renderTab(overrides: Partial<SettingsSnapshot> = {}, set = vi.fn()) {
  const settings = { ...baseSettings, ...overrides } as SettingsSnapshot;
  const view = render(<MenuBarTab settings={settings} set={set} saving={false} providers={catalog} />);
  return { ...view, set, settings };
}

const select = (name: string) => screen.getByRole("combobox", { name }) as HTMLSelectElement;
const fieldLabels = (region: HTMLElement) =>
  Array.from(region.querySelectorAll(".settings-field__label"), (n) => n.textContent);

beforeEach(() => {
  vi.clearAllMocks();
  tauriMocks.getTrayVisibilityStatus.mockResolvedValue({ support: "supported" });
});

describe("MenuBarTab layout", () => {
  it("has the Mac Icon and Combined icon sections with Pin to Taskbar last in Icon", async () => {
    renderTab();
    expect(screen.getAllByRole("heading", { level: 3 }).map((h) => h.textContent)).toEqual([
      "MenuBarIconSectionTitle",
      "CombinedIconSectionTitle",
    ]);
    expect(fieldLabels(screen.getByRole("region", { name: "MenuBarIconSectionTitle" }))).toEqual([
      "MenuBarIconStyleLabel",
      "ColorPaceInTray",
      "ColorByProviderInTray",
      "PaceSignLabel",
      "PromoteTrayIconLabel",
    ]);
    expect(fieldLabels(screen.getByRole("region", { name: "CombinedIconSectionTitle" }))).toEqual([
      "CombinedIconStyleLabel",
      "ShowEveryProviderInTrayMenuLabel",
      "SwitcherRowsLabel",
      "PreferHighestUsage",
      "OverviewProvidersLabel",
    ]);
    await waitFor(() =>
      expect((screen.getByRole("checkbox", { name: "PromoteTrayIconLabel" }) as HTMLInputElement).disabled).toBe(false),
    );
  });

  it("drops the Windows Display mode row", () => {
    renderTab();
    expect(screen.queryByText("DisplayModeLabel")).toBeNull();
  });

  it("keeps Pin to Taskbar disabled where Windows cannot promote the icon", async () => {
    tauriMocks.getTrayVisibilityStatus.mockResolvedValue({ support: "unsupported" });
    renderTab();
    expect(await screen.findByText("PromoteTrayIconUnsupportedHint")).toBeInTheDocument();
    expect((screen.getByRole("checkbox", { name: "PromoteTrayIconLabel" }) as HTMLInputElement).disabled).toBe(true);
  });
});

describe("MenuBarTab Icon section", () => {
  it("saves the Menu bar style", () => {
    const { set } = renderTab();
    expect(select("MenuBarIconStyleLabel").value).toBe("bars");
    fireEvent.change(select("MenuBarIconStyleLabel"), { target: { value: "percent" } });
    expect(set).toHaveBeenCalledWith({ menuBarIconStyle: "percent" });
  });

  it("offers Progress bars stacked after the other menu bar styles and saves it", () => {
    const { set } = renderTab();
    const options = Array.from(select("MenuBarIconStyleLabel").options).map((o) => [o.value, o.textContent]);
    expect(options).toEqual([
      ["bars", "MenuBarIconStyleBars"],
      ["percent", "MenuBarIconStylePercent"],
      ["stackedBars", "MenuBarIconStyleStackedBars"],
    ]);
    fireEvent.change(select("MenuBarIconStyleLabel"), { target: { value: "stackedBars" } });
    expect(set).toHaveBeenCalledWith({ menuBarIconStyle: "stackedBars" });
  });

  it("shows a saved Progress bars stacked style", () => {
    renderTab({ menuBarIconStyle: "stackedBars" });
    expect(select("MenuBarIconStyleLabel").value).toBe("stackedBars");
    expect(select("MenuBarIconStyleLabel").disabled).toBe(false);
  });

  it("disables the Menu bar style while the icon is stacked", () => {
    renderTab({ trayIconMode: "stacked" });
    expect(select("MenuBarIconStyleLabel").disabled).toBe(true);
  });

  it("updates the Color Pace Indicator", () => {
    const { set } = renderTab();
    fireEvent.click(screen.getByRole("checkbox", { name: "ColorPaceInTray" }));
    expect(set).toHaveBeenCalledWith({ menuBarColorPace: true });
  });

  it("updates Color by provider and keeps it independent of the pace toggle", () => {
    const { set } = renderTab();
    const colorByProvider = screen.getByRole("checkbox", { name: "ColorByProviderInTray" }) as HTMLInputElement;
    expect(colorByProvider.checked).toBe(false);
    fireEvent.click(colorByProvider);
    expect(set).toHaveBeenCalledWith({ menuBarColorByProvider: true });
    expect(set).not.toHaveBeenCalledWith(expect.objectContaining({ menuBarColorPace: expect.anything() }));
  });

  it("shows a saved Color by provider as checked and not disabled by stacked icons", () => {
    renderTab({ menuBarColorByProvider: true, trayIconMode: "stacked" });
    const colorByProvider = screen.getByRole("checkbox", { name: "ColorByProviderInTray" }) as HTMLInputElement;
    expect(colorByProvider.checked).toBe(true);
    expect(colorByProvider.disabled).toBe(false);
  });

  it("saves the Pace sign with ahead-of-pace positive as the default", () => {
    const { set } = renderTab();
    const paceSign = select("PaceSignLabel");
    expect(paceSign.value).toBe("ahead");
    expect(Array.from(paceSign.options, (o) => o.textContent)).toEqual([
      "PaceSignAheadPositive",
      "PaceSignReservePositive",
    ]);
    fireEvent.change(paceSign, { target: { value: "reserve" } });
    expect(set).toHaveBeenCalledWith({ menuBarPaceReservePositive: true });
  });

  it("shows a saved reserve-positive Pace sign and switches back", () => {
    const { set } = renderTab({ menuBarPaceReservePositive: true });
    expect(select("PaceSignLabel").value).toBe("reserve");
    fireEvent.change(select("PaceSignLabel"), { target: { value: "ahead" } });
    expect(set).toHaveBeenCalledWith({ menuBarPaceReservePositive: false });
  });
});

describe("MenuBarTab Combined icon section", () => {
  it("maps Switcher and Stacked onto the tray icon mode", () => {
    const { set } = renderTab();
    expect(select("CombinedIconStyleLabel").value).toBe("switcher");
    fireEvent.change(select("CombinedIconStyleLabel"), { target: { value: "stacked" } });
    expect(set).toHaveBeenCalledWith({ trayIconMode: "stacked" });
  });

  it("shows Per provider as Switcher with every provider in the tray menu", () => {
    const { set } = renderTab({ trayIconMode: "perProvider" });
    expect(select("CombinedIconStyleLabel").value).toBe("switcher");
    const every = screen.getByRole("checkbox", { name: "ShowEveryProviderInTrayMenuLabel" }) as HTMLInputElement;
    expect(every.checked).toBe(true);
    fireEvent.click(every);
    expect(set).toHaveBeenCalledWith({ trayIconMode: "single" });
  });

  it("turns on every provider in the tray menu", () => {
    const { set } = renderTab();
    fireEvent.click(screen.getByRole("checkbox", { name: "ShowEveryProviderInTrayMenuLabel" }));
    expect(set).toHaveBeenCalledWith({ trayIconMode: "perProvider" });
  });

  it("persists explicit top and bottom rows when stacked", () => {
    const { set } = renderTab({ trayIconMode: "stacked" });
    expect(screen.getByRole("checkbox", { name: "ShowEveryProviderInTrayMenuLabel" })).toBeDisabled();
    expect(screen.getByRole("checkbox", { name: "PreferHighestUsage" })).toBeDisabled();
    fireEvent.change(select("StackedTrayTopProvider"), { target: { value: "claude" } });
    fireEvent.change(select("StackedTrayBottomProvider"), { target: { value: "codex" } });
    expect(set).toHaveBeenCalledWith({ stackedTrayTopProvider: "claude" });
    expect(set).toHaveBeenCalledWith({ stackedTrayBottomProvider: "codex" });
  });

  it("maps Switcher rows to switcherShowsIcons", () => {
    const { set } = renderTab();
    expect(select("SwitcherRowsLabel").value).toBe("icons");
    fireEvent.change(select("SwitcherRowsLabel"), { target: { value: "weekly" } });
    expect(set).toHaveBeenCalledWith({ switcherShowsIcons: false });
  });

  it("updates Show most-used provider", () => {
    const { set } = renderTab();
    fireEvent.click(screen.getByRole("checkbox", { name: "PreferHighestUsage" }));
    expect(set).toHaveBeenCalledWith({ menuBarShowsHighestUsage: true });
  });
});

describe("MenuBarTab Overview providers", () => {
  it("summarizes the default as the first six enabled providers", () => {
    renderTab();
    expect(screen.getByText("OverviewProvidersDefaultSummary")).toBeInTheDocument();
  });

  it("summarizes a saved selection by name in provider order", () => {
    renderTab({ mergedOverviewSelectedProviders: ["gemini", "codex"] });
    expect(screen.getByText("CODEX, GEMINI")).toBeInTheDocument();
  });

  it("opens a sheet with the first six checked and the seventh blocked at the limit", () => {
    renderTab();
    fireEvent.click(screen.getByRole("button", { name: "OverviewProvidersConfigure" }));
    const sheet = screen.getByRole("dialog", { name: "OverviewProvidersChooseUpTo" });
    const boxes = within(sheet).getAllByRole("checkbox") as HTMLInputElement[];
    expect(boxes.map((box) => box.getAttribute("aria-label"))).toEqual([
      "CODEX",
      "CLAUDE",
      "GEMINI",
      "CURSOR",
      "COPILOT",
      "ZAI",
      "KIMI",
    ]);
    expect(boxes.map((box) => box.checked)).toEqual([true, true, true, true, true, true, false]);
    expect(boxes[6].disabled).toBe(true);
  });

  it("saves the selection without the unchecked provider, in provider order", () => {
    const { set } = renderTab();
    fireEvent.click(screen.getByRole("button", { name: "OverviewProvidersConfigure" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "CLAUDE" }));
    expect(set).toHaveBeenCalledWith({
      mergedOverviewSelectedProviders: ["codex", "gemini", "cursor", "copilot", "zai"],
    });
  });

  it("adds a provider below the limit and closes with Done", () => {
    const { set } = renderTab({ mergedOverviewSelectedProviders: ["zai", "codex"] });
    fireEvent.click(screen.getByRole("button", { name: "OverviewProvidersConfigure" }));
    expect((screen.getByRole("checkbox", { name: "KIMI" }) as HTMLInputElement).disabled).toBe(false);
    fireEvent.click(screen.getByRole("checkbox", { name: "KIMI" }));
    expect(set).toHaveBeenCalledWith({ mergedOverviewSelectedProviders: ["codex", "zai", "kimi"] });
    fireEvent.click(screen.getByRole("button", { name: "DialogDone" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
