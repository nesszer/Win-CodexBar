import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => key, language: "english" }),
}));
// The float bar section has its own tests; here it only marks the pane's end.
vi.mock("../../../floatbar/SettingsSection", () => ({
  default: () => <section aria-label="FloatBarSection" />,
}));

import MenuTab from "./MenuTab";
import type { SettingsSnapshot } from "../../../types/bridge";

const baseSettings = {
  enabledProviders: ["codex", "claude"],
  trayIconMode: "single",
  trayPanelAlwaysOnTop: false,
  overviewLayout: "detailed",
  showAsUsed: false,
  showAllTokenAccountsInMenu: false,
  resetTimeRelative: true,
  showResetWhenExhausted: false,
  showPace: false,
  quotaWarningMarkersVisible: true,
  workdayTickStyle: "subtle",
  weeklyProgressWorkDays: null,
  costSummaryDisplayStyle: "compact",
  costReportingPeriod: "rolling:30",
  agentSessionsEnabled: false,
  stayAwakeEnabled: false,
  agentSessionSshHosts: [],
  trayScalePercent: 100,
} as unknown as SettingsSnapshot;

function renderTab(overrides: Partial<SettingsSnapshot> = {}, set = vi.fn()) {
  const view = render(
    <MenuTab settings={{ ...baseSettings, ...overrides } as SettingsSnapshot} set={set} saving={false} />,
  );
  return { ...view, set };
}

const select = (name: string) => screen.getByRole("combobox", { name }) as HTMLSelectElement;
const optionLabels = (name: string) =>
  Array.from(select(name).options, (option) => option.textContent);

describe("MenuTab layout", () => {
  it("orders the sections like the Mac Menu pane with the Windows extras after Usage", () => {
    renderTab();
    expect(screen.getAllByRole("heading", { level: 3 }).map((h) => h.textContent)).toEqual([
      "MenuUsageSectionTitle",
      "MenuWindowsSectionTitle",
      "MenuContentSectionTitle",
      "AgentSessionsTitle",
      "MenuCostSummarySectionTitle",
    ]);
    const regions = screen.getAllByRole("region");
    expect(regions[regions.length - 1].getAttribute("aria-label")).toBe("FloatBarSection");
  });

  it("orders the Usage rows like the Mac", () => {
    renderTab();
    const usage = screen.getByRole("region", { name: "MenuUsageSectionTitle" });
    const labels = Array.from(usage.querySelectorAll(".settings-field__label"), (n) => n.textContent);
    expect(labels).toEqual([
      "UsageBarsFillLabel",
      "QuotaWarningMarkersLabel",
      "ShowPace",
      "WorkDaysLabel",
      "WorkdayTickAppearanceLabel",
      "ResetTimesLabel",
    ]);
  });

  it("offers the tray Panel scale as its only slider", () => {
    const { container } = renderTab();
    expect(
      Array.from(container.querySelectorAll('input[type="range"]'), (input) => input.getAttribute("aria-label")),
    ).toEqual(["PanelScaleLabel"]);
  });

  it("no longer hosts the provider switcher shortcuts", () => {
    renderTab();
    expect(screen.queryByText("SwitcherShortcutsTitle")).toBeNull();
  });
});

