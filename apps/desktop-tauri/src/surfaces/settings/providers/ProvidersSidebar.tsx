import {
  Fragment,
  type DragEvent as ReactDragEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  useLayoutEffect,
  useEffect,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
import { useLocale } from "../../../hooks/useLocale";
import type { LocaleKey } from "../../../i18n/keys";
import { ProviderIcon } from "../../../components/providers/ProviderIcon";
import type { ServiceStatusIndicator } from "../../../types/bridge";
import { SERVICE_STATUS_TO_KEY } from "../../../lib/serviceStatus";

/** Last-fetch state mapped from a ProviderUsageSnapshot / settings pair. */
export type ProviderSidebarStatus =
  | "ok"
  | "stale"
  | "error"
  | "disabled"
  | "loading";

export interface ProviderSidebarRow {
  id: string;
  displayName: string;
  enabled: boolean;
  status: ProviderSidebarStatus;
  /**
   * Service status while "Check provider status" is on and the provider has
   * a status page (`unknown` before the first poll). It replaces the
   * fetch-status dot, as on macOS (`PreferencesSidebar.swift:197-203`).
   */
  service?: ServiceStatusIndicator;
}

interface Props {
  /** Rows to show, already filtered by the search and in display order. */
  providers: ProviderSidebarRow[];
  /** Selected provider, or null while an app pane is selected. */
  selectedId: string | null;
  /** False while sorted alphabetically or filtered: drag, Alt+Arrow and Move are off. */
  canReorder: boolean;
  onSelect: (id: string) => void;
  /** Full provider order after a move; only called while `canReorder`. */
  onReorder: (orderedIds: string[]) => void;
  onToggleEnabled: (id: string, enabled: boolean) => void;
  disabled?: boolean;
}

const STATUS_TO_KEY: Record<ProviderSidebarStatus, LocaleKey> = {
  ok: "ProviderStatusOk",
  stale: "ProviderStatusStale",
  error: "ProviderStatusError",
  disabled: "ProviderStatusDisabled",
  loading: "ProviderStatusLoading",
};

interface MenuState {
  id: string;
  x: number;
  y: number;
}

/**
 * Provider rows of the Settings sidebar (macOS `SettingsSidebarProviderRow`):
 * a 16px icon, the name and a fetch-status dot on enabled rows; disabled rows
 * are dimmed. Enable/Disable, Move Up and Move Down live in the row context
 * menu (right-click, Shift+F10 or the Menu key). Reorder also works by HTML5
 * drag and Alt+ArrowUp / Alt+ArrowDown, only while `canReorder`.
 */
export function ProvidersSidebar({
  providers,
  selectedId,
  canReorder,
  onSelect,
  onReorder,
  onToggleEnabled,
  disabled,
}: Props) {
  const { t } = useLocale();
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);
  const [menu, setMenu] = useState<MenuState | null>(null);
  const rowRefs = useRef(new Map<string, HTMLLIElement>());
  const reorderable = canReorder && !disabled;
  const ids = providers.map((p) => p.id);

  const focusRow = (id: string) => rowRefs.current.get(id)?.focus();

  const moveId = (id: string, delta: number) => {
    if (!reorderable) return;
    const idx = ids.indexOf(id);
    const target = idx + delta;
    if (idx < 0 || target < 0 || target >= ids.length) return;
    const next = [...ids];
    next.splice(idx, 1);
    next.splice(target, 0, id);
    onReorder(next);
  };

  const handleDragStart = (id: string) => (e: ReactDragEvent<HTMLLIElement>) => {
    if (!reorderable) return;
    setDragId(id);
    e.dataTransfer.effectAllowed = "move";
    try {
      e.dataTransfer.setData("text/plain", id);
    } catch {
      /* Some engines throw on setData during dragstart; the id is kept in state. */
    }
  };

  const handleDragOver = (overId: string) => (e: ReactDragEvent<HTMLLIElement>) => {
    // No drop target once reordering stops mid-drag (sort, search or a save).
    if (!reorderable || !dragId || dragId === overId) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    if (dropTargetId !== overId) setDropTargetId(overId);
  };

  const handleDrop = (overId: string) => (e: ReactDragEvent<HTMLLIElement>) => {
    if (!dragId || dragId === overId) return;
    e.preventDefault();
    const from = ids.indexOf(dragId);
    const to = ids.indexOf(overId);
    setDragId(null);
    setDropTargetId(null);
    if (from < 0 || to < 0 || !reorderable) return;
    const next = [...ids];
    next.splice(from, 1);
    next.splice(to, 0, dragId);
    onReorder(next);
  };

  const handleDragEnd = () => {
    setDragId(null);
    setDropTargetId(null);
  };

  const openMenuAtRow = (id: string) => {
    const rect = rowRefs.current.get(id)?.getBoundingClientRect();
    setMenu({ id, x: (rect?.left ?? 0) + 24, y: rect?.bottom ?? 0 });
  };

  const handleKey = (row: ProviderSidebarRow, index: number) => (
    e: ReactKeyboardEvent<HTMLLIElement>,
  ) => {
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const delta = e.key === "ArrowUp" ? -1 : 1;
      if (e.altKey) {
        moveId(row.id, delta);
        return;
      }
      const next = providers[index + delta];
      if (next) {
        onSelect(next.id);
        focusRow(next.id);
      }
      return;
    }
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onSelect(row.id);
      return;
    }
    if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
      e.preventDefault();
      openMenuAtRow(row.id);
    }
  };

  const handleContextMenu = (id: string) => (e: ReactMouseEvent<HTMLLIElement>) => {
    e.preventDefault();
    setMenu({ id, x: e.clientX, y: e.clientY });
  };

  const menuRow = menu ? providers.find((p) => p.id === menu.id) ?? null : null;
  const menuIndex = menuRow ? ids.indexOf(menuRow.id) : -1;
  const focusId = selectedId && ids.includes(selectedId) ? selectedId : ids[0];

  return (
    <>
      <ul
        className="providers-sidebar"
        role="listbox"
        aria-label={t("ProvidersAriaLabel")}
        aria-orientation="vertical"
      >
        {providers.length === 0 && (
          <li className="providers-sidebar__empty" role="presentation">
            {t("ProviderSidebarNoMatches")}
          </li>
        )}
        {providers.map((p, index) => {
          const isSelected = p.id === selectedId;
          const statusLabel = p.service
            ? t("ProviderServiceStatusHelp").replace("{}", t(SERVICE_STATUS_TO_KEY[p.service]))
            : t(STATUS_TO_KEY[p.status]);
          const cls = [
            "providers-sidebar__row",
            isSelected && "providers-sidebar__row--selected",
            !p.enabled && "providers-sidebar__row--disabled",
            dragId === p.id && "providers-sidebar__row--dragging",
            dropTargetId === p.id && "providers-sidebar__row--drop",
          ]
            .filter(Boolean)
            .join(" ");

          return (
            <li
              key={p.id}
              ref={(node) => {
                if (node) rowRefs.current.set(p.id, node);
                else rowRefs.current.delete(p.id);
              }}
              className={cls}
              role="option"
              tabIndex={p.id === focusId ? 0 : -1}
              aria-selected={isSelected}
              aria-label={`${p.displayName} — ${statusLabel}`}
              data-provider-id={p.id}
              draggable={reorderable}
              onClick={() => onSelect(p.id)}
              onKeyDown={handleKey(p, index)}
              onContextMenu={handleContextMenu(p.id)}
              onDragStart={handleDragStart(p.id)}
              onDragOver={handleDragOver(p.id)}
              onDrop={handleDrop(p.id)}
              onDragEnd={handleDragEnd}
            >
              <ProviderIcon providerId={p.id} size={16} />
              <span className="providers-sidebar__name" title={p.displayName}>
                {p.displayName}
              </span>
              {p.enabled && p.service && (
                <span
                  className={`providers-sidebar__service providers-sidebar__service--${p.service}`}
                  title={statusLabel}
                  aria-hidden="true"
                />
              )}
              {p.enabled && !p.service && (
                <span
                  className={`providers-sidebar__status providers-sidebar__status--${p.status}`}
                  title={statusLabel}
                  aria-hidden="true"
                />
              )}
            </li>
          );
        })}
      </ul>
      {menu && menuRow && (
        <ProviderRowMenu
          x={menu.x}
          y={menu.y}
          onClose={(refocus) => {
            setMenu(null);
            if (refocus) focusRow(menu.id);
          }}
          items={[
            {
              label: t(menuRow.enabled ? "ProviderContextDisable" : "ProviderContextEnable"),
              disabled: Boolean(disabled),
              run: () => onToggleEnabled(menuRow.id, !menuRow.enabled),
            },
            {
              label: t("ProviderSidebarMoveUp"),
              disabled: !reorderable || menuIndex <= 0,
              run: () => moveId(menuRow.id, -1),
            },
            {
              label: t("ProviderSidebarMoveDown"),
              disabled: !reorderable || menuIndex >= ids.length - 1,
              run: () => moveId(menuRow.id, 1),
            },
          ]}
        />
      )}
    </>
  );
}

