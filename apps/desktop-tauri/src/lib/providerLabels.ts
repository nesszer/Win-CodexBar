import type { LocaleKey } from "../i18n/keys";
import { localizeProviderText } from "./providerText";

type Translate = (key: LocaleKey) => string;

const COST_PERIOD_KEYS: ReadonlyMap<string, LocaleKey> = new Map<string, LocaleKey>([
  ["atlascloud:Atlas Cloud balance", "AtlasCloudBalance"],
]);

export function providerCostPeriodTitle(
  providerId: string,
  period: string,
  t: Translate,
): string {
  const key = COST_PERIOD_KEYS.get(`${providerId}:${period}`);
  return key ? t(key) : localizeProviderText(period, t);
}
