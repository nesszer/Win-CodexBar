import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrencyRates, getSettingsSnapshot } from "../lib/tauri";
import { FALLBACK_CURRENCY_RATES, formatDisplayCurrency, mergeValidCurrencyRates, normalizePreferredCurrency } from "../lib/currency";
import type { SettingsSnapshot } from "../types/bridge";

interface CurrencyContextValue {
  preferredCode: string;
  rates: Record<string, number>;
  format: (amount: number | null | undefined, sourceCode: string, sourceSymbol?: string | null) => string;
}

const CurrencyContext = createContext<CurrencyContextValue>({
  preferredCode: "AUTO",
  rates: FALLBACK_CURRENCY_RATES,
  format: (amount, sourceCode, sourceSymbol) => formatDisplayCurrency(amount, sourceCode, "AUTO", FALLBACK_CURRENCY_RATES, sourceSymbol),
});

export function CurrencyProvider({ children }: { children: ReactNode }) {
  const [preferredCode, setPreferredCode] = useState("AUTO");
  const [rates, setRates] = useState(FALLBACK_CURRENCY_RATES);
  const requestId = useRef(0);

  const applySettings = useCallback((settings: SettingsSnapshot) => {
    const selected = normalizePreferredCurrency(settings.preferredCurrencyCode);
    setPreferredCode(selected);
    if (selected === "AUTO") {
      requestId.current += 1;
      return;
    }
    const id = ++requestId.current;
    void getCurrencyRates(selected)
      .then((snapshot) => {
        if (requestId.current === id) setRates(mergeValidCurrencyRates(snapshot.rates));
      })
      .catch(() => {
        // Keep the offline fallback; exchange-rate availability never blocks app surfaces.
      });
  }, []);

  useEffect(() => {
    let active = true;
    void getSettingsSnapshot().then((settings) => { if (active) applySettings(settings); }).catch(() => {});
    const onUpdated = (event: Event) => {
      const settings = (event as CustomEvent<SettingsSnapshot>).detail;
      if (settings) applySettings(settings);
    };
    window.addEventListener("codexbar:settings-updated", onUpdated);
    let unlisten: (() => void) | undefined;
    void listen("settings-changed", () => {
      void getSettingsSnapshot().then((settings) => { if (active) applySettings(settings); }).catch(() => {});
    }).then((stop) => { if (active) unlisten = stop; else stop(); }).catch(() => {});
    return () => {
      active = false;
      requestId.current += 1;
      window.removeEventListener("codexbar:settings-updated", onUpdated);
      unlisten?.();
    };
  }, [applySettings]);

  const format = useCallback(
    (amount: number | null | undefined, sourceCode: string, sourceSymbol?: string | null) =>
      formatDisplayCurrency(amount, sourceCode, preferredCode, rates, sourceSymbol),
    [preferredCode, rates],
  );

  return <CurrencyContext.Provider value={{ preferredCode, rates, format }}>{children}</CurrencyContext.Provider>;
}

export function useCurrency(): CurrencyContextValue {
  return useContext(CurrencyContext);
}