describe("MenuTab Usage section", () => {
  it("maps Usage bars fill to showAsUsed", () => {
    const { set } = renderTab();
    expect(select("UsageBarsFillLabel").value).toBe("remaining");
    expect(optionLabels("UsageBarsFillLabel")).toEqual(["UsageBarsFillRemaining", "UsageBarsFillUsed"]);
    fireEvent.change(select("UsageBarsFillLabel"), { target: { value: "used" } });
    expect(set).toHaveBeenCalledWith({ showAsUsed: true });
  });

  it("maps Reset times to resetTimeRelative", () => {
    const { set } = renderTab();
    expect(select("ResetTimesLabel").value).toBe("countdown");
    fireEvent.change(select("ResetTimesLabel"), { target: { value: "clock" } });
    expect(set).toHaveBeenCalledWith({ resetTimeRelative: false });
  });

  it("toggles quota warning markers and pace", () => {
    const { set } = renderTab();
    fireEvent.click(screen.getByRole("checkbox", { name: "QuotaWarningMarkersLabel" }));
    expect(set).toHaveBeenCalledWith({ quotaWarningMarkersVisible: false });
    fireEvent.click(screen.getByRole("checkbox", { name: "ShowPace" }));
    expect(set).toHaveBeenCalledWith({ showPace: true });
  });

  it("offers Automatic, 4, 5 and 7 work days and saves null for Automatic", () => {
    const { set } = renderTab({ weeklyProgressWorkDays: 5 });
    expect(select("WorkDaysLabel").value).toBe("5");
    expect(Array.from(select("WorkDaysLabel").options, (o) => o.value)).toEqual(["", "4", "5", "7"]);
    fireEvent.change(select("WorkDaysLabel"), { target: { value: "7" } });
    expect(set).toHaveBeenCalledWith({ weeklyProgressWorkDays: 7 });
    fireEvent.change(select("WorkDaysLabel"), { target: { value: "" } });
    expect(set).toHaveBeenCalledWith({ weeklyProgressWorkDays: null });
  });

  it("keeps a saved work day count that is not a Mac preset selectable", () => {
    renderTab({ weeklyProgressWorkDays: 3 });
    expect(select("WorkDaysLabel").value).toBe("3");
    expect(Array.from(select("WorkDaysLabel").options, (o) => o.value)).toEqual(["", "3", "4", "5", "7"]);
  });

  it("disables Workday tick appearance while Work days is Automatic", () => {
    renderTab();
    expect(select("WorkdayTickAppearanceLabel").disabled).toBe(true);
  });

  it("saves the Workday tick appearance once work days are set", () => {
    const { set } = renderTab({ weeklyProgressWorkDays: 5 });
    const tick = select("WorkdayTickAppearanceLabel");
    expect(tick.disabled).toBe(false);
    expect(tick.value).toBe("subtle");
    fireEvent.change(tick, { target: { value: "high_contrast" } });
    expect(set).toHaveBeenCalledWith({ workdayTickStyle: "high_contrast" });
  });
});

describe("MenuTab Windows section", () => {
  it("updates always on top and the exhausted reset display", () => {
    const { set } = renderTab();
    fireEvent.click(screen.getByRole("checkbox", { name: "TrayPanelAlwaysOnTopLabel" }));
    expect(set).toHaveBeenCalledWith({ trayPanelAlwaysOnTop: true });
    fireEvent.click(screen.getByRole("checkbox", { name: "ShowResetWhenExhausted" }));
    expect(set).toHaveBeenCalledWith({ showResetWhenExhausted: true });
  });

  it("shows the saved Panel scale and saves the new one when the slider is released", async () => {
    const set = vi.fn();
    await act(async () => {
      renderTab({ trayScalePercent: 150 }, set);
    });
    const slider = screen.getByRole("slider", { name: "PanelScaleLabel" });
    expect(slider).toHaveValue("150");
    expect(screen.getByText("PanelScaleLabel (150%)")).toBeInTheDocument();
    fireEvent.change(slider, { target: { value: "125" } });
    expect(set).not.toHaveBeenCalled();
    fireEvent.pointerUp(slider);
    expect(set).toHaveBeenCalledTimes(1);
    expect(set).toHaveBeenCalledWith({ trayScalePercent: 125 });
  });
});

