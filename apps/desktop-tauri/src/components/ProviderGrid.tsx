import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import type { ProviderUsageSnapshot, TrayPanelStyle } from "../types/bridge";
import { ProviderIcon } from "./providers/ProviderIcon";
import { getProviderIcon } from "./providers/providerIcons";
import { useLocale } from "../hooks/useLocale";
import { prioritizeProviders, switcherWeeklyPercent } from "./providerGridUtils";

const TAB_ROW_MAX = 8;
const TAB_WRAP_MAX = 18;

export default function ProviderGrid({
  providers,
  selectedProviderId,
  showAsUsed,
  showProviderIcons = true,
  variant = "modern",
  expanded,
  onExpandedChange,
  onSelect,
  onReorder,
  onGestureStart,
  onGestureEnd,
}: {
  providers: ProviderUsageSnapshot[];
  selectedProviderId: string | null;
  showAsUsed: boolean;
  showProviderIcons?: boolean;
  /** Settings > Menu > Panel look; labelled tabs belong to the modern look. */
  variant?: TrayPanelStyle;
  expanded?: boolean;
  onExpandedChange?: (expanded: boolean) => void;
  onSelect: (providerId: string | null) => void;
  /** Persist a new provider order (list of provider IDs) after a drag-reorder. */
  onReorder?: (orderedIds: string[]) => void;
  /** Called on mousedown of a draggable item, before a possible HTML5 drag starts. */
  onGestureStart?: () => void;
  /** Called on mouseup or dragend of a draggable item (drag finished or canceled). */
  onGestureEnd?: () => void;
}) {
  const { t } = useLocale();
  const [uncontrolledExpanded, setUncontrolledExpanded] = useState(false);
  const [dragId, setDragId] = useState<string | null>(null);
  const [overId, setOverId] = useState<string | null>(null);
  const canReorder = typeof onReorder === "function";
  const gridRef = useRef<HTMLDivElement>(null);

  // Keep the selected item visible when it changes (keyboard switching in the
  // scrollable dense grid); this never expands a collapsed grid.
  useEffect(() => {
    gridRef.current
      ?.querySelector(".provider-grid__item--active")
      ?.scrollIntoView?.({ block: "nearest", inline: "nearest" });
  }, [selectedProviderId]);

  const applyReorder = (targetId: string) => {
    if (!onReorder || !dragId || dragId === targetId) return;
    const ids = providers.map((provider) => provider.providerId);
    const from = ids.indexOf(dragId);
    const to = ids.indexOf(targetId);
    if (from < 0 || to < 0) return;
    const next = ids.slice();
    next.splice(from, 1);
    next.splice(to, 0, dragId);
    onReorder(next);
  };
  const endDrag = () => {
    setDragId(null);
    setOverId(null);
  };
  const isExpanded = expanded ?? uncontrolledExpanded;
  const setExpanded = (next: boolean) => {
    if (expanded === undefined) setUncontrolledExpanded(next);
    onExpandedChange?.(next);
  };
  const gridPercent = (provider: ProviderUsageSnapshot) =>
    switcherWeeklyPercent(provider, showAsUsed);
  const totalItems = providers.length + 1;
  const shouldCollapse = totalItems > 32;
  const collapsedProviders = useMemo(
    () => prioritizeProviders(providers, selectedProviderId),
    [providers, selectedProviderId],
  );
  const visibleProviders =
    shouldCollapse && !isExpanded
      ? collapsedProviders.slice(0, 18)
      : providers;
  const hiddenCount = Math.max(0, providers.length - visibleProviders.length);
  const densityClass =
    totalItems <= 6
      ? " provider-grid--sparse"
      : shouldCollapse
        ? " provider-grid--compact"
        : "";
  // Labelled tabs while they fit: one row up to TAB_ROW_MAX items, wrapped
  // rows up to TAB_WRAP_MAX; beyond that the dense icon grid takes over.
  const tabLayout =
    variant === "classic"
      ? null
      : totalItems <= TAB_ROW_MAX ? "row" : totalItems <= TAB_WRAP_MAX ? "wrap" : null;
  const labelFor = (name: string) =>
    densityClass.includes("compact") ? compactGridLabel(name) : name;

  return (
    <div
      ref={gridRef}
      className={`provider-grid${densityClass}${tabLayout ? " provider-grid--tabs" : ""}${showProviderIcons ? "" : " provider-grid--no-icons"}`}
      data-tab-layout={tabLayout ?? undefined}
      data-provider-count={totalItems}
      data-expanded={isExpanded ? "true" : "false"}
      data-show-icons={showProviderIcons ? "true" : "false"}
    >
      <button
        type="button"
        className={`provider-grid__item${selectedProviderId === null ? " provider-grid__item--active" : ""}`}
        onClick={() => onSelect(null)}
        aria-label={t("PanelAllProviders")}
      >
        {showProviderIcons && (
          <span className="provider-grid__icon-overview" aria-hidden={variant === "modern"}>
            {variant === "modern" ? (
              <svg viewBox="0 0 16 16" fill="currentColor">
                <rect x="2" y="2" width="5" height="5" rx="1.2" />
                <rect x="9" y="2" width="5" height="5" rx="1.2" />
                <rect x="2" y="9" width="5" height="5" rx="1.2" />
                <rect x="9" y="9" width="5" height="5" rx="1.2" />
              </svg>
            ) : (
              "⊞"
            )}
          </span>
        )}
        <span className="provider-grid__label">{t("PanelAllProvidersShort")}</span>
      </button>
      {visibleProviders.map((p) => (
        <button
          key={p.providerId}
          type="button"
          className={`provider-grid__item${p.providerId === selectedProviderId ? " provider-grid__item--active" : ""}${dragId === p.providerId ? " provider-grid__item--dragging" : ""}${canReorder && overId === p.providerId && dragId && dragId !== p.providerId ? " provider-grid__item--drop-target" : ""}`}
          onClick={() => onSelect(p.providerId)}
          aria-label={p.displayName}
          draggable={canReorder}
          onMouseDown={canReorder ? () => onGestureStart?.() : undefined}
          onMouseUp={canReorder ? () => onGestureEnd?.() : undefined}
          onDragStart={
            canReorder
              ? (e) => {
                  setDragId(p.providerId);
                  e.dataTransfer.effectAllowed = "move";
                }
              : undefined
          }
          onDragOver={
            canReorder
              ? (e) => {
                  if (!dragId) return;
                  e.preventDefault();
                  e.dataTransfer.dropEffect = "move";
                  if (overId !== p.providerId) setOverId(p.providerId);
                }
              : undefined
          }
          onDrop={
            canReorder
              ? (e) => {
                  e.preventDefault();
                  applyReorder(p.providerId);
                  endDrag();
                }
              : undefined
          }
          onDragEnd={
            canReorder
              ? () => {
                  onGestureEnd?.();
                  endDrag();
                }
              : undefined
          }
        >
          {showProviderIcons && <ProviderIcon providerId={p.providerId} size={16} />}
          <span className="provider-grid__label">{labelFor(p.displayName)}</span>
          {!p.error && (
            <span
              className="provider-grid__weekly-track"
              style={{
                "--weekly-pct": `${gridPercent(p)}%`,
                "--weekly-color": getProviderIcon(p.providerId).brandColor,
              } as CSSProperties}
            />
          )}
        </button>
      ))}
      {shouldCollapse && (
        <button
          type="button"
          className="provider-grid__item provider-grid__item--more"
          onClick={() => setExpanded(!isExpanded)}
          aria-label={isExpanded ? t("PanelShowFewerProviders") : t("PanelShowAllProviders")}
          aria-expanded={isExpanded}
        >
          {showProviderIcons && (
            <span className="provider-grid__icon-overview" aria-hidden>
              {isExpanded ? "−" : "+"}
            </span>
          )}
          <span className="provider-grid__label">
            {isExpanded ? t("PanelShowFewerProviders") : `+${hiddenCount}`}
          </span>
        </button>
      )}
    </div>
  );
}

function compactGridLabel(displayName: string): string {
  const clean = displayName.replace(/[._-]+/g, " ").replace(/\s+/g, " ").trim();
  if (clean.length <= 5) return clean;

  const words = clean.split(" ").filter(Boolean);
  const first = words[0] ?? clean;
  if (words.length > 1) {
    if (first.length <= 3 && /\d|^[A-Z]+$/.test(first)) return first;
    const initials = words
      .slice(0, 2)
      .map((word) => word[0]?.toUpperCase() ?? "")
      .join("");
    if (initials.length >= 2) return initials;
  }

  const capitals = clean.match(/[A-Z0-9]/g);
  if (capitals && capitals.length >= 2 && capitals.length <= 4) {
    return capitals.join("");
  }

  return clean.slice(0, 4);
}
