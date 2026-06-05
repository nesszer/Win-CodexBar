import type { ProviderCatalogEntry, ProviderUsageSnapshot } from "../types/bridge";

export function orderProviderSnapshots(
  providers: ProviderUsageSnapshot[],
  catalog: ProviderCatalogEntry[],
  enabledProviderIds: string[],
): ProviderUsageSnapshot[] {
  const order = new Map<string, number>();
  for (const [index, provider] of catalog.entries()) {
    order.set(provider.id, index);
  }
  for (const [index, providerId] of enabledProviderIds.entries()) {
    if (!order.has(providerId)) {
      order.set(providerId, catalog.length + index);
    }
  }

  return [...providers].sort((a, b) => {
    const aOrder = order.get(a.providerId);
    const bOrder = order.get(b.providerId);
    if (aOrder != null && bOrder != null && aOrder !== bOrder) return aOrder - bOrder;
    if (aOrder != null) return -1;
    if (bOrder != null) return 1;
    return a.displayName.localeCompare(b.displayName);
  });
}