describe("MenuTab Content section", () => {
  it("updates the Overview layout and token account preferences", () => {
    const { set } = renderTab();
    fireEvent.change(select("OverviewLayoutLabel"), { target: { value: "compact" } });
    expect(set).toHaveBeenCalledWith({ overviewLayout: "compact" });
    fireEvent.click(screen.getByRole("checkbox", { name: "ShowAllTokenAccountsLabel" }));
    expect(set).toHaveBeenCalledWith({ showAllTokenAccountsInMenu: true });
  });

  it("leads with the hover highlight toggle, on unless saved off", () => {
    const { set, unmount } = renderTab();
    const content = screen.getByRole("region", { name: "MenuContentSectionTitle" });
    const labels = Array.from(content.querySelectorAll(".settings-field__label"), (n) => n.textContent);
    expect(labels).toEqual([
      "HighlightMenuCardsOnHoverLabel",
      "OverviewLayoutLabel",
      "ShowAllTokenAccountsLabel",
    ]);
    const toggle = screen.getByRole("checkbox", { name: "HighlightMenuCardsOnHoverLabel" });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    expect(set).toHaveBeenCalledWith({ highlightMenuCardsOnHover: false });
    unmount();

    renderTab({ highlightMenuCardsOnHover: false });
    expect(
      screen.getByRole("checkbox", { name: "HighlightMenuCardsOnHoverLabel" }),
    ).not.toBeChecked();
  });
});

describe("MenuTab Agent sessions section", () => {
  it("orders Stay Awake, Agent sessions, then Additional SSH hosts", () => {
    renderTab();
    const group = screen.getByRole("region", { name: "AgentSessionsTitle" });
    expect(
      Array.from(group.querySelectorAll(".settings-field__label"), (n) => n.textContent),
    ).toEqual(["AgentSessionsStayAwakeLabel", "AgentSessionsEnableLabel", "AgentSessionsSshHostsLabel"]);
  });

  it("toggles Stay Awake and Agent sessions", () => {
    const { set } = renderTab();
    fireEvent.click(screen.getByRole("checkbox", { name: "AgentSessionsStayAwakeLabel" }));
    expect(set).toHaveBeenCalledWith({ stayAwakeEnabled: true });
    fireEvent.click(screen.getByRole("checkbox", { name: "AgentSessionsEnableLabel" }));
    expect(set).toHaveBeenCalledWith({ agentSessionsEnabled: true });
  });

  it("disables SSH hosts until agent sessions are on, then saves the parsed list on blur", () => {
    const off = renderTab();
    expect((screen.getByRole("textbox", { name: "AgentSessionsSshHostsLabel" }) as HTMLInputElement).disabled).toBe(
      true,
    );
    off.unmount();

    const { set } = renderTab({ agentSessionsEnabled: true });
    const input = screen.getByRole("textbox", { name: "AgentSessionsSshHostsLabel" }) as HTMLInputElement;
    expect(input.placeholder).toBe("user@host, user@host");
    fireEvent.change(input, { target: { value: "dev@box, , ops@vm, ci@runner" } });
    fireEvent.blur(input);
    expect(set).toHaveBeenCalledWith({ agentSessionSshHosts: ["dev@box", "ops@vm", "ci@runner"] });
  });
});

describe("MenuTab Cost summary section", () => {
  it("saves the cost summary display style", () => {
    const { set } = renderTab();
    expect(select("CostSummaryDisplayStyle").value).toBe("compact");
    fireEvent.change(select("CostSummaryDisplayStyle"), { target: { value: "hidden" } });
    expect(set).toHaveBeenCalledWith({ costSummaryDisplayStyle: "hidden" });
  });

  it("shows the saved History window and saves a new one", () => {
    const { set } = renderTab({ costReportingPeriod: "month-to-date" });
    const group = screen.getByRole("region", { name: "MenuCostSummarySectionTitle" });
    const picker = within(group).getByRole("combobox", { name: "CostPeriodHistoryWindow" }) as HTMLSelectElement;
    expect(picker.value).toBe("month-to-date");
    fireEvent.change(picker, { target: { value: "rolling:90" } });
    expect(set).toHaveBeenCalledWith({ costReportingPeriod: "rolling:90" });
  });

  it("falls back to 30 days for a missing or unreadable History window", () => {
    renderTab({ costReportingPeriod: "rolling:0" });
    expect(select("CostPeriodHistoryWindow").value).toBe("rolling:30");
  });
});
