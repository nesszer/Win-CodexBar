import { readFileSync } from "node:fs";
import { ALL_LOCALE_KEYS, type LocaleKey } from "../i18n/keys";
import type { Language, LocaleStrings } from "../types/bridge";

/** Translator backed by the shipped Fluent file, with `{ "{}" }` unescaped. */
export function fromFtl(file: string): (key: LocaleKey) => string {
  const text = readFileSync(`${import.meta.dirname}/../../../../rust/src/locale/${file}`, "utf8");
  const table = new Map<string, string>();
  for (const line of text.split(/\r?\n/)) {
    const m = /^([A-Za-z][\w-]*)\s*=\s?(.*)$/.exec(line);
    if (m) table.set(m[1], m[2].replace(/\{ "\{\}" \}/g, "{}"));
  }
  return (key) => table.get(key) ?? key;
}

/**
 * Build a complete `LocaleStrings` bundle whose entries are the key names
 * themselves, so tests can assert on "the translator returned *something*
 * that corresponds to this key" without caring about EN/ZH wording.
 */
export function buildBundle(
  overrides: Partial<Record<(typeof ALL_LOCALE_KEYS)[number], string>> = {},
  language: Language = "english",
): LocaleStrings {
  const entries: Record<string, string> = {};
  for (const key of ALL_LOCALE_KEYS) {
    entries[key] = overrides[key] ?? key;
  }
  return { language, entries };
}
