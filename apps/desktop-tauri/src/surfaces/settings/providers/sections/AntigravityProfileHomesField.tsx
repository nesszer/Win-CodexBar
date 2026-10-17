import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import type { LocaleKey } from "../../../../i18n/keys";
import type { SettingsUpdate } from "../../../../types/bridge";
import { Field } from "../../../../components/FormControls";

interface Props {
  /** Saved `antigravityAdditionalProfileHomes`. */
  value: string[];
  disabled: boolean;
  t: (key: LocaleKey) => string;
  onSettingsChange: (patch: SettingsUpdate) => void;
}

/** Case-insensitive identity for a Windows folder path, matching the backend dedupe. */
function profileHomeKey(path: string): string {
  return path.trim().replace(/\//g, "\\").toLowerCase();
}

/** Appends picked folders to the saved list, dropping blanks and case-insensitive repeats. */
export function mergeProfileHomes(current: string[], picked: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const path of [...current, ...picked]) {
    const trimmed = path.trim();
    if (!trimmed) continue;
    const key = profileHomeKey(trimmed);
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(trimmed);
  }
  return out;
}

/**
 * Extra Gemini profile folders scanned for Antigravity local history (macOS
 * "additional profile homes"). Folders come from the native picker; each row
 * can be removed. A cancelled picker or a failed pick leaves the saved list unchanged.
 */
export function AntigravityProfileHomesField({
  value,
  disabled,
  t,
  onSettingsChange,
}: Props) {
  const [error, setError] = useState<string | null>(null);

  const addFolders = async () => {
    setError(null);
    try {
      const selected = await open({ directory: true, multiple: true });
      if (selected === null) return;
      const picked = Array.isArray(selected) ? selected : [selected];
      const next = mergeProfileHomes(value, picked);
      const unchanged =
        next.length === value.length && next.every((path, i) => path === value[i]);
      if (!unchanged) onSettingsChange({ antigravityAdditionalProfileHomes: next });
    } catch (err) {
      setError(`${t("AntigravityProfileHomesPickFailed")} ${String(err)}`);
    }
  };

  const removeFolder = (path: string) => {
    onSettingsChange({
      antigravityAdditionalProfileHomes: value.filter((saved) => saved !== path),
    });
  };

  return (
    <Field
      label={t("AntigravityProfileHomesLabel")}
      description={t("AntigravityProfileHomesHelp")}
    >
      <div className="antigravity-profile-homes">
        {value.length === 0 ? (
          <p>{t("AntigravityProfileHomesEmpty")}</p>
        ) : (
          <ul className="antigravity-profile-homes__list">
            {value.map((path) => (
              <li key={path} className="antigravity-profile-homes__item">
                <span className="antigravity-profile-homes__path" title={path}>
                  {path}
                </span>
                <button
                  type="button"
                  className="credential-btn"
                  disabled={disabled}
                  aria-label={`${t("AntigravityProfileHomesRemove")} ${path}`}
                  onClick={() => removeFolder(path)}
                >
                  {t("AntigravityProfileHomesRemove")}
                </button>
              </li>
            ))}
          </ul>
        )}
        <div className="settings-group__row">
          <button
            type="button"
            className="credential-btn"
            disabled={disabled}
            onClick={() => void addFolders()}
          >
            {t("AntigravityProfileHomesAdd")}
          </button>
        </div>
        {error && <p role="status">{error}</p>}
      </div>
    </Field>
  );
}
