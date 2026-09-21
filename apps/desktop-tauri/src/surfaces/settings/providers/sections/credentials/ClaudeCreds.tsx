import { useEffect, useState } from "react";
import type { LocaleKey } from "../../../../../i18n/keys";
import { getSettingsSnapshot, updateSettings } from "../../../../../lib/tauri";

interface Props {
  t: (key: LocaleKey) => string;
}

/**
 * Claude-specific credential/options.
 *
 * Port of the `ProviderId::Claude` branch of the "Options" block in
 * `rust/src/native_ui/preferences.rs::render_provider_detail_panel`.
 * Exposes "Avoid keychain prompts" and "Allow reading Claude Code's
 * credentials" (OAuth consent gate added in 76d3f010 — this toggle is the UI
 * surface for that setting). Usage-row visibility is handled by the shared
 * provider detail visibility section.
 * The broader `disable_keychain_access` master switch lives in Advanced.
 */
export function ClaudeCreds({ t }: Props) {
  const [avoidKeychain, setAvoidKeychain] = useState<boolean | null>(null);
  const [allowReadingClaudeCodeCredentials, setAllowReadingClaudeCodeCredentials] =
    useState<boolean | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let cancelled = false;
    getSettingsSnapshot()
      .then((s) => {
        if (cancelled) return;
        setAvoidKeychain(s.claudeAvoidKeychainPrompts);
        setAllowReadingClaudeCodeCredentials(
          s.claudeAllowReadingClaudeCodeCredentials ?? false,
        );
      })
      .catch((e) => !cancelled && setError(String(e)));
    return () => {
      cancelled = true;
    };
  }, []);

  const toggleAvoidKeychain = async (next: boolean) => {
    setSaving(true);
    try {
      const updated = await updateSettings({
        claudeAvoidKeychainPrompts: next,
      });
      setAvoidKeychain(updated.claudeAvoidKeychainPrompts);
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const toggleAllowReadingClaudeCodeCredentials = async (next: boolean) => {
    setSaving(true);
    try {
      const updated = await updateSettings({
        claudeAllowReadingClaudeCodeCredentials: next,
      });
      setAllowReadingClaudeCodeCredentials(
        updated.claudeAllowReadingClaudeCodeCredentials ?? next,
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  if (
    avoidKeychain === null ||
    allowReadingClaudeCodeCredentials === null
  )
    return null;

  return (
    <section className="provider-detail-section">
      <h4>{t("CredentialsSectionTitle")}</h4>
      <label className="provider-detail-toggle">
        <input
          type="checkbox"
          checked={avoidKeychain}
          disabled={saving}
          onChange={(e) => void toggleAvoidKeychain(e.target.checked)}
        />
        <span>
          <span className="provider-detail-toggle__label">
            {t("ProviderClaudeAvoidKeychainPrompts")}
          </span>
          <span className="provider-detail-toggle__helper">
            {t("ProviderClaudeAvoidKeychainPromptsHelp")}
          </span>
        </span>
      </label>
      <label className="provider-detail-toggle">
        <input
          type="checkbox"
          checked={allowReadingClaudeCodeCredentials}
          disabled={saving}
          onChange={(e) =>
            void toggleAllowReadingClaudeCodeCredentials(e.target.checked)
          }
        />
        <span>
          <span className="provider-detail-toggle__label">
            {t("ProviderClaudeAllowReadingClaudeCodeCredentials")}
          </span>
          <span className="provider-detail-toggle__helper">
            {t("ProviderClaudeAllowReadingClaudeCodeCredentialsHelp")}
          </span>
        </span>
      </label>
      {error && <div className="provider-detail-error">{error}</div>}
    </section>
  );
}
