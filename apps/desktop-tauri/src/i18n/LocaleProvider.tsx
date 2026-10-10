import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { getLocaleStrings, setUiLanguage } from "../lib/tauri";
import { useTauriEvent } from "../hooks/useTauriEvent";
import type {
  Language,
  LocaleChangedPayload,
  LocaleStrings,
} from "../types/bridge";
import { ALL_LOCALE_KEYS, type LocaleKey } from "./keys";

interface LocaleContextValue {
  /** Translate a key using the active language. Returns the key name as a
   *  safe fallback if the backend did not supply a string (should only
   *  happen during a transient refetch). */
  t: (key: LocaleKey) => string;
  language: Language;
  setLanguage: (language: Language) => Promise<void>;
  /** Force a refetch (e.g. after an external settings change). */
  reload: () => Promise<void>;
}

const LocaleContext = createContext<LocaleContextValue | null>(null);

function validateBundle(bundle: LocaleStrings): void {
  if (!import.meta.env.DEV) return;
  for (const key of ALL_LOCALE_KEYS) {
    if (!(key in bundle.entries)) {
      // Warn once — tests in Rust assert completeness, but flag mismatches
      // loudly during development if the TS list drifts from the enum.
      // eslint-disable-next-line no-console
      console.warn(`[locale] missing key from bridge response: ${key}`);
    }
  }
}

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [bundle, setBundle] = useState<LocaleStrings | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async (language?: Language | null) => {
    const next = await getLocaleStrings(language ?? null);
    validateBundle(next);
    setBundle(next);
  }, []);

  useEffect(() => {
    let cancelled = false;

    load(null).catch((cause: unknown) => {
      if (!cancelled) {
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    });
    return () => {
      cancelled = true;
    };
  }, [load]);

  useTauriEvent<LocaleChangedPayload>("locale-changed", (event) => {
    load(event.payload).catch(() => {
      // Best-effort refetch — the next manual reload will retry.
    });
  }, [load]);

  const setLanguage = useCallback(
    async (language: Language) => {
      await setUiLanguage(language);
      // The backend emits `locale-changed`, but also refetch eagerly in
      // case we beat the listener registration.
      await load(language);
    },
    [load],
  );

  const reload = useCallback(() => load(null), [load]);

  const value = useMemo<LocaleContextValue | null>(() => {
    if (!bundle) return null;
    const entries = bundle.entries;
    return {
      t: (key: LocaleKey) => entries[key] ?? key,
      language: bundle.language,
      setLanguage,
      reload,
    };
  }, [bundle, setLanguage, reload]);

  if (error) {
    return (
      <main className="shell">
        <section className="panel error">
          <h2>Locale load failed</h2>
          <p>{error}</p>
        </section>
      </main>
    );
  }

  if (!value) {
    // Suspend first paint until the locale bundle has arrived so that
    // every rendered string is already localized.
    return null;
  }

  return (
    <LocaleContext.Provider value={value}>{children}</LocaleContext.Provider>
  );
}

export function useLocale(): LocaleContextValue {
  const ctx = useContext(LocaleContext);
  if (!ctx) {
    throw new Error("useLocale() must be used inside <LocaleProvider />");
  }
  return ctx;
}
