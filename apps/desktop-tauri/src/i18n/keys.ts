import { CORE_LOCALE_KEYS } from "./keyGroups/core";
import { SHELL_LOCALE_KEYS } from "./keyGroups/shell";
import { PROVIDER_TEXT_LOCALE_KEYS } from "./keyGroups/providerText";

// Generated from rust/src/locale.rs LocaleKey enum.
// Keep in sync with rust/src/locale.rs — the Rust IPC `get_locale_strings`
// returns its entries map keyed by these exact variant names. The runtime
// LocaleProvider asserts every key in this list is present in the bridge
// response so a mismatch fails loudly in development.

// Topic arrays spread in the original key order.
export const ALL_LOCALE_KEYS = [
  ...CORE_LOCALE_KEYS,
  ...SHELL_LOCALE_KEYS,
  ...PROVIDER_TEXT_LOCALE_KEYS,
] as const;

export type LocaleKey = (typeof ALL_LOCALE_KEYS)[number];
