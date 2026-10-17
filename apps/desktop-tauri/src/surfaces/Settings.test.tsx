import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BootstrapState, SurfaceTarget } from "../types/bridge";

const mocks = vi.hoisted(() => ({
  update: vi.fn(),
  reorderVisibleProviders: vi.fn(),
  restoreDefaultProviderOrder: vi.fn(),
  setSurfaceMode: vi.fn(),
  windowLabel: "settings",
  shellTarget: null as SurfaceTarget | null,
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    setSize: () => Promise.resolve(),
    setPosition: () => Promise.resolve(),
    minimize: () => Promise.resolve(),
  }),
  LogicalPosition: class {},
  LogicalSize: class {},
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({ label: mocks.windowLabel }),
}));
vi.mock("../lib/tauri", () => ({
  closeSettingsWindow: vi.fn(),
  getWorkAreaRect: () => Promise.resolve(null),
  getProviderStatuses: () => Promise.resolve([]),
  setSurfaceMode: mocks.setSurfaceMode,
  reorderVisibleProviders: mocks.reorderVisibleProviders,
  restoreDefaultProviderOrder: mocks.restoreDefaultProviderOrder,
}));
vi.mock("../hooks/useSettings", () => ({
  useSettings: (settings: unknown) => ({
    settings,
    saving: false,
    error: null,
    update: mocks.update,
  }),
}));
vi.mock("../hooks/useProviders", () => ({ useProviders: () => ({ providers: [] }) }));
vi.mock("../hooks/useSurfaceMode", () => ({ useSurfaceTarget: () => mocks.shellTarget }));
vi.mock("../hooks/useLocale", () => ({ useLocale: () => ({ t: (key: string) => key }) }));
vi.mock("./settings/tabs/GeneralTab", () => ({
  default: ({ mode }: { mode: string }) => <div data-testid="tab">general:{mode}</div>,
}));
vi.mock("./settings/tabs/MenuBarTab", () => ({
  default: () => <div data-testid="tab">menuBar</div>,
}));
vi.mock("./settings/tabs/MenuTab", () => ({
  default: () => <div data-testid="tab">menu</div>,
}));
vi.mock("./settings/tabs/AdvancedTab", () => ({
  default: () => <div data-testid="tab">advanced</div>,
}));
vi.mock("./settings/tabs/HooksTab", () => ({
  default: () => <div data-testid="tab">hooks</div>,
}));
vi.mock("./settings/tabs/AboutTab", () => ({
  default: () => <div data-testid="tab">about</div>,
}));
vi.mock("./settings/tabs/UsageSpendTab", () => ({
  default: () => <div data-testid="tab">usageSpend</div>,
}));
vi.mock("./settings/tabs/ProvidersTab", () => ({
  default: ({ providerId }: { providerId: string | null }) => (
    <div data-testid="tab">providers:{String(providerId)}</div>
  ),
}));

import Settings from "./Settings";

const providers = [
  { id: "codex", displayName: "Codex", cookieDomain: null },
  { id: "zai", displayName: "z.ai", cookieDomain: null },
  { id: "claude", displayName: "Claude", cookieDomain: null },
  { id: "cline", displayName: "Cline", cookieDomain: null },
  { id: "grok", displayName: "Grok", cookieDomain: null },
];

function makeState(settings: Record<string, unknown> = {}): BootstrapState {
  return {
    settings: {
      enabledProviders: ["codex", "grok", "zai"],
      providersSortedAlphabetically: false,
      ...settings,
    },
    providers,
  } as unknown as BootstrapState;
}

const state = makeState();
const title = () => screen.getByTestId("settings-title").textContent;
const providerNames = () =>
  within(screen.getByRole("listbox", { name: "ProvidersAriaLabel" }))
    .queryAllByRole("option")
    .map((row) => row.getAttribute("data-provider-id"));
const row = (id: string) =>
  screen.getByRole("listbox").querySelector<HTMLElement>(`[data-provider-id="${id}"]`)!;

