import { useMemo, useState, type CSSProperties } from "react";
import type { ProviderUsageSnapshot } from "../types/bridge";
import { ProviderIcon } from "./providers/ProviderIcon";
import { getProviderIcon } from "./providers/providerIcons";

export default function ProviderGrid({
  providers,
  selectedProviderId,
  showAsUsed,
  expanded,
  onExpandedChange,
  onSelect,
}: {
  providers: ProviderUsageSnapshot[];
  selectedProviderId: string | null;
  showAsUsed: boolean;
  expanded?: boolean;
  onExpandedChange?: (expanded: boolean) => void;
  onSelect: (providerId: string | null) => void;
}) {
  const [uncontrolledExpanded, setUncontrolledExpanded] = useState(false);
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
      className={`provider-grid${densityClass}`}
      data-provider-count={totalItems}
      data-expanded={isExpanded ? "true" : "false"}
    >
      <button
        type="button"
        className={`provider-grid__item${selectedProviderId === null ? " provider-grid__item--active" : ""}`}
        onClick={() => onSelect(null)}
        aria-label="All providers"
      >
        <span className="provider-grid__icon-overview">⊞</span>
        <span className="provider-grid__label">All</span>
      </button>
      {visibleProviders.map((p) => (
        <button
          key={p.providerId}
          type="button"
          className={`provider-grid__item${p.providerId === selectedProviderId ? " provider-grid__item--active" : ""}`}
          onClick={() => onSelect(p.providerId)}
          aria-label={p.displayName}
        >
          <ProviderIcon providerId={p.providerId} size={16} />
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
          <span className="provider-grid__icon-overview" aria-hidden>
            {isExpanded ? "−" : "+"}
          </span>
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
