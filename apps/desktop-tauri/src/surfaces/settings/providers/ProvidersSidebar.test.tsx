import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { loadStyles, loadStylesheet, ruleBlock } from "../../../test/styles";
import { ProvidersSidebar, type ProviderSidebarRow } from "./ProvidersSidebar";

vi.mock("../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => key }),
}));

function rows(): ProviderSidebarRow[] {
  return [
    { id: "codex", displayName: "Codex", enabled: true, status: "ok" },
    { id: "claude", displayName: "Claude", enabled: true, status: "error" },
    { id: "cursor", displayName: "Cursor", enabled: false, status: "disabled" },
  ];
}

function renderSidebar(overrides: Partial<Parameters<typeof ProvidersSidebar>[0]> = {}) {
  const props = {
    providers: rows(),
    selectedId: "codex",
    canReorder: true,
    onSelect: vi.fn(),
    onReorder: vi.fn(),
    onToggleEnabled: vi.fn(),
    ...overrides,
  };
  const view = render(<ProvidersSidebar {...props} />);
  return { ...props, ...view };
}

const option = (name: string) => screen.getByRole("option", { name: new RegExp(`^${name} `) });

describe("ProvidersSidebar", () => {
  it("shows a status dot on enabled rows only and dims disabled rows", () => {
    renderSidebar();
    expect(screen.getAllByRole("option")).toHaveLength(3);
    expect(option("Codex").querySelector(".providers-sidebar__status--ok")).not.toBeNull();
    expect(option("Claude").querySelector(".providers-sidebar__status--error")).not.toBeNull();
    const cursor = option("Cursor");
    expect(cursor.querySelector(".providers-sidebar__status")).toBeNull();
    expect(cursor).toHaveClass("providers-sidebar__row--disabled");
    expect(option("Codex")).toHaveAttribute("aria-selected", "true");
  });

  it("shows the service-status dot instead of the fetch dot while status is polled", () => {
    renderSidebar({
      providers: [
        { id: "codex", displayName: "Codex", enabled: true, status: "ok", service: "none" },
        { id: "claude", displayName: "Claude", enabled: true, status: "ok", service: "minor" },
        { id: "copilot", displayName: "Copilot", enabled: true, status: "ok", service: "critical" },
        { id: "gemini", displayName: "Gemini", enabled: true, status: "ok", service: "unknown" },
        { id: "zai", displayName: "Zai", enabled: true, status: "error" },
      ],
    });
    const dot = (name: string) => option(name).querySelector("[aria-hidden='true']:last-child");

    expect(dot("Codex")).toHaveClass("providers-sidebar__service providers-sidebar__service--none");
    expect(dot("Claude")).toHaveClass("providers-sidebar__service--minor");
    expect(dot("Copilot")).toHaveClass("providers-sidebar__service--critical");
    expect(dot("Gemini")).toHaveClass("providers-sidebar__service--unknown");
    expect(option("Codex").querySelector(".providers-sidebar__status")).toBeNull();
    expect(dot("Copilot")).toHaveAttribute(
      "title",
      "ProviderServiceStatusHelp".replace("{}", "ServiceStatusCriticalIssue"),
    );
    // No status page: the fetch-status dot stays.
    expect(dot("Zai")).toHaveClass("providers-sidebar__status--error");
    expect(option("Zai").querySelector(".providers-sidebar__service")).toBeNull();
  });

  it("colors the service dot with the macOS system colors at 6px", () => {
    const css = loadStyles();
    const base = ruleBlock(css, ".providers-sidebar__service");
    expect(base).toContain("width: 6px;");
    expect(base).toContain("height: 6px;");
    expect(base).toContain("background: var(--service-status-gray);");
    expect(ruleBlock(css, ".providers-sidebar__service--none")).toContain("var(--service-status-none)");
    expect(ruleBlock(css, ".providers-sidebar__service--minor")).toContain("var(--service-status-minor)");
    expect(ruleBlock(css, ".providers-sidebar__service--major")).toContain("var(--service-status-major)");
    expect(ruleBlock(css, ".providers-sidebar__service--critical")).toContain(
      "var(--service-status-critical)",
    );
    const dark = ruleBlock(css, '[data-theme="dark"]');
    for (const decl of [
      "--service-status-none: #30d158;",
      "--service-status-minor: #ffd60a;",
      "--service-status-major: #ff9f0a;",
      "--service-status-critical: #ff453a;",
      "--service-status-gray: #8e8e93;",
    ]) {
      expect(dark).toContain(decl);
    }
    const light = ruleBlock(css, '[data-theme="light"]');
    for (const decl of [
      "--service-status-none: #34c759;",
      "--service-status-minor: #ffcc00;",
      "--service-status-major: #ff9500;",
      "--service-status-critical: #ff3b30;",
      "--service-status-gray: #8e8e93;",
    ]) {
      expect(light).toContain(decl);
    }
  });

  it("draws the sidebar glyphs white on dark and dark on light", () => {
    const css = loadStylesheet("surfaces/settings/settings-layout.css");
    expect(ruleBlock(css, ".providers-sidebar__row .provider-icon")).toContain(
      "filter: brightness(0) invert(1);",
    );
    const light = ruleBlock(css, '[data-theme="light"] .providers-sidebar__row .provider-icon');
    expect(light).toContain("filter: brightness(0);");
    expect(light).not.toContain("invert");
  });

  it("says when no provider matches", () => {
    renderSidebar({ providers: [], selectedId: null });
    expect(screen.getByText("ProviderSidebarNoMatches")).toBeInTheDocument();
    expect(screen.queryAllByRole("option")).toHaveLength(0);
  });

  it("selects with the arrow keys and moves with Alt+Arrow", () => {
    const { onSelect, onReorder } = renderSidebar();
    fireEvent.keyDown(option("Codex"), { key: "ArrowDown" });
    expect(onSelect).toHaveBeenCalledWith("claude");
    expect(onReorder).not.toHaveBeenCalled();

    fireEvent.keyDown(option("Codex"), { key: "ArrowDown", altKey: true });
    expect(onReorder).toHaveBeenCalledWith(["claude", "codex", "cursor"]);
  });

  it("keeps Alt+Arrow and drag inert while the list cannot be reordered", () => {
    const { onReorder } = renderSidebar({ canReorder: false });
    fireEvent.keyDown(option("Codex"), { key: "ArrowDown", altKey: true });
    expect(onReorder).not.toHaveBeenCalled();
    expect(option("Codex")).toHaveAttribute("draggable", "false");
  });

  it("toggles a provider from the row context menu", () => {
    const { onToggleEnabled } = renderSidebar();
    fireEvent.contextMenu(option("Claude"), { clientX: 40, clientY: 60 });
    const menu = screen.getByRole("menu");
    fireEvent.click(within(menu).getByRole("menuitem", { name: "ProviderContextDisable" }));
    expect(onToggleEnabled).toHaveBeenCalledWith("claude", false);
    expect(screen.queryByRole("menu")).toBeNull();

    fireEvent.contextMenu(option("Cursor"));
    fireEvent.click(screen.getByRole("menuitem", { name: "ProviderContextEnable" }));
    expect(onToggleEnabled).toHaveBeenCalledWith("cursor", true);
  });

  it("moves a provider from the context menu and disables impossible moves", () => {
    const { onReorder } = renderSidebar();
    fireEvent.contextMenu(option("Codex"));
    expect(screen.getByRole("menuitem", { name: "ProviderSidebarMoveUp" })).toBeDisabled();
    fireEvent.click(screen.getByRole("menuitem", { name: "ProviderSidebarMoveDown" }));
    expect(onReorder).toHaveBeenCalledWith(["claude", "codex", "cursor"]);
  });

  it("disables Move Up and Move Down while sorted or searching", () => {
    renderSidebar({ canReorder: false });
    fireEvent.contextMenu(option("Claude"));
    expect(screen.getByRole("menuitem", { name: "ProviderSidebarMoveUp" })).toBeDisabled();
    expect(screen.getByRole("menuitem", { name: "ProviderSidebarMoveDown" })).toBeDisabled();
    expect(screen.getByRole("menuitem", { name: "ProviderContextDisable" })).toBeEnabled();
  });

  it("opens the menu from the keyboard and closes it with Escape", () => {
    renderSidebar();
    const claude = option("Claude");
    fireEvent.keyDown(claude, { key: "F10", shiftKey: true });
    const menu = screen.getByRole("menu");
    expect(menu).toBeInTheDocument();
    fireEvent.keyDown(menu, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(claude).toHaveFocus();

    fireEvent.keyDown(claude, { key: "ContextMenu" });
    expect(screen.getByRole("menu")).toBeInTheDocument();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("reorders by drag and drop", () => {
    const { onReorder } = renderSidebar();
    const store = new Map<string, string>();
    const dataTransfer = {
      setData: (k: string, v: string) => store.set(k, v),
      getData: (k: string) => store.get(k) ?? "",
      effectAllowed: "",
      dropEffect: "",
    };
    fireEvent.dragStart(option("Cursor"), { dataTransfer });
    fireEvent.dragOver(option("Codex"), { dataTransfer });
    fireEvent.drop(option("Codex"), { dataTransfer });
    expect(onReorder).toHaveBeenCalledWith(["cursor", "codex", "claude"]);
  });

  it("shows no drop target when reordering stops during a drag", () => {
    const view = renderSidebar();
    const dataTransfer = { setData: () => {}, getData: () => "", effectAllowed: "", dropEffect: "" };
    fireEvent.dragStart(option("Cursor"), { dataTransfer });
    view.rerender(
      <ProvidersSidebar
        providers={view.providers}
        selectedId={view.selectedId}
        canReorder={false}
        onSelect={view.onSelect}
        onReorder={view.onReorder}
        onToggleEnabled={view.onToggleEnabled}
      />,
    );
    // fireEvent returns false only when the handler called preventDefault (accepted the drop).
    expect(fireEvent.dragOver(option("Codex"), { dataTransfer })).toBe(true);
    expect(option("Codex")).not.toHaveClass("providers-sidebar__row--drop");
    fireEvent.drop(option("Codex"), { dataTransfer });
    expect(view.onReorder).not.toHaveBeenCalled();
  });
});