describe("Settings sidebar", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.windowLabel = "settings";
    mocks.shellTarget = null;
    mocks.reorderVisibleProviders.mockResolvedValue([]);
    mocks.restoreDefaultProviderOrder.mockResolvedValue([]);
  });

  it("lists the app panes in the macOS order, then the providers with an on count", () => {
    const { container } = render(<Settings state={state} />);
    const panes = Array.from(
      container.querySelectorAll<HTMLElement>(".settings-sidebar__pane"),
      (pane) => pane.dataset.paneId,
    );
    expect(panes).toEqual([
      "general",
      "usageSpend",
      "notifications",
      "menuBar",
      "menu",
      "advanced",
      "hooks",
      "about",
    ]);
    expect(container.querySelectorAll(".settings-sidebar__chip")).toHaveLength(7);
    expect(container.querySelector(".settings-sidebar__app-icon")).not.toBeNull();
    expect(providerNames()).toEqual(["codex", "zai", "claude", "cline", "grok"]);
    expect(screen.getByText("SettingsSidebarProvidersOnCount")).toBeInTheDocument();
  });

  it("titles the window after the pane, or the provider on a provider pane", () => {
    render(<Settings state={state} />);
    expect(title()).toBe("TabGeneral");
    fireEvent.click(screen.getByRole("button", { name: "TabMenuBar" }));
    expect(title()).toBe("TabMenuBar");
    expect(screen.getByTestId("tab").textContent).toBe("menuBar");
    fireEvent.click(row("claude"));
    expect(title()).toBe("Claude");
    expect(screen.getByTestId("tab").textContent).toBe("providers:claude");
    expect(row("claude")).toHaveAttribute("aria-selected", "true");
  });

  it("filters providers by name or id without changing the selection", () => {
    render(<Settings state={state} initialTab="providers:codex" />);
    const search = screen.getByRole("searchbox", { name: "ProviderSidebarSearch" });
    fireEvent.change(search, { target: { value: "cl" } });
    expect(providerNames()).toEqual(["claude", "cline"]);
    expect(screen.getByTestId("tab").textContent).toBe("providers:codex");

    fireEvent.change(search, { target: { value: "ZA" } });
    expect(providerNames()).toEqual(["zai"]);

    fireEvent.change(search, { target: { value: "nothing" } });
    expect(providerNames()).toEqual([]);
    expect(screen.getByText("ProviderSidebarNoMatches")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "ProviderSidebarClearSearch" }));
    expect(providerNames()).toHaveLength(5);
  });

  it("sorts enabled providers first, alphabetically, without saving an order", () => {
    render(<Settings state={makeState({ providersSortedAlphabetically: true })} />);
    expect(providerNames()).toEqual(["codex", "grok", "zai", "claude", "cline"]);
    const sort = screen.getByRole("button", { name: "SettingsSidebarSortProviders" });
    expect(sort).toHaveAttribute("aria-pressed", "true");
    expect(sort).toHaveAttribute("title", "SettingsSidebarSortOnHint");

    fireEvent.keyDown(row("codex"), { key: "ArrowDown", altKey: true });
    expect(mocks.reorderVisibleProviders).not.toHaveBeenCalled();

    fireEvent.click(sort);
    expect(mocks.update).toHaveBeenCalledWith({ providersSortedAlphabetically: false });
  });

  it("turns sorting on from the toggle", () => {
    render(<Settings state={state} />);
    const sort = screen.getByRole("button", { name: "SettingsSidebarSortProviders" });
    expect(sort).toHaveAttribute("aria-pressed", "false");
    fireEvent.click(sort);
    expect(mocks.update).toHaveBeenCalledWith({ providersSortedAlphabetically: true });
  });

  it("enables and disables providers from the context menu in saved order", () => {
    render(<Settings state={state} />);
    fireEvent.contextMenu(row("claude"));
    fireEvent.click(screen.getByRole("menuitem", { name: "ProviderContextEnable" }));
    expect(mocks.update).toHaveBeenLastCalledWith({
      enabledProviders: ["codex", "zai", "claude", "grok"],
    });

    fireEvent.contextMenu(row("zai"));
    fireEvent.click(screen.getByRole("menuitem", { name: "ProviderContextDisable" }));
    expect(mocks.update).toHaveBeenLastCalledWith({ enabledProviders: ["codex", "grok"] });
  });

  it("reorders with Alt+Arrow and saves the new order", () => {
    render(<Settings state={state} />);
    fireEvent.keyDown(row("codex"), { key: "ArrowDown", altKey: true });
    expect(mocks.reorderVisibleProviders).toHaveBeenCalledWith([
      "zai",
      "codex",
      "claude",
      "cline",
      "grok",
    ]);
    expect(providerNames()).toEqual(["zai", "codex", "claude", "cline", "grok"]);
  });

  it("does not reorder while a search filters the list", () => {
    render(<Settings state={state} />);
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "c" } });
    fireEvent.keyDown(row("codex"), { key: "ArrowDown", altKey: true });
    expect(mocks.reorderVisibleProviders).not.toHaveBeenCalled();
  });

  const optionsButton = () => screen.getByRole("button", { name: "ProviderSidebarOptions" });
  const undoItem = () => screen.getByRole("menuitem", { name: "ProviderSidebarUndoOrder" });

  it("shows only enabled providers with the enabled-only option, and does not reorder then", () => {
    render(<Settings state={makeState({ providersEnabledOnly: true })} />);
    expect(providerNames()).toEqual(["codex", "zai", "grok"]);
    fireEvent.keyDown(row("codex"), { key: "ArrowDown", altKey: true });
    expect(mocks.reorderVisibleProviders).not.toHaveBeenCalled();

    fireEvent.click(optionsButton());
    expect(screen.getByRole("menuitemcheckbox", { name: "ProviderSidebarShowEnabledOnly" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "ProviderSidebarShowEnabledOnly" }));
    expect(mocks.update).toHaveBeenCalledWith({ providersEnabledOnly: false });
  });

  it("turns the enabled-only option on from the options menu", () => {
    render(<Settings state={state} />);
    fireEvent.click(optionsButton());
    fireEvent.click(screen.getByRole("menuitemcheckbox", { name: "ProviderSidebarShowEnabledOnly" }));
    expect(mocks.update).toHaveBeenCalledWith({ providersEnabledOnly: true });
  });

  it("undoes the last drag reorder from the options menu", () => {
    render(<Settings state={state} />);
    fireEvent.keyDown(row("codex"), { key: "ArrowDown", altKey: true });
    expect(providerNames()).toEqual(["zai", "codex", "claude", "cline", "grok"]);

    fireEvent.click(optionsButton());
    fireEvent.click(undoItem());
    expect(mocks.reorderVisibleProviders).toHaveBeenLastCalledWith([
      "codex",
      "zai",
      "claude",
      "cline",
      "grok",
    ]);
    expect(providerNames()).toEqual(["codex", "zai", "claude", "cline", "grok"]);
  });

  it("undoes the last drag reorder with Ctrl+Z", () => {
    const { container } = render(<Settings state={state} />);
    fireEvent.keyDown(row("codex"), { key: "ArrowDown", altKey: true });
    fireEvent.keyDown(container.querySelector(".settings-sidebar")!, { key: "z", ctrlKey: true });
    expect(mocks.reorderVisibleProviders).toHaveBeenLastCalledWith([
      "codex",
      "zai",
      "claude",
      "cline",
      "grok",
    ]);
  });

  it("drops the undo once the saved order changes outside the sidebar", () => {
    const { rerender } = render(<Settings state={state} />);
    fireEvent.keyDown(row("codex"), { key: "ArrowDown", altKey: true });
    fireEvent.click(optionsButton());
    expect(undoItem()).toBeEnabled();
    fireEvent.click(optionsButton());

    const moved = [...providers].reverse();
    rerender(<Settings state={{ ...state, providers: moved } as BootstrapState} />);
    fireEvent.click(optionsButton());
    expect(undoItem()).toBeDisabled();
  });

  it("restores the default order and turns sorting off, and undo brings both back", async () => {
    mocks.restoreDefaultProviderOrder.mockResolvedValue(
      providers.map((p, order) => ({ id: p.id, displayName: p.displayName, enabled: true, order })),
    );
    render(<Settings state={makeState({ providersSortedAlphabetically: true })} />);
    fireEvent.click(optionsButton());
    fireEvent.click(screen.getByRole("menuitem", { name: "ProviderSidebarRestoreDefaultOrder" }));
    expect(mocks.update).toHaveBeenCalledWith({ providersSortedAlphabetically: false });
    expect(mocks.restoreDefaultProviderOrder).toHaveBeenCalledTimes(1);

    fireEvent.click(optionsButton());
    await waitFor(() => expect(undoItem()).toBeEnabled());
    fireEvent.click(undoItem());
    expect(mocks.update).toHaveBeenLastCalledWith({ providersSortedAlphabetically: true });
    expect(mocks.reorderVisibleProviders).toHaveBeenLastCalledWith([
      "codex",
      "zai",
      "claude",
      "cline",
      "grok",
    ]);
  });

  it("turns sorting back on when restoring the default order fails", async () => {
    mocks.restoreDefaultProviderOrder.mockRejectedValue(new Error("save failed"));
    render(<Settings state={makeState({ providersSortedAlphabetically: true })} />);
    fireEvent.click(optionsButton());
    fireEvent.click(screen.getByRole("menuitem", { name: "ProviderSidebarRestoreDefaultOrder" }));
    expect(mocks.update).toHaveBeenCalledWith({ providersSortedAlphabetically: false });
    await waitFor(() =>
      expect(mocks.update).toHaveBeenLastCalledWith({ providersSortedAlphabetically: true }),
    );
  });

  it("drives the main window surface only outside the detached window", () => {
    mocks.windowLabel = "main";
    render(<Settings state={state} />);
    fireEvent.click(row("grok"));
    expect(mocks.setSurfaceMode).toHaveBeenCalledWith("settings", {
      kind: "settings",
      tab: "providers:grok",
    });
  });
});

