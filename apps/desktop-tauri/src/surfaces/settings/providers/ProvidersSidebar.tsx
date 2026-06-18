import {
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type DragEvent as ReactDragEvent,
  useEffect,
  useRef,
  useState,
} from "react";
import { useLocale } from "../../../hooks/useLocale";
import type { LocaleKey } from "../../../i18n/keys";
import { ProviderIcon } from "../../../components/providers/ProviderIcon";
import { getProviderIcon } from "../../../components/providers/providerIcons";

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
  /** Primary subtitle text, e.g. source hint or disabled reason. */
  subtitlePrimary: string;
  /** Optional secondary metric line (e.g. "Session 42%"). */
  subtitleSecondary?: string;
}

interface Props {
  providers: ProviderSidebarRow[];
  selectedId: string | null;
  searchText: string;
  onSearchTextChange: (value: string) => void;
  onSelect: (id: string) => void;
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

/**
 * Providers sidebar — parity port of egui `render_providers_sidebar`
 * (rust/src/native_ui/preferences.rs:3370).
 *
 * Responsibilities (Phase 6a only):
 *  - render rows with brand icon + status dot + two-line subtitle + toggle
 *  - drag-and-drop reorder (HTML5 native; emits `onReorder`)
 *  - button reorder for WebView2/automation paths where HTML5 drag can be brittle
 *  - keyboard reorder: Alt+ArrowUp / Alt+ArrowDown on the selected row
 *  - mount-in reveal animation keyed on row id
 */
export function ProvidersSidebar({
  providers,
  selectedId,
  searchText,
  onSearchTextChange,
  onSelect,
  onReorder,
  onToggleEnabled,
  disabled,
}: Props) {
  const { t } = useLocale();

  // Optimistic local order so drag-drop feels instant while the backend
  // round-trips `reorder_providers`.
  const [localOrder, setLocalOrder] = useState<string[]>(() =>
    providers.map((p) => p.id),
  );
  useEffect(() => {
    setLocalOrder(providers.map((p) => p.id));
  }, [providers]);

  const byId = new Map(providers.map((p) => [p.id, p]));
  const ordered = localOrder
    .map((id) => byId.get(id))
    .filter((p): p is ProviderSidebarRow => Boolean(p));

  // Track previously-mounted ids to trigger a reveal animation for new rows.
  const seenRef = useRef<Set<string>>(new Set(ordered.map((p) => p.id)));
  const [justMounted, setJustMounted] = useState<Set<string>>(new Set());
  useEffect(() => {
    const seen = seenRef.current;
    const newly = new Set<string>();
    for (const p of ordered) {
      if (!seen.has(p.id)) {
        newly.add(p.id);
        seen.add(p.id);
      }
    }
    if (newly.size) {
      setJustMounted(newly);
      const h = window.setTimeout(() => setJustMounted(new Set()), 260);
      return () => window.clearTimeout(h);
    }
    return undefined;
  }, [ordered]);

  const [dragId, setDragId] = useState<string | null>(null);
  const [dropTargetId, setDropTargetId] = useState<string | null>(null);

  const commitReorder = (nextIds: string[]) => {
    setLocalOrder(nextIds);
    onReorder(nextIds);
  };

  const moveId = (id: string, delta: number) => {
    const idx = localOrder.indexOf(id);
    if (idx < 0) return;
    const target = idx + delta;
    if (target < 0 || target >= localOrder.length) return;
    const next = [...localOrder];
    next.splice(idx, 1);
    next.splice(target, 0, id);
    commitReorder(next);
  };

  const handleDragStart = (id: string) => (e: ReactDragEvent<HTMLLIElement>) => {
    if (disabled) return;
    setDragId(id);
    e.dataTransfer.effectAllowed = "move";
    try {
      e.dataTransfer.setData("text/plain", id);
    } catch {
      /* Firefox sometimes throws; ignore. */
    }
  };

  const handleDragOver = (overId: string) => (e: ReactDragEvent<HTMLLIElement>) => {
    if (!dragId || dragId === overId) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "move";
    if (dropTargetId !== overId) setDropTargetId(overId);
  };

  const handleDrop = (overId: string) => (e: ReactDragEvent<HTMLLIElement>) => {
    if (!dragId || dragId === overId) return;
    e.preventDefault();
    const from = localOrder.indexOf(dragId);
    const to = localOrder.indexOf(overId);
    if (from < 0 || to < 0) return;
    const next = [...localOrder];
    next.splice(from, 1);
    next.splice(to, 0, dragId);
    commitReorder(next);
    setDragId(null);
    setDropTargetId(null);
  };

  const handleDragEnd = () => {
    setDragId(null);
    setDropTargetId(null);
  };

  const handleKey = (row: ProviderSidebarRow) => (e: ReactKeyboardEvent<HTMLLIElement>) => {
    if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
      e.preventDefault();
      moveId(row.id, e.key === "ArrowUp" ? -1 : 1);
      return;
    }
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      onSelect(row.id);
    }
  };

  const sidebarRef = useRef<HTMLUListElement>(null);

  // Explicit wheel handler — WebView2 on Windows can swallow wheel events
  // when parent containers have overflow:hidden/clip. This ensures the
  // sidebar always scrolls in response to wheel input.
  const handleWheel = (e: React.WheelEvent<HTMLUListElement>) => {
    const el = sidebarRef.current;
    if (!el) return;
    el.scrollTop += e.deltaY;
    e.stopPropagation();
  };

  return (
    <div className="providers-sidebar-shell">
      <div className="providers-sidebar-search">
        <input
          className="providers-sidebar-search__input"
          type="search"
          value={searchText}
          onChange={(e) => onSearchTextChange(e.target.value)}
          placeholder={t("ProviderSidebarSearch")}
          aria-label={t("ProviderSidebarSearch")}
          spellCheck={false}
        />
        {searchText.trim() && (
          <button
            type="button"
            className="providers-sidebar-search__clear"
            onClick={() => onSearchTextChange("")}
            aria-label={t("ProviderSidebarClearSearch")}
            title={t("ProviderSidebarClearSearch")}
          >
            ×
          </button>
        )}
      </div>
      <ul
        ref={sidebarRef}
        className="providers-sidebar"
        role="listbox"
        aria-label="Providers"
        aria-orientation="vertical"
        onWheel={handleWheel}
      >
        {ordered.length === 0 && (
          <li className="providers-sidebar__empty">
            {t("ProviderSidebarNoMatches")}
          </li>
        )}
        {ordered.map((p, index) => {
          const isSelected = p.id === selectedId;
          const isDrop = dropTargetId === p.id;
          const isDragging = dragId === p.id;
          const reveal = justMounted.has(p.id);
          const brand = getProviderIcon(p.id).brandColor;
          const canMoveUp = index > 0 && !disabled;
          const canMoveDown = index < ordered.length - 1 && !disabled;
          const cls = [
            "providers-sidebar__row",
            isSelected && "providers-sidebar__row--selected",
            !p.enabled && "providers-sidebar__row--disabled",
            isDragging && "providers-sidebar__row--dragging",
            isDrop && "providers-sidebar__row--drop",
            reveal && "providers-sidebar__row--reveal",
          ]
            .filter(Boolean)
            .join(" ");

          const rowStyle: CSSProperties = {
            ["--provider-brand" as string]: brand,
          };

          return (
            <li
              key={p.id}
              className={cls}
              role="option"
              tabIndex={isSelected ? 0 : -1}
              aria-selected={isSelected}
              draggable={!disabled}
              style={rowStyle}
              onClick={() => onSelect(p.id)}
              onKeyDown={handleKey(p)}
              onDragStart={handleDragStart(p.id)}
              onDragOver={handleDragOver(p.id)}
              onDrop={handleDrop(p.id)}
              onDragEnd={handleDragEnd}
            >
              <span
                className={`providers-sidebar__status providers-sidebar__status--${p.status}`}
                title={t(STATUS_TO_KEY[p.status])}
                aria-label={t(STATUS_TO_KEY[p.status])}
              />
              <ProviderIcon providerId={p.id} size={24} />
              <div className="providers-sidebar__text">
                <span className="providers-sidebar__name">{p.displayName}</span>
                <span className="providers-sidebar__subtitle">
                  <span className="providers-sidebar__subtitle-primary">
                    {p.subtitlePrimary}
                  </span>
                  {p.subtitleSecondary && (
                    <span className="providers-sidebar__subtitle-secondary">
                      {p.subtitleSecondary}
                    </span>
                  )}
                </span>
              </div>
              <span
                className="providers-sidebar__handle"
                aria-hidden="true"
                title={t("ProviderSidebarReorderHint")}
              >
                ⋮⋮
              </span>
              <span className="providers-sidebar__reorder-controls">
                <button
                  type="button"
                  className="providers-sidebar__reorder-button"
                  disabled={!canMoveUp}
                  onClick={(e) => {
                    e.stopPropagation();
                    moveId(p.id, -1);
                  }}
                  aria-label={`${t("ProviderSidebarMoveUp")} ${p.displayName}`}
                  title={`${t("ProviderSidebarMoveUp")} ${p.displayName}`}
                >
                  ↑
                </button>
                <button
                  type="button"
                  className="providers-sidebar__reorder-button"
                  disabled={!canMoveDown}
                  onClick={(e) => {
                    e.stopPropagation();
                    moveId(p.id, 1);
                  }}
                  aria-label={`${t("ProviderSidebarMoveDown")} ${p.displayName}`}
                  title={`${t("ProviderSidebarMoveDown")} ${p.displayName}`}
                >
                  ↓
                </button>
              </span>
              <input
                type="checkbox"
                className="providers-sidebar__checkbox"
                checked={p.enabled}
                disabled={disabled}
                onClick={(e) => e.stopPropagation()}
                onChange={(e) => onToggleEnabled(p.id, e.target.checked)}
                aria-label={`${p.displayName} enabled`}
              />
            </li>
          );
          })}
      </ul>
    </div>
  );
}
