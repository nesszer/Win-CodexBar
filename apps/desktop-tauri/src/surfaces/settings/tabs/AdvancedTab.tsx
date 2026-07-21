import { useCallback, useEffect, useState } from "react";
import { useLocale } from "../../../hooks/useLocale";
import {
  registerGlobalShortcut,
  unregisterGlobalShortcut,
} from "../../../lib/tauri";
import { ShortcutCapture } from "../../../components/ShortcutCapture";
import { Field, Toggle } from "../../../components/FormControls";
import type { TabProps } from "../../Settings";

function formatCodexSessionsDirs(paths: string[]): string {
  return paths.join("; ");
}

function parseCodexSessionsDirs(value: string): string[] {
  return value
    .split(/[;\n]/)
    .map((path) => path.trim())
    .filter(Boolean);
}

function parseSshHosts(value: string): string[] {
  return value.split(/[,\n]/).map((host) => host.trim()).filter(Boolean);
}

export default function AdvancedTab({ settings, set, saving }: TabProps) {
  const { t } = useLocale();
  const [shortcutError, setShortcutError] = useState<string | null>(null);
  const [codexDirsDraft, setCodexDirsDraft] = useState(() =>
    formatCodexSessionsDirs(settings.codexCustomSessionsDirs),
  );
  const [sshHostsDraft, setSshHostsDraft] = useState(() =>
    (settings.agentSessionSshHosts ?? []).join(", "),
  );

  useEffect(() => {
    if (!saving) {
      setCodexDirsDraft(formatCodexSessionsDirs(settings.codexCustomSessionsDirs));
    }
  }, [saving, settings.codexCustomSessionsDirs]);

  useEffect(() => {
    if (!saving) setSshHostsDraft((settings.agentSessionSshHosts ?? []).join(", "));
  }, [saving, settings.agentSessionSshHosts]);

  const commitShortcut = useCallback(
    async (accelerator: string) => {
      setShortcutError(null);
      try {
        await registerGlobalShortcut(accelerator).catch(() => {});
        set({ globalShortcut: accelerator });
      } catch (err: unknown) {
        setShortcutError(err instanceof Error ? err.message : String(err));
      }
    },
    [set],
  );

  const clearShortcut = useCallback(async () => {
    setShortcutError(null);
    try {
      await unregisterGlobalShortcut().catch(() => {});
      set({ globalShortcut: "" });
    } catch (err: unknown) {
      setShortcutError(err instanceof Error ? err.message : String(err));
    }
  }, [set]);

  const commitCodexDirs = useCallback(() => {
    set({ codexCustomSessionsDirs: parseCodexSessionsDirs(codexDirsDraft) });
  }, [codexDirsDraft, set]);

  return (
    <>
      {/* ── Keyboard shortcut ────────────────────────────────────── */}
      <section className="settings-section">
        <h3 className="settings-section__title">{t("SectionKeyboard")}</h3>
        <div className="settings-section__group">
          <Field
            label={t("GlobalShortcutFieldLabel")}
            description={t("GlobalShortcutToggleHelper")}
          >
            <ShortcutCapture
              value={settings.globalShortcut}
              disabled={saving}
              onCommit={(accel) => void commitShortcut(accel)}
              onClear={() => void clearShortcut()}
            />
          </Field>
        </div>
        {shortcutError && (
          <p className="settings-section__error">{shortcutError}</p>
        )}
        <p className="settings-section__hint">{t("ShortcutRecordingHint")}</p>
      </section>

      {/* -- Codex local logs -------------------------------------- */}
      <section className="settings-section">
        <h3 className="settings-section__title settings-section__title--bold">
          {t("CodexLocalLogsTitle")}
        </h3>
        <p className="settings-section__caption">
          {t("CodexLocalLogsCaption")}
        </p>
        <div className="settings-section__group">
          <Field
            label={t("CodexLogPathsLabel")}
            description={t("CodexLogPathsHelper")}
          >
            <input
              type="text"
              className="text-input"
              value={codexDirsDraft}
              placeholder={String.raw`\\wsl.localhost\<distro>\home\<user>\.codex`}
              disabled={saving}
              onChange={(event) => setCodexDirsDraft(event.target.value)}
              onBlur={commitCodexDirs}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.currentTarget.blur();
                }
              }}
            />
          </Field>
        </div>
      </section>

      {/* -- Privacy ----------------------------------------------- */}
      <section className="settings-section">
        <h3 className="settings-section__title">{t("AgentSessionsTitle")}</h3>
        <div className="settings-section__group">
          <Field
            label={t("AgentSessionsEnableLabel")}
            description={t("AgentSessionsEnableHelper")}
            leading
          >
            <Toggle
              checked={settings.agentSessionsEnabled ?? false}
              disabled={saving}
              onChange={(v) => set({ agentSessionsEnabled: v })}
            />
          </Field>
          <Field
            label={t("AgentSessionsSshHostsLabel")}
            description={t("AgentSessionsSshHostsHelper")}
          >
            <input
              type="text"
              className="text-input"
              value={sshHostsDraft}
              disabled={saving || !settings.agentSessionsEnabled}
              onChange={(event) => setSshHostsDraft(event.target.value)}
              onBlur={() => set({ agentSessionSshHosts: parseSshHosts(sshHostsDraft) })}
              onKeyDown={(event) => {
                if (event.key === "Enter") event.currentTarget.blur();
              }}
            />
          </Field>
        </div>
      </section>

      {/* -- Privacy ----------------------------------------------- */}
      <section className="settings-section">
        <h3 className="settings-section__title">{t("PrivacyTitle")}</h3>
        <div className="settings-section__group">
          <Field
            label={t("HidePersonalInfo")}
            description={t("HidePersonalInfoHelper")}
            leading
          >
            <Toggle
              checked={settings.hidePersonalInfo}
              disabled={saving}
              onChange={(v) => set({ hidePersonalInfo: v })}
            />
          </Field>
        </div>
      </section>

      {/* -- Local integrations ----------------------------------- */}
      <section className="settings-section">
        <h3 className="settings-section__title">
          {t("SectionLocalIntegrations")}
        </h3>
        <div className="settings-section__group">
          <Field
            label={t("PowerToysPipeLabel")}
            description={t("PowerToysPipeHelper")}
            leading
          >
            <Toggle
              checked={settings.powertoysStatusPipeEnabled}
              disabled={saving}
              onChange={(v) => set({ powertoysStatusPipeEnabled: v })}
            />
          </Field>
        </div>
      </section>

      {/* -- Network proxy ---------------------------------------- */}
      <section className="settings-section">
        <h3 className="settings-section__title">{t("NetworkProxyTitle")}</h3>
        <p className="settings-section__caption">{t("NetworkProxyCaption")}</p>
        <div className="settings-section__group">
          <Field
            label={t("NetworkProxyEnableLabel")}
            description={t("NetworkProxyEnableHelper")}
            leading
          >
            <Toggle
              checked={settings.httpProxyEnabled ?? false}
              disabled={saving}
              onChange={(v) => set({ httpProxyEnabled: v })}
            />
          </Field>
          <Field
            label={t("NetworkProxyUrlLabel")}
            description={t("NetworkProxyUrlHelper")}
          >
            <input
              type="text"
              className="text-input"
              value={settings.httpProxyUrl ?? ""}
              placeholder="http://127.0.0.1:7890"
              disabled={saving || !settings.httpProxyEnabled}
              onChange={(event) => set({ httpProxyUrl: event.target.value })}
              onBlur={(event) =>
                set({ httpProxyUrl: event.target.value.trim() })
              }
            />
          </Field>
          <Field label={t("NetworkProxyUserLabel")}>
            <input
              type="text"
              className="text-input"
              value={settings.httpProxyUsername ?? ""}
              autoComplete="off"
              disabled={saving || !settings.httpProxyEnabled}
              onChange={(event) =>
                set({ httpProxyUsername: event.target.value })
              }
            />
          </Field>
          <Field
            label={t("NetworkProxyPasswordLabel")}
            description={t("NetworkProxyPasswordHelper")}
          >
            <input
              type="password"
              className="text-input"
              value={settings.httpProxyPassword ?? ""}
              autoComplete="new-password"
              disabled={saving || !settings.httpProxyEnabled}
              onChange={(event) =>
                set({ httpProxyPassword: event.target.value })
              }
            />
          </Field>
        </div>
      </section>

      {/* -- External hooks --------------------------------------- */}
      <section className="settings-section">
        <h3 className="settings-section__title">{t("HooksTitle")}</h3>
        <p className="settings-section__caption">{t("HooksCaption")}</p>
        <div className="settings-section__group">
          <Field
            label={t("HooksEnableLabel")}
            description={t("HooksEnableHelper")}
            leading
          >
            <Toggle
              checked={settings.hooksEnabled ?? false}
              disabled={saving}
              onChange={(v) => set({ hooksEnabled: v })}
            />
          </Field>
        </div>
        <p className="settings-section__hint">{t("HooksConfigPathHint")}</p>
      </section>

      {/* ── Keychain access ──────────────────────────────────────── */}
      <section className="settings-section">
        <h3 className="settings-section__title settings-section__title--bold">
          KEYCHAIN ACCESS
        </h3>
        <p className="settings-section__caption">
          Disable all Keychain reads and writes. Browser cookie import is
          unavailable; paste Cookie headers manually in Providers.
        </p>
        <div className="settings-section__group">
          <Field
            label={t("DisableAllKeychainLabel")}
            description={t("DisableAllKeychainHelper")}
            leading
          >
            <Toggle
              checked={settings.disableKeychainAccess}
              disabled={saving}
              onChange={(v) => set({ disableKeychainAccess: v })}
            />
          </Field>
          <Field
            label={t("AvoidKeychainPromptsLabel")}
            description={t("AvoidKeychainPromptsHelper")}
            leading
          >
            <Toggle
              checked={settings.claudeAvoidKeychainPrompts}
              disabled={saving || settings.disableKeychainAccess}
              onChange={(v) => set({ claudeAvoidKeychainPrompts: v })}
            />
          </Field>
        </div>
      </section>
    </>
  );
}
