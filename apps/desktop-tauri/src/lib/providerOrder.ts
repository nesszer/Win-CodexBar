import type { ProviderCatalogEntry, ProviderUsageSnapshot } from "../types/bridge";

export function orderProviderSnapshots(
  providers: ProviderUsageSnapshot[],
  catalog: ProviderCatalogEntry[],
  enabledProviderIds: string[],
  providerOrder: string[] = [],
): ProviderUsageSnapshot[] {
  const order = new Map<string, number>();
  const orderedIds = providerOrder.length > 0
    ? providerOrder
    : catalog.map((provider) => provider.id);
  for (const [index, providerId] of orderedIds.entries()) {
    order.set(providerId, index);
  }
  for (const [index, providerId] of enabledProviderIds.entries()) {
    if (!order.has(providerId)) {
      order.set(providerId, orderedIds.length + index);
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
