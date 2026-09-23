export const SUPPORTED_CURRENCIES = [
  "USD", "GBP", "EUR", "CZK", "CNY", "JPY", "KRW", "CAD", "AUD", "HKD", "TWD", "SGD",
  "INR", "CHF", "AED", "TRY",
] as const;

export const FALLBACK_CURRENCY_RATES: Record<string, number> = {
  USD: 1,
  GBP: 0.79,
  EUR: 0.92,
  CZK: 21,
  CNY: 7.27,
  JPY: 154,
  KRW: 1428.9,
  CAD: 1.38,
  AUD: 1.55,
  HKD: 7.8,
  TWD: 32.3,
  SGD: 1.34,
  INR: 84.5,
  CHF: 0.8,
  AED: 3.6725,
  TRY: 48.5,
};

export function normalizePreferredCurrency(value: string | null | undefined): string {
  const code = value?.trim().toUpperCase() || "AUTO";
  return code === "AUTO" || SUPPORTED_CURRENCIES.includes(code as (typeof SUPPORTED_CURRENCIES)[number])
    ? code
    : "AUTO";
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
  if (!SUPPORTED_CURRENCIES.includes(source as (typeof SUPPORTED_CURRENCIES)[number]) ||
      !SUPPORTED_CURRENCIES.includes(target as (typeof SUPPORTED_CURRENCIES)[number])) return null;
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
      maximumFractionDigits: 2,
    }).format(converted);
  } catch {
    return `${converted.toFixed(2)} ${preferred}`;
  }
}

export function mergeValidCurrencyRates(input: Record<string, number>): Record<string, number> {
  const rates = { ...FALLBACK_CURRENCY_RATES };
  for (const code of SUPPORTED_CURRENCIES) {
    const value = input[code];
    if (Number.isFinite(value) && value > 0 && (code !== "USD" || Math.abs(value - 1) <= Number.EPSILON)) {
      rates[code] = value;
    }
  }
  return rates;
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
