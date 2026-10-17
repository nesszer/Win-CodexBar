import { useEffect, useState } from "react";
import type { LocaleKey } from "../../../../../i18n/keys";
import type { SettingsUpdate } from "../../../../../types/bridge";
import { Field } from "../../../../../components/FormControls";

interface Props {
  /** Saved `claudeSwapExecutablePath`. */
  value: string;
  disabled: boolean;
  t: (key: LocaleKey) => string;
  onSettingsChange: (patch: SettingsUpdate) => void;
}

/**
 * claude-swap executable path (macOS inline field of the claude-swap toggle),
 * shown directly under "Read accounts from claude-swap" only while it is on. Saves on blur or Enter. A failed save
 * leaves the saved value unchanged, so the same draft stays retryable.
 */
export function ClaudeSwapPathField({ value, disabled, t, onSettingsChange }: Props) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);

  const save = () => {
    const next = draft.trim();
    if (next === value.trim()) return;
    onSettingsChange({ claudeSwapExecutablePath: next });
  };

  return (
    <Field
      label={t("ClaudeSwapExecutablePath")}
      description={t("ClaudeSwapExecutablePathHelp")}
    >
      <input
        type="text"
        className="text-input"
        value={draft}
        disabled={disabled}
        spellCheck={false}
        aria-label={t("ClaudeSwapExecutablePath")}
        placeholder={t("ClaudeSwapExecutablePathPlaceholder")}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={save}
        onKeyDown={(e) => {
          if (e.key === "Enter") save();
        }}
      />
    </Field>
  );
}