export interface MenuItem {
  label: string;
  disabled: boolean;
  run: () => void;
  /** Set for a checkbox item; its state shows as a check mark. */
  checked?: boolean;
  /** Native-style tooltip for the item. */
  title?: string;
  /** Draws a divider above the item. */
  separatorBefore?: boolean;
}

const MENU_EDGE = 4;

/** A small floating menu at a point, kept inside the window. */
export function ProviderRowMenu({
  x,
  y,
  items,
  onClose,
}: {
  x: number;
  y: number;
  items: MenuItem[];
  /** `refocus`: return focus to the row (keyboard close or after a command). */
  onClose: (refocus: boolean) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });
  const onCloseRef = useRef(onClose);
  useLayoutEffect(() => {
    onCloseRef.current = onClose;
  });

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const left = Math.max(MENU_EDGE, Math.min(x, window.innerWidth - rect.width - MENU_EDGE));
    const top = Math.max(MENU_EDGE, Math.min(y, window.innerHeight - rect.height - MENU_EDGE));
    setPos({ left, top });
    el.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [x, y]);

  useEffect(() => {
    const onPointer = (e: MouseEvent) => {
      if (ref.current?.contains(e.target as Node)) return;
      // A `data-menu-toggle` button closes its own menu on click, not here.
      if ((e.target as Element | null)?.closest?.("[data-menu-toggle]")) return;
      onCloseRef.current(false);
    };
    const onDismiss = () => onCloseRef.current(false);
    document.addEventListener("mousedown", onPointer, true);
    window.addEventListener("blur", onDismiss);
    window.addEventListener("resize", onDismiss);
    return () => {
      document.removeEventListener("mousedown", onPointer, true);
      window.removeEventListener("blur", onDismiss);
      window.removeEventListener("resize", onDismiss);
    };
  }, []);

  const handleKey = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape" || e.key === "Tab") {
      e.preventDefault();
      onClose(true);
      return;
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const buttons = Array.from(
        ref.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [],
      );
      if (buttons.length === 0) return;
      const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
      const step = e.key === "ArrowDown" ? 1 : -1;
      buttons[(at + step + buttons.length) % buttons.length].focus();
    }
  };

  return createPortal(
    <div
      ref={ref}
      className="providers-sidebar-menu"
      role="menu"
      style={{ left: pos.left, top: pos.top }}
      onKeyDown={handleKey}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item) => (
        <Fragment key={item.label}>
          {item.separatorBefore && (
            <div className="providers-sidebar-menu__separator" role="separator" />
          )}
          <button
            type="button"
            role={item.checked === undefined ? "menuitem" : "menuitemcheckbox"}
            aria-checked={item.checked}
            className="providers-sidebar-menu__item"
            title={item.title}
            disabled={item.disabled}
            onClick={() => {
              item.run();
              onClose(true);
            }}
          >
            {item.checked !== undefined && (
              <span className="providers-sidebar-menu__check" aria-hidden="true">
                {item.checked ? "✓" : ""}
              </span>
            )}
            {item.label}
          </button>
        </Fragment>
      ))}
    </div>,
    document.body,
  );
}
