import { describe, expect, it } from "vitest";
import {
  FALLBACK_CURRENCY_RATES,
  convertCurrencyAmount,
  formatDisplayCurrency,
  mergeValidCurrencyRates,
  normalizePreferredCurrency,
  sumDisplayCurrencyAmounts,
} from "./currency";

describe("preferred currency display", () => {
  it("normalizes supported preferences and falls back safely for unknown codes", () => {
    expect(normalizePreferredCurrency(undefined)).toBe("AUTO");
    expect(normalizePreferredCurrency(" try ")).toBe("TRY");
    expect(normalizePreferredCurrency("BTC")).toBe("AUTO");
  });

  it("converts both currencies through the USD pivot and rounds for display", () => {
    expect(convertCurrencyAmount(10, "USD", "TRY", FALLBACK_CURRENCY_RATES)).toBe(485);
    expect(convertCurrencyAmount(10, "GBP", "TRY", FALLBACK_CURRENCY_RATES)).toBeCloseTo(613.92405, 4);
    const display = formatDisplayCurrency(10, "USD", "TRY", FALLBACK_CURRENCY_RATES);
    expect(display).not.toContain("10.00");
    expect(display).toMatch(/485/);
  });

  it("keeps AUTO, credits, unknown units, and missing-rate values in source units", () => {
    expect(formatDisplayCurrency(4.25, "USD", "AUTO", FALLBACK_CURRENCY_RATES)).toMatch(/4\.25/);
    expect(formatDisplayCurrency(4.25, "Credits", "TRY", FALLBACK_CURRENCY_RATES)).toBe("4.25 Credits");
    expect(formatDisplayCurrency(10, "USD", "TRY", {} , "$" )).toBe("$10.00");
    expect(convertCurrencyAmount(8, "Quota", "TRY", FALLBACK_CURRENCY_RATES)).toBeNull();
  });

  it("rejects malformed exchange rates and preserves offline fallbacks", () => {
    const rates = mergeValidCurrencyRates({ USD: 1, TRY: Number.NaN, EUR: -2, BTC: 90 });
    expect(rates.TRY).toBe(48.5);
    expect(rates.EUR).toBe(0.92);
    expect(rates.BTC).toBeUndefined();
  });

  it("sums only converted overview rows and reports incomplete coverage", () => {
    const result = sumDisplayCurrencyAmounts([
      { amount: 10, currency: "USD" },
      { amount: 10, currency: "EUR" },
      { amount: 5, currency: "Credits" },
    ], "TRY", FALLBACK_CURRENCY_RATES);
    expect(result.included).toBe(2);
    expect(result.considered).toBe(3);
    expect(result.total).toBeCloseTo(10 * 48.5 + (10 / 0.92) * 48.5, 8);
  });
});
