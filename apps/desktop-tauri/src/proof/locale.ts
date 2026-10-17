import englishFtl from "../../../../rust/src/locale/en-US.ftl?raw";
import { ALL_LOCALE_KEYS } from "../i18n/keys";
import type { LanguageOption, LocaleStrings } from "../types/bridge";

/** Parse the shipped English Fluent file the way the Rust bridge flattens it. */
export function parseFtlEntries(text: string): Record<string, string> {
  const entries: Record<string, string> = {};
  for (const line of text.split(/\r?\n/)) {
    const match = /^([A-Za-z][\w-]*)\s*=\s?(.*)$/.exec(line);
    if (match) entries[match[1]] = match[2].replace(/\{ "\{\}" \}/g, "{}");
  }
  return entries;
}

let cached: LocaleStrings | null = null;

/** English bundle; keys missing from the file fall back to the key name, as the UI does. */
export function englishLocaleStrings(): LocaleStrings {
  if (cached) return cached;
  const parsed = parseFtlEntries(englishFtl);
  const entries: Record<string, string> = {};
  for (const key of ALL_LOCALE_KEYS) {
    entries[key] = parsed[key] ?? key;
  }
  cached = { language: "english", entries: { ...parsed, ...entries } };
  return cached;
}

export const PROOF_LANGUAGES: LanguageOption[] = [
  { value: "english", display: "English" },
];
