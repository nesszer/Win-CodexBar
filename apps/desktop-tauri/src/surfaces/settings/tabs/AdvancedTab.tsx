import { useCallback, useEffect, useState } from "react";
import { useLocale } from "../../../hooks/useLocale";
import {
  getSafeDiagnostics,
  registerGlobalShortcut,
  unregisterGlobalShortcut,
} from "../../../lib/tauri";
import { ShortcutCapture } from "../../../components/ShortcutCapture";
import { Field, SettingToggle } from "../../../components/FormControls";
import type { TabProps } from "../settingsTabs";
import PreferencesTransferSection from "./PreferencesTransferSection";

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
  const ctl = { settings, set, saving };
  const [shortcutError, setShortcutError] = useState<string | null>(null);
  const [diagnosticsStatus, setDiagnosticsStatus] = useState<string | null>(
    null,
  );
  const [codexDirsDraft, setCodexDirsDraft] = useState(() =>
    formatCodexSessionsDirs(settings.codexCustomSessionsDirs),
  );
  const [sshHostsDraft, setSshHostsDraft] = useState(() =>
    (settings.agentSessionSshHosts ?? []).join(", "),
  );
  const [proxyUrlDraft, setProxyUrlDraft] = useState(() =>
    settings.httpProxyUrl ?? "",
  );
  const [proxyUsernameDraft, setProxyUsernameDraft] = useState(() =>
    settings.httpProxyUsername ?? "",
  );
  const [proxyPasswordDraft, setProxyPasswordDraft] = useState(() =>
    settings.httpProxyPassword ?? "",
  );

  const copyDiagnostics = useCallback(async () => {
    try {
      const text = await getSafeDiagnostics();
      await navigator.clipboard.writeText(text);
      setDiagnosticsStatus(t("DiagnosticsCopied"));
    } catch (error) {
      setDiagnosticsStatus(`${t("DiagnosticsCopyFailed")} ${String(error)}`);
    }
  }, [t]);

  const commitCodexDirs = useCallback(() => {
    set({ codexCustomSessionsDirs: parseCodexSessionsDirs(codexDirsDraft) });
  }, [codexDirsDraft, set]);

  useEffect(() => {
    if (!saving) {
      setCodexDirsDraft(formatCodexSessionsDirs(settings.codexCustomSessionsDirs));
    }
  }, [saving, settings.codexCustomSessionsDirs]);

  useEffect(() => {
    if (!saving) setSshHostsDraft((settings.agentSessionSshHosts ?? []).join(", "));
  }, [saving, settings.agentSessionSshHosts]);

  useEffect(() => {
    if (!saving) setProxyUrlDraft(settings.httpProxyUrl ?? "");
  }, [saving, settings.httpProxyUrl]);

  useEffect(() => {
    if (!saving) setProxyUsernameDraft(settings.httpProxyUsername ?? "");
  }, [saving, settings.httpProxyUsername]);

  useEffect(() => {
    if (!saving) setProxyPasswordDraft(settings.httpProxyPassword ?? "");
  }, [saving, settings.httpProxyPassword]);

  const commitProxyUrl = useCallback(() => {
    const next = proxyUrlDraft.trim();
    if (next !== (settings.httpProxyUrl ?? "")) set({ httpProxyUrl: next });
  }, [proxyUrlDraft, set, settings.httpProxyUrl]);

  const commitProxyUsername = useCallback(() => {
    const next = proxyUsernameDraft.trim();
    if (next !== (settings.httpProxyUsername ?? "")) set({ httpProxyUsername: next });
  }, [proxyUsernameDraft, set, settings.httpProxyUsername]);

  const commitProxyPassword = useCallback(() => {
    if (proxyPasswordDraft !== (settings.httpProxyPassword ?? "")) {
      set({ httpProxyPassword: proxyPasswordDraft });
    }
  }, [proxyPasswordDraft, set, settings.httpProxyPassword]);

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
              aria-label={t("CodexLogPathsLabel")}
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
          <SettingToggle
            ctl={ctl}
            field="agentSessionsEnabled"
            label={t("AgentSessionsEnableLabel")}
            description={t("AgentSessionsEnableHelper")}
          />
          <SettingToggle
            ctl={ctl}
            field="stayAwakeEnabled"
            label={t("AgentSessionsStayAwakeLabel")}
            description={t("AgentSessionsStayAwakeHelper")}
          />
          <Field
            label={t("AgentSessionsSshHostsLabel")}
            description={t("AgentSessionsSshHostsHelper")}
          >
            <input
              type="text"
              className="text-input"
              value={sshHostsDraft}
              aria-label={t("AgentSessionsSshHostsLabel")}
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
          <SettingToggle
            ctl={ctl}
            field="hidePersonalInfo"
            label={t("HidePersonalInfo")}
            description={t("HidePersonalInfoHelper")}
          />
        </div>
      </section>

      {/* -- Local integrations ----------------------------------- */}
      <section className="settings-section">
        <h3 className="settings-section__title">
          {t("SectionLocalIntegrations")}
        </h3>
        <div className="settings-section__group">
          <SettingToggle
            ctl={ctl}
            field="powertoysStatusPipeEnabled"
            label={t("PowerToysPipeLabel")}
            description={t("PowerToysPipeHelper")}
          />
        </div>
      </section>

      {/* -- Network proxy ---------------------------------------- */}
      <section className="settings-section">
        <h3 className="settings-section__title">{t("NetworkProxyTitle")}</h3>
        <p className="settings-section__caption">{t("NetworkProxyCaption")}</p>
        <div className="settings-section__group">
          <SettingToggle
            ctl={ctl}
            field="httpProxyEnabled"
            label={t("NetworkProxyEnableLabel")}
            description={t("NetworkProxyEnableHelper")}
          />
          <Field
            label={t("NetworkProxyUrlLabel")}
            description={t("NetworkProxyUrlHelper")}
          >
            <input
              type="text"
              className="text-input"
              value={proxyUrlDraft}
              placeholder="http://127.0.0.1:7890"
              aria-label={t("NetworkProxyUrlLabel")}
              disabled={saving || !settings.httpProxyEnabled}
              onChange={(event) => setProxyUrlDraft(event.target.value)}
              onBlur={commitProxyUrl}
              onKeyDown={(event) => {
                if (event.key === "Enter") event.currentTarget.blur();
              }}
            />
          </Field>
          <Field label={t("NetworkProxyUserLabel")}>
            <input
              type="text"
              className="text-input"
              value={proxyUsernameDraft}
              aria-label={t("NetworkProxyUserLabel")}
              autoComplete="off"
              disabled={saving || !settings.httpProxyEnabled}
              onChange={(event) => setProxyUsernameDraft(event.target.value)}
              onBlur={commitProxyUsername}
              onKeyDown={(event) => {
                if (event.key === "Enter") event.currentTarget.blur();
              }}
            />
          </Field>
          <Field
            label={t("NetworkProxyPasswordLabel")}
            description={t("NetworkProxyPasswordHelper")}
          >
            <input
              type="password"
              className="text-input"
              value={proxyPasswordDraft}
              aria-label={t("NetworkProxyPasswordLabel")}
              autoComplete="new-password"
              disabled={saving || !settings.httpProxyEnabled}
              onChange={(event) => setProxyPasswordDraft(event.target.value)}
              onBlur={commitProxyPassword}
              onKeyDown={(event) => {
                if (event.key === "Enter") event.currentTarget.blur();
              }}
            />
          </Field>
        </div>
      </section>

      {/* -- External hooks --------------------------------------- */}
      <section className="settings-section">
        <h3 className="settings-section__title">{t("HooksTitle")}</h3>
        <p className="settings-section__caption">{t("HooksCaption")}</p>
        <div className="settings-section__group">
          <SettingToggle
            ctl={ctl}
            field="hooksEnabled"
            label={t("HooksEnableLabel")}
            description={t("HooksEnableHelper")}
          />
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
          <SettingToggle
            ctl={ctl}
            field="disableKeychainAccess"
            label={t("DisableAllKeychainLabel")}
            description={t("DisableAllKeychainHelper")}
          />
          <SettingToggle
            ctl={ctl}
            field="claudeAvoidKeychainPrompts"
            label={t("AvoidKeychainPromptsLabel")}
            description={t("AvoidKeychainPromptsHelper")}
            disabled={saving || settings.disableKeychainAccess}
          />
        </div>
      </section>

      <PreferencesTransferSection />

      {/* ── Diagnostics ──────────────────────────────────────────── */}
      <section className="settings-section">
        <h3 className="settings-section__title settings-section__title--bold">
          {t("DiagnosticsSectionHeading")}
        </h3>
        <div className="settings-section__group">
          <button
            type="button"
            className="credential-btn"
            onClick={() => void copyDiagnostics()}
          >
            {t("DiagnosticsCopyButton")}
          </button>
          {diagnosticsStatus && (
            <p className="settings-section__hint">{diagnosticsStatus}</p>
          )}
        </div>
      </section>
    </>
  );
}