describe("Settings deep links", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.windowLabel = "settings";
    mocks.shellTarget = null;
  });

  it("opens cold on the requested provider row", () => {
    render(<Settings state={state} initialTab="providers:claude" tabRequestId={1} />);
    expect(screen.getByTestId("tab").textContent).toBe("providers:claude");
    expect(row("claude")).toHaveAttribute("aria-selected", "true");
    expect(title()).toBe("Claude");
  });

  it("selects a provider from the proof-mode shell target", () => {
    mocks.shellTarget = { kind: "settings", tab: "providers:codex" };
    render(<Settings state={state} />);
    expect(screen.getByTestId("tab").textContent).toBe("providers:codex");
    expect(row("codex")).toHaveAttribute("aria-selected", "true");
    expect(title()).toBe("Codex");
  });

  it("clears a search that would hide the deep-linked provider", () => {
    const { rerender } = render(<Settings state={state} initialTab="general" tabRequestId={0} />);
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "zai" } });
    rerender(<Settings state={state} initialTab="providers:cline" tabRequestId={1} />);
    expect(screen.getByRole("searchbox")).toHaveValue("");
    expect(row("cline")).toHaveAttribute("aria-selected", "true");
  });

  it("clears a search that would hide a provider named by the shell target", () => {
    mocks.windowLabel = "main";
    mocks.shellTarget = { kind: "settings", tab: "general" };
    const { rerender } = render(<Settings state={state} />);
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "zai" } });
    mocks.shellTarget = { kind: "settings", tab: "providers:cline" };
    rerender(<Settings state={state} />);
    expect(screen.getByRole("searchbox")).toHaveValue("");
    expect(row("cline")).toHaveAttribute("aria-selected", "true");
    expect(title()).toBe("Cline");
  });

  it("keeps the search when the shell target echoes a visible row click", () => {
    mocks.windowLabel = "main";
    mocks.shellTarget = { kind: "settings", tab: "general" };
    const { rerender } = render(<Settings state={state} />);
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "c" } });
    fireEvent.click(row("claude"));
    mocks.shellTarget = { kind: "settings", tab: "providers:claude" };
    rerender(<Settings state={state} />);
    expect(screen.getByRole("searchbox")).toHaveValue("c");
    expect(providerNames()).toEqual(["codex", "claude", "cline"]);
    expect(row("claude")).toHaveAttribute("aria-selected", "true");
  });

  it("keeps the last provider when a plain providers request arrives", () => {
    const { rerender } = render(
      <Settings state={state} initialTab="providers:grok" tabRequestId={1} />,
    );
    rerender(<Settings state={state} initialTab="about" tabRequestId={2} />);
    expect(screen.getByTestId("tab").textContent).toBe("about");
    expect(title()).toBe("TabAbout");

    rerender(<Settings state={state} initialTab="providers" tabRequestId={3} />);
    expect(screen.getByTestId("tab").textContent).toBe("providers:grok");
  });

  it("falls back to the first provider for a plain or unknown provider request", () => {
    render(<Settings state={state} initialTab="providers:nope" tabRequestId={1} />);
    expect(screen.getByTestId("tab").textContent).toBe("providers:codex");
  });
});
