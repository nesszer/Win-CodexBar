import { CURRENCY_CATALOG } from "./currencyCatalog.generated";

// The catalog (order, symbols, offline rates) is generated from rust/src/currency.rs.
export const SUPPORTED_CURRENCIES: readonly string[] = CURRENCY_CATALOG.map((entry) => entry.code);

export const FALLBACK_CURRENCY_RATES: Record<string, number> = Object.fromEntries(
  CURRENCY_CATALOG.map((entry) => [entry.code, entry.fallbackRate]),
);

export const CURRENCY_PICKER_OPTIONS: ReadonlyArray<{ value: string; label: string }> =
  CURRENCY_CATALOG.map((entry) => ({ value: entry.code, label: `${entry.code} (${entry.symbol})` }));

export function normalizePreferredCurrency(value: string | null | undefined): string {
  const code = value?.trim().toUpperCase() || "AUTO";
  return code === "AUTO" || SUPPORTED_CURRENCIES.includes(code) ? code : "AUTO";
}

export function convertCurrencyAmount(
  amount: number,
  sourceCode: string,
  targetCode: string,
  rates: Record<string, number>,
): number | null {
  if (!Number.isFinite(amount)) return null;
  const source = sourceCode.trim().toUpperCase();
  const target = targetCode.trim().toUpperCase();
  if (!SUPPORTED_CURRENCIES.includes(source) || !SUPPORTED_CURRENCIES.includes(target)) return null;
  if (source === target) return amount;
  const sourceRate = source === "USD" ? 1 : rates[source];
  const targetRate = target === "USD" ? 1 : rates[target];
  if (!Number.isFinite(sourceRate) || sourceRate <= 0 || !Number.isFinite(targetRate) || targetRate <= 0) return null;
  const result = (amount / sourceRate) * targetRate;
  return Number.isFinite(result) ? result : null;
}

function formatOriginal(amount: number, code: string, symbol?: string | null): string {
  if (symbol) return `${symbol}${amount.toFixed(2)}`;
  if (!/^[A-Z]{3}$/.test(code)) return `${amount.toFixed(2)} ${code}`;
  try {
    return new Intl.NumberFormat("en-US", { style: "currency", currency: code }).format(amount);
  } catch {
    return `${amount.toFixed(2)} ${code}`;
  }
}

export function formatDisplayCurrency(
  amount: number | null | undefined,
  sourceCode: string,
  preferredCode: string,
  rates: Record<string, number>,
  sourceSymbol?: string | null,
): string {
  if (amount == null || !Number.isFinite(amount)) return "—";
  const trimmedSource = sourceCode.trim();
  const source = trimmedSource.toUpperCase();
  const sourceLabel = /^[A-Za-z]{3}$/.test(trimmedSource) ? source : trimmedSource;
  const preferred = normalizePreferredCurrency(preferredCode);
  if (preferred === "AUTO") return formatOriginal(amount, sourceLabel, sourceSymbol);
  const converted = convertCurrencyAmount(amount, source, preferred, rates);
  if (converted == null) return formatOriginal(amount, sourceLabel, sourceSymbol);
  try {
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: preferred,
    }).format(converted);
  } catch {
    return `${converted.toFixed(2)} ${preferred}`;
  }
}

export function sumDisplayCurrencyAmounts(
  rows: Array<{ amount: number | null | undefined; currency: string }>,
  preferredCode: string,
  rates: Record<string, number>,
): { total: number | null; included: number; considered: number } {
  const target = normalizePreferredCurrency(preferredCode);
  let total = 0;
  let included = 0;
  for (const row of rows) {
    if (row.amount == null || !Number.isFinite(row.amount)) continue;
    let amount: number | null;
    if (target === "AUTO") {
      amount = row.currency.trim().toUpperCase() === "USD" ? row.amount : null;
    } else {
      amount = convertCurrencyAmount(row.amount, row.currency || "USD", target, rates);
    }
    if (amount == null) continue;
    total += amount;
    included += 1;
  }
  return { total: included > 0 && Number.isFinite(total) ? total : null, included, considered: rows.length };
}
