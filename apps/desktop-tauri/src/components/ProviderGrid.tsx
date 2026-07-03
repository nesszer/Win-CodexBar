import { useMemo, useState, type CSSProperties } from "react";
import type { ProviderUsageSnapshot } from "../types/bridge";
import { ProviderIcon } from "./providers/ProviderIcon";
import { getProviderIcon } from "./providers/providerIcons";

export default function ProviderGrid({
  providers,
  selectedProviderId,
  showAsUsed,
  showProviderIcons = true,
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
  const [uncontrolledExpanded, setUncontrolledExpanded] = useState(false);
  const [dragId, setDragId] = useState<string | null>(null);
  const [overId, setOverId] = useState<string | null>(null);
  const canReorder = typeof onReorder === "function";

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
  const gridPercent = (provider: ProviderUsageSnapshot) => {
    const pct = showAsUsed
      ? provider.primary.usedPercent
      : provider.primary.remainingPercent;
    return Math.max(0, Math.min(100, pct));
  };
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
  const labelFor = (name: string) =>
    densityClass.includes("compact") ? compactGridLabel(name) : name;

  return (
    <div
      className={`provider-grid${densityClass}${showProviderIcons ? "" : " provider-grid--no-icons"}`}
      data-provider-count={totalItems}
      data-expanded={isExpanded ? "true" : "false"}
      data-show-icons={showProviderIcons ? "true" : "false"}
    >
      <button
        type="button"
        className={`provider-grid__item${selectedProviderId === null ? " provider-grid__item--active" : ""}`}
        onClick={() => onSelect(null)}
        aria-label="All providers"
      >
        {showProviderIcons && <span className="provider-grid__icon-overview">⊞</span>}
        <span className="provider-grid__label">All</span>
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
          aria-label={isExpanded ? "Show fewer providers" : "Show all providers"}
          aria-expanded={isExpanded}
        >
          {showProviderIcons && (
            <span className="provider-grid__icon-overview" aria-hidden>
              {isExpanded ? "−" : "+"}
            </span>
          )}
          <span className="provider-grid__label">
            {isExpanded ? "Less" : `+${hiddenCount}`}
          </span>
        </button>
      )}
    </div>
  );
}

export function prioritizeProviders(
  providers: ProviderUsageSnapshot[],
  selectedProviderId: string | null,
): ProviderUsageSnapshot[] {
  if (!selectedProviderId) return providers;
  const selectedIndex = providers.findIndex((provider) => provider.providerId === selectedProviderId);
  if (selectedIndex < 0 || selectedIndex < 18) return providers;
  const selected = providers[selectedIndex];
  return [selected, ...providers.slice(0, selectedIndex), ...providers.slice(selectedIndex + 1)];
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
