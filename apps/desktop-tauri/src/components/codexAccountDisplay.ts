import type { CodexAccount } from "../types/bridge";

/**
 * Return the same account labels in Settings and the tray menu. The backend
 * normally supplies the canonical SHA-256-derived labels; the deterministic
 * opaque-id fallback keeps older/test bridges collision-safe too.
 */
export function buildCodexAccountDisplayNames(
  accounts: readonly CodexAccount[],
  canonical: Readonly<Record<string, string>> = {},
): Record<string, string> {
  const groups = new Map<string, CodexAccount[]>();
  for (const account of accounts) {
    const base = codexAccountBaseName(account);
    const key = base.trim().toLowerCase();
    const group = groups.get(key) ?? [];
    group.push(account);
    groups.set(key, group);
  }

  const result: Record<string, string> = {};
  for (const account of accounts) {
    const supplied = canonical[account.id]?.trim();
    if (supplied) {
      result[account.id] = supplied;
      continue;
    }

    const base = codexAccountBaseName(account);
    const group = groups.get(base.trim().toLowerCase()) ?? [];
    result[account.id] =
      group.length > 1
        ? `${base} · ${opaqueAccountSuffix(account.id)}`
        : base;
  }
  return result;
}

/**
 * Project the labels rendered by account rows in the tray and Settings.
 * The redaction rule mirrors `CodexAccount::privacy_safe_display_name` in the
 * Rust model: privacy mode redacts only the ambient/System account; managed
 * account labels keep the canonical display-name projection. Client-side
 * projection keeps the relabel reactive when the setting toggles without a
 * refetch.
 */
export function buildCodexAccountSurfaceLabels(
  accounts: readonly CodexAccount[],
  canonical: Readonly<Record<string, string>>,
  accountOrdinals: Readonly<Record<string, number>>,
  hidePersonalInfo: boolean,
  accountWord: string,
): Record<string, string> {
  const displayNames = buildCodexAccountDisplayNames(accounts, canonical);

  return Object.fromEntries(
    accounts.map((account) => {
      if (hidePersonalInfo && account.source === "ambient") {
        return [account.id, `${accountWord.trim()} ${accountOrdinals[account.id]}`];
      }
      return [account.id, displayNames[account.id]];
    }),
  );
}

export function codexAccountBaseName(account: CodexAccount): string {
  const nickname = account.nickname?.trim();
  const email = account.emailHint?.trim().toLowerCase();
  if (email && nickname) return `${email} — ${nickname}`;
  return nickname || email || "Workspace";
}

function opaqueAccountSuffix(id: string): string {
  // Production account ids are app-owned UUIDs. FNV-1a is only a fallback
  // when the Rust bridge did not provide its SHA-256 label; no provider id,
  // email, or filesystem path is exposed by this path.
  let hash = 0x811c9dc5;
  for (let index = 0; index < id.length; index += 1) {
    hash ^= id.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}
