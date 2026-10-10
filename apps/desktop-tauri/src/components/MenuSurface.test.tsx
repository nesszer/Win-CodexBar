import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
}));

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  ...tauriMocks,
}));
vi.mock("@tauri-apps/api/event", () => eventMocks);

import { LocaleProvider } from "../i18n/LocaleProvider";
import { buildBundle } from "../test/localeHarness";
import MenuSurface, { MenuSummary, type MenuFooterRow } from "./MenuSurface";

function renderSummary(total: number) {
  return render(
    <LocaleProvider>
      <MenuSummary
        total={total}
        errorCount={0}
        isRefreshing={false}
        lastRefresh={null}
      />
    </LocaleProvider>,
  );
}

describe("MenuSummary", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({ SummaryProvidersLabel: "providers" }),
    );
    eventMocks.listen.mockResolvedValue(() => {});
  });

  it("uses a singular provider label for one provider", async () => {
    renderSummary(1);

    expect(await screen.findByText("1 provider")).toBeInTheDocument();
  });

  it("keeps the plural provider label for multiple providers", async () => {
    renderSummary(2);

    expect(await screen.findByText("2 providers")).toBeInTheDocument();
  });
});

function renderFooter(groups: MenuFooterRow[][]) {
  return render(
    <LocaleProvider>
      <MenuSurface footerGroups={groups}>
        <div>cards</div>
      </MenuSurface>
    </LocaleProvider>,
  );
}

describe("MenuSurface footer", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle());
    eventMocks.listen.mockResolvedValue(() => {});
  });

  it("renders the Mac footer groups in order, each after a separator", async () => {
    const noop = vi.fn();
    renderFooter([
      [
        { id: "switchAccount", label: "Switch Account...", onClick: noop },
        { id: "dashboard", label: "Usage Dashboard", onClick: noop },
        { id: "statusPage", label: "Status Page", onClick: noop },
      ],
      [
        { id: "refresh", label: "Refresh", shortcut: "Ctrl+R", onClick: noop },
        { id: "settings", label: "Settings...", shortcut: "Ctrl+,", onClick: noop },
        { id: "about", label: "About CodexBar (v0.70.0)", onClick: noop },
        { id: "quit", label: "Quit", shortcut: "Ctrl+Q", onClick: noop },
      ],
    ]);

    const nav = await screen.findByRole("navigation", { name: "PanelMenu" });
    const rows = within(nav).getAllByRole("button");
    expect(rows.map((row) => row.textContent)).toEqual([
      "Switch Account...",
      "Usage Dashboard",
      "Status Page",
      "RefreshCtrl+R",
      "Settings...Ctrl+,",
      "About CodexBar (v0.70.0)",
      "QuitCtrl+Q",
    ]);
    expect(
      Array.from(nav.children).map((child) =>
        child.classList.contains("menu-surface__footer-sep") ? "sep" : child.textContent,
      ),
    ).toEqual([
      "sep",
      "Switch Account...",
      "Usage Dashboard",
      "Status Page",
      "sep",
      "RefreshCtrl+R",
      "Settings...Ctrl+,",
      "About CodexBar (v0.70.0)",
      "QuitCtrl+Q",
    ]);
  });

  it("draws an SVG icon on every row except Refresh, which gets the taller row", async () => {
    const noop = vi.fn();
    renderFooter([
      [
        { id: "switchAccount", label: "Switch Account...", onClick: noop },
        { id: "dashboard", label: "Usage Dashboard", onClick: noop },
        { id: "statusPage", label: "Status Page", onClick: noop },
        { id: "refresh", label: "Refresh", shortcut: "Ctrl+R", onClick: noop },
        { id: "settings", label: "Settings...", shortcut: "Ctrl+,", onClick: noop },
        { id: "about", label: "About CodexBar (v0.70.0)", onClick: noop },
        { id: "quit", label: "Quit", shortcut: "Ctrl+Q", onClick: noop },
      ],
    ]);

    const nav = await screen.findByRole("navigation", { name: "PanelMenu" });
    const rows = within(nav).getAllByRole("button");
    expect(rows.map((row) => row.querySelectorAll("svg").length)).toEqual([1, 1, 1, 0, 1, 1, 1]);
    expect(
      rows.map((row) => row.classList.contains("menu-surface__footer-row--refresh")),
    ).toEqual([false, false, false, true, false, false, false]);
    expect(
      Array.from(nav.querySelectorAll(".menu-surface__footer-shortcut")).map((s) => s.textContent),
    ).toEqual(["Ctrl+R", "Ctrl+,", "Ctrl+Q"]);
  });

  it("skips empty groups, so no separator is drawn for them", async () => {
    renderFooter([[], [{ id: "quit", label: "Quit", shortcut: "Ctrl+Q", onClick: vi.fn() }]]);

    const nav = await screen.findByRole("navigation", { name: "PanelMenu" });
    expect(nav.querySelectorAll(".menu-surface__footer-sep")).toHaveLength(1);
    expect(within(nav).getAllByRole("button").map((row) => row.textContent)).toEqual([
      "QuitCtrl+Q",
    ]);
  });

  it("runs the clicked row's action and names its shortcut for assistive tech", async () => {
    const onRefresh = vi.fn();
    const onSettings = vi.fn();
    renderFooter([
      [
        { id: "refresh", label: "Refresh", shortcut: "Ctrl+R", onClick: onRefresh },
        { id: "settings", label: "Settings...", shortcut: "Ctrl+,", onClick: onSettings },
      ],
    ]);

    const settings = await screen.findByRole("button", { name: "Settings..." });
    expect(screen.getByRole("button", { name: "Refresh" })).toHaveAttribute(
      "aria-keyshortcuts",
      "Control+R",
    );
    expect(settings).toHaveAttribute("aria-keyshortcuts", "Control+,");
    fireEvent.click(settings);

    expect(onSettings).toHaveBeenCalledTimes(1);
    expect(onRefresh).toHaveBeenCalledTimes(0);
  });

  it("renders no footer when every group is empty", async () => {
    renderFooter([[], []]);

    expect(await screen.findByText("cards")).toBeInTheDocument();
    expect(screen.queryByRole("navigation")).toBeNull();
  });
});
