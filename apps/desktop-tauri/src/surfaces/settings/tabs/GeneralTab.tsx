import { useCallback, useEffect, useRef, useState } from "react";
import { useLocale } from "../../../hooks/useLocale";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  getAppInfo,
  playNotificationSound,
  quitApp,
  registerGlobalShortcut,
  unregisterGlobalShortcut,
} from "../../../lib/tauri";
import type { AppInfoBridge } from "../../../types/bridge";
import {
  Field,
  NumberInput,
  Select,
  SettingsGroup,
  Toggle,
} from "../../../components/FormControls";
import { ShortcutCapture } from "../../../components/ShortcutCapture";
import type {
  Language,
  LanguageOption,
  NotificationSoundEvent,
  NotificationSoundPaths,
  NotificationSoundTheme,
  ThemePreference,
} from "../../../types/bridge";
import type { LocaleKey } from "../../../i18n/keys";
import { CURRENCY_PICKER_OPTIONS } from "../../../lib/currency";
import type { TabProps } from "../settingsTabs";
import PreferencesTransferSection from "./PreferencesTransferSection";
import SwitcherShortcutsDialog from "../SwitcherShortcutsDialog";

/** Select value for Language "System" (follow the Windows display language). */
const SYSTEM_LANGUAGE = "system";

const FALLBACK_LANGUAGE_OPTIONS: LanguageOption[] = [
  { value: "english", display: "English" },
  { value: "chinese", display: "中文" },
  { value: "chinesetraditional", display: "繁體中文" },
  { value: "japanese", display: "日本語" },
  { value: "korean", display: "한국어" },
  { value: "spanish", display: "Español" },
  { value: "portuguesebrazil", display: "Português (Brasil)" },
  { value: "russian", display: "Русский" },
  { value: "turkish", display: "Türkçe" },
  { value: "ukrainian", display: "Українська" },
];

// Mac order (Manual, minutes, Adaptive last) plus the Windows "1 hour",
// which existing installs may have saved.
const REFRESH_CADENCE_OPTIONS: { value: string; labelKey: LocaleKey }[] = [
  { value: "0", labelKey: "RefreshIntervalManual" },
  { value: "60", labelKey: "RefreshInterval1Min" },
  { value: "120", labelKey: "RefreshInterval2Min" },
  { value: "300", labelKey: "RefreshInterval5Min" },
  { value: "900", labelKey: "RefreshInterval15Min" },
  { value: "1800", labelKey: "RefreshInterval30Min" },
  { value: "3600", labelKey: "RefreshInterval1Hour" },
  { value: "adaptive", labelKey: "RefreshIntervalAdaptive" },
];

const NOTIFICATION_SOUND_THEME_OPTIONS: {
  value: NotificationSoundTheme;
  labelKey: LocaleKey;
}[] = [
  { value: "windows", labelKey: "NotificationSoundThemeWindows" },
  { value: "codexBar", labelKey: "NotificationSoundThemeCodexBar" },
];

type NotificationSoundPathKey = keyof NotificationSoundPaths;

const NOTIFICATION_SOUND_EVENTS: {
  event: NotificationSoundEvent;
  pathKey: NotificationSoundPathKey;
  labelKey: LocaleKey;
  helperKey: LocaleKey;
}[] = [
  {
    event: "predictiveWarning",
    pathKey: "predictiveWarning",
    labelKey: "NotificationSoundEventPredictiveWarning",
    helperKey: "NotificationSoundEventPredictiveWarningHelper",
  },
  {
    event: "highUsage",
    pathKey: "highUsage",
    labelKey: "NotificationSoundEventHighUsage",
    helperKey: "NotificationSoundEventHighUsageHelper",
  },
  {
    event: "criticalUsage",
    pathKey: "criticalUsage",
    labelKey: "NotificationSoundEventCriticalUsage",
    helperKey: "NotificationSoundEventCriticalUsageHelper",
  },
  {
    event: "exhausted",
    pathKey: "exhausted",
    labelKey: "NotificationSoundEventExhausted",
    helperKey: "NotificationSoundEventExhaustedHelper",
  },
  {
    event: "statusIssue",
    pathKey: "statusIssue",
    labelKey: "NotificationSoundEventStatusIssue",
    helperKey: "NotificationSoundEventStatusIssueHelper",
  },
  {
    event: "sessionDepleted",
    pathKey: "sessionDepleted",
    labelKey: "NotificationSoundEventSessionDepleted",
    helperKey: "NotificationSoundEventSessionDepletedHelper",
  },
  {
    event: "sessionRestored",
    pathKey: "sessionRestored",
    labelKey: "NotificationSoundEventSessionRestored",
    helperKey: "NotificationSoundEventSessionRestoredHelper",
  },
];

const NOTIFICATION_SOUND_THEME_SELECT_MIN_WIDTH = 180;
const NOTIFICATION_SOUND_PREVIEW_DURATION_MS = 1500;


const THEME_OPTIONS: { value: ThemePreference; labelKey: LocaleKey }[] = [
  { value: "auto", labelKey: "ThemeAutoOption" },
  { value: "light", labelKey: "ThemeLightOption" },
  { value: "dark", labelKey: "ThemeDarkOption" },
];

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

function isNotificationSoundTheme(v: string): v is NotificationSoundTheme {
  return v === "windows" || v === "codexBar";
}

/** Lead-time choices for pace warnings, in hours; 0 (any time) is listed first. */
const PACE_WARNING_LEAD_HOURS = [0, 6, 12, 24, 48];

/** The presets plus the saved value when it is not one (e.g. 168 from imported
 *  preferences), kept in order so the select shows what is saved. */
function paceWarningLeadOptions(current: number): number[] {
  const options = [...PACE_WARNING_LEAD_HOURS];
  if (!options.includes(current)) options.push(current);
  return options.sort((a, b) => a - b);
}

export default function GeneralTab({
  mode = "general",
  settings,
  set,
  saving,
}: TabProps & { mode?: "general" | "notifications" }) {
  const { t } = useLocale();
  const ctl = { settings, set, saving };
  const [playingSound, setPlayingSound] = useState<NotificationSoundEvent | null>(null);
  const [soundError, setSoundError] = useState<string | null>(null);
  const [shortcutError, setShortcutError] = useState<string | null>(null);
  const [switcherOpen, setSwitcherOpen] = useState(false);
  const [languageOptions, setLanguageOptions] = useState<LanguageOption[]>(
    FALLBACK_LANGUAGE_OPTIONS,
  );

  useEffect(() => {
    invoke<LanguageOption[]>("get_available_languages")
      .then(setLanguageOptions)
      .catch(() => {}); // graceful fallback to static default
  }, []);

  const [appVersion, setAppVersion] = useState<AppInfoBridge | null>(null);

  useEffect(() => {
    getAppInfo()
      .then(setAppVersion)
      .catch(() => {}); // version row degrades to absent, never blocks the tab
  }, []);

  const soundPreviewTimeoutRef = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (soundPreviewTimeoutRef.current !== null) {
        window.clearTimeout(soundPreviewTimeoutRef.current);
      }
    },
    [],
  );

  const handleTestSound = useCallback((event: NotificationSoundEvent) => {
    setSoundError(null);
    setPlayingSound(event);
    if (soundPreviewTimeoutRef.current !== null) {
      window.clearTimeout(soundPreviewTimeoutRef.current);
    }
    const timeoutId = window.setTimeout(() => {
      soundPreviewTimeoutRef.current = null;
      setPlayingSound(null);
    }, NOTIFICATION_SOUND_PREVIEW_DURATION_MS);
    soundPreviewTimeoutRef.current = timeoutId;
    void playNotificationSound(event).catch((error: unknown) => {
      window.clearTimeout(timeoutId);
      if (soundPreviewTimeoutRef.current === timeoutId) {
        soundPreviewTimeoutRef.current = null;
      }
      setPlayingSound(null);
      setSoundError(error instanceof Error ? error.message : String(error));
    });
  }, []);

  const handleChooseSound = useCallback(
    async (pathKey: NotificationSoundPathKey) => {
      setSoundError(null);
      try {
        const selected = await open({
          multiple: false,
          directory: false,
          filters: [{ name: t("NotificationSoundWaveFile"), extensions: ["wav"] }],
        });
        if (typeof selected === "string") {
          set({
            notificationSoundPaths: {
              ...settings.notificationSoundPaths,
              [pathKey]: selected,
            },
          });
        }
      } catch (error: unknown) {
        setSoundError(error instanceof Error ? error.message : String(error));
      }
    },
    [set, settings.notificationSoundPaths, t],
  );

  const handleClearSound = useCallback(
    (pathKey: NotificationSoundPathKey) => {
      setSoundError(null);
      set({
        notificationSoundPaths: {
          ...settings.notificationSoundPaths,
          [pathKey]: null,
        },
      });
    },
    [set, settings.notificationSoundPaths],
  );

  const commitShortcut = useCallback(
    async (accelerator: string) => {
      setShortcutError(null);
      try {
        await registerGlobalShortcut(accelerator);
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
      await unregisterGlobalShortcut();
      set({ globalShortcut: "" });
    } catch (err: unknown) {
      setShortcutError(err instanceof Error ? err.message : String(err));
    }
  }, [set]);

  if (mode === "notifications") {
    return (
      <NotificationsPane
        settings={settings}
        set={set}
        saving={saving}
        playingSound={playingSound}
        soundError={soundError}
        onTestSound={handleTestSound}
        onChooseSound={(pathKey) => void handleChooseSound(pathKey)}
        onClearSound={handleClearSound}
      />
    );
  }

  const refreshValue = settings.adaptiveRefresh
    ? "adaptive"
    : String(settings.refreshIntervalSecs);
  const refreshOptions = REFRESH_CADENCE_OPTIONS.map((o) => ({
    value: o.value,
    label: t(o.labelKey),
  }));
  // A saved interval outside the menu (for example 30 s from an older build)
  // stays visible instead of the menu silently showing another value.
  if (!refreshOptions.some((o) => o.value === refreshValue)) {
    refreshOptions.push({
      value: refreshValue,
      label: `${settings.refreshIntervalSecs} s`,
    });
  }
  const manualRefresh = !settings.adaptiveRefresh && settings.refreshIntervalSecs === 0;

  return (
    <>
      <SettingsGroup title={t("StartupSettings")}>
        <Field label={t("InterfaceLanguage")} description={t("GeneralLanguageHelper")}>
          <Select
            variant="menu"
            value={settings.uiLanguageFollowsSystem ? SYSTEM_LANGUAGE : settings.uiLanguage}
            disabled={saving}
            ariaLabel={t("InterfaceLanguage")}
            options={[
              // Mac PreferencesGeneralPane.swift:6 lists "System" first, labelled in English.
              { value: SYSTEM_LANGUAGE, label: "System" },
              ...languageOptions.map((opt) => ({
                value: opt.value,
                label: opt.display,
              })),
            ]}
            onChange={(v) =>
              set(
                v === SYSTEM_LANGUAGE
                  ? { uiLanguageFollowsSystem: true }
                  : { uiLanguage: v as Language },
              )
            }
          />
        </Field>
        <Field label={t("PreferredCurrencyLabel")} description={t("PreferredCurrencyHelper")}>
          <Select
            variant="menu"
            value={settings.preferredCurrencyCode ?? "AUTO"}
            disabled={saving}
            ariaLabel={t("PreferredCurrencyLabel")}
            options={[
              { value: "AUTO", label: "AUTO" },
              ...CURRENCY_PICKER_OPTIONS,
            ]}
            onChange={(value) => set({ preferredCurrencyCode: value })}
          />
        </Field>
        <Field label={t("Appearance")} description={t("ThemeHelper")}>
          <Select
            variant="menu"
            value={settings.theme}
            disabled={saving}
            ariaLabel={t("ThemeLabel")}
            options={THEME_OPTIONS.map((option) => ({
              value: option.value,
              label: t(option.labelKey),
            }))}
            onChange={(value) => set({ theme: value as ThemePreference })}
          />
        </Field>
        <Field label={t("StartAtLogin")}>
          <Toggle
            variant="switch"
            checked={settings.startAtLogin}
            disabled={saving}
            onChange={(v) => set({ startAtLogin: v })}
          />
        </Field>
        <Field label={t("StartMinimized")} description={t("StartMinimizedHelper")}>
          <Toggle
            variant="switch"
            checked={settings.startMinimized}
            disabled={saving}
            onChange={(v) => set({ startMinimized: v })}
          />
        </Field>
      </SettingsGroup>

      <SettingsGroup
        title={t("SectionRefreshing")}
        footer={manualRefresh ? <p>{t("RefreshManualFooter")}</p> : undefined}
      >
        <Field label={t("RefreshIntervalLabel")}>
          <Select
            variant="menu"
            value={refreshValue}
            disabled={saving}
            options={refreshOptions}
            onChange={(v) => {
              if (v === "adaptive") {
                set({ adaptiveRefresh: true });
                return;
              }
              set({
                adaptiveRefresh: false,
                refreshIntervalSecs: Number(v),
              });
            }}
          />
        </Field>
        <Field label={t("RefreshAllProvidersOnMenuOpen")}>
          <Toggle
            variant="switch"
            checked={settings.refreshAllProvidersOnMenuOpen}
            disabled={saving}
            onChange={(v) => set({ refreshAllProvidersOnMenuOpen: v })}
          />
        </Field>
        <Field label={t("LowPowerMode")} description={t("LowPowerModeHelper")}>
          <Select
            variant="menu"
            value={settings.lowPowerModePreference ?? (settings.lowPowerMode ? "on" : "off")}
            disabled={saving}
            options={[
              { value: "off", label: t("LowPowerModeOff") },
              { value: "on", label: t("LowPowerModeOn") },
              { value: "automatic", label: t("LowPowerModeAutomatic") },
            ]}
            onChange={(v) => set({
              lowPowerModePreference: v as "off" | "on" | "automatic",
            })}
          />
        </Field>
        <Field
          label={t("CheckProviderStatus")}
          description={t("CheckProviderStatusHelper")}
        >
          <Toggle
            variant="switch"
            checked={settings.statusChecksEnabled}
            disabled={saving}
            onChange={(v) => set({ statusChecksEnabled: v })}
          />
        </Field>
      </SettingsGroup>

      <PreferencesTransferSection onOpenSwitcherShortcuts={() => setSwitcherOpen(true)} />

      <SettingsGroup
        title={t("SectionKeyboard")}
        footer={
          <>
            {shortcutError && (
              <p className="settings-group__error" role="alert">{shortcutError}</p>
            )}
            <p>{t("ShortcutRecordingHint")}</p>
          </>
        }
      >
        <Field label={t("GlobalShortcutFieldLabel")}>
          <ShortcutCapture
            value={settings.globalShortcut}
            disabled={saving}
            onCommit={(accel) => void commitShortcut(accel)}
            onClear={() => void clearShortcut()}
          />
        </Field>
      </SettingsGroup>

      <div className="settings-pane-footer">
        <span>
          {appVersion &&
            `${t("Version")} ${appVersion.version}${
              appVersion.buildNumber !== "dev" ? ` (${appVersion.buildNumber})` : ""
            }`}
        </span>
        <button
          type="button"
          className="credential-btn credential-btn--primary"
          onClick={() => void quitApp()}
        >
          {t("QuitCodexBar")}
        </button>
      </div>

      {switcherOpen && (
        <SwitcherShortcutsDialog
          settings={settings}
          set={set}
          saving={saving}
          onClose={() => setSwitcherOpen(false)}
        />
      )}
    </>
  );
}

function NotificationsPane({
  settings,
  set,
  saving,
  playingSound,
  soundError,
  onTestSound,
  onChooseSound,
  onClearSound,
}: TabProps & {
  playingSound: NotificationSoundEvent | null;
  soundError: string | null;
  onTestSound: (event: NotificationSoundEvent) => void;
  onChooseSound: (pathKey: NotificationSoundPathKey) => void;
  onClearSound: (pathKey: NotificationSoundPathKey) => void;
}) {
  const { t } = useLocale();
  // `show_notifications` still gates credential and pace alerts in the
  // backend, so their switches lock (keeping their saved state) while
  // threshold warnings are off. Quota depleted & restored has its own key.
  const alertsMuted = !settings.showNotifications;

  return (
    <>
      <SettingsGroup title={t("SectionAlerts")}>
        <Field
          label={t("CredentialExpiryNotifications")}
          description={t("CredentialExpiryNotificationsHelper")}
        >
          <Toggle
            variant="switch"
            checked={settings.credentialExpiryNotificationsEnabled}
            ariaLabel={t("CredentialExpiryNotifications")}
            disabled={saving || alertsMuted}
            onChange={(v) => set({ credentialExpiryNotificationsEnabled: v })}
          />
        </Field>
        <Field
          label={t("SessionQuotaNotifications")}
          description={t("SessionQuotaNotificationsHelper")}
        >
          <Toggle
            variant="switch"
            checked={
              settings.sessionQuotaNotificationsEnabled ?? settings.showNotifications
            }
            ariaLabel={t("SessionQuotaNotifications")}
            disabled={saving}
            onChange={(v) => set({ sessionQuotaNotificationsEnabled: v })}
          />
        </Field>
        <Field label={t("ShowNotifications")} description={t("ShowNotificationsHelper")}>
          <Toggle
            variant="switch"
            checked={settings.showNotifications}
            ariaLabel={t("ShowNotifications")}
            disabled={saving}
            onChange={(v) => set({ showNotifications: v })}
          />
        </Field>
        <Field
          label={t("PredictivePaceWarnings")}
          description={t("PredictivePaceWarningsHelper")}
        >
          <Toggle
            variant="switch"
            checked={settings.predictivePaceWarningEnabled}
            ariaLabel={t("PredictivePaceWarnings")}
            disabled={saving || alertsMuted}
            onChange={(v) => set({ predictivePaceWarningEnabled: v })}
          />
        </Field>
        <Field
          label={t("PredictivePaceWarningLeadLabel")}
          description={t("PredictivePaceWarningLeadHelper")}
        >
          <Select
            variant="menu"
            value={String(settings.predictivePaceWarningLeadHours ?? 0)}
            disabled={saving || alertsMuted || !settings.predictivePaceWarningEnabled}
            ariaLabel={t("PredictivePaceWarningLeadLabel")}
            options={paceWarningLeadOptions(settings.predictivePaceWarningLeadHours ?? 0).map(
              (hours) => ({
                value: String(hours),
                label:
                  hours === 0
                    ? t("PredictivePaceWarningLeadAny")
                    : t("PredictivePaceWarningLeadHours").replace("{}", String(hours)),
              }),
            )}
            onChange={(value) => set({ predictivePaceWarningLeadHours: Number(value) })}
          />
        </Field>
        {/* Independent of the master alerts switch, like the Mac: a reset is
            good news, not a usage warning. */}
        <Field
          label={t("LimitResetNotifications")}
          description={t("LimitResetNotificationsHelper")}
        >
          <Toggle
            variant="switch"
            checked={settings.limitResetNotificationsEnabled ?? false}
            ariaLabel={t("LimitResetNotifications")}
            disabled={saving}
            onChange={(v) => set({ limitResetNotificationsEnabled: v })}
          />
        </Field>
      </SettingsGroup>

      {/* The thresholds stay visible while threshold warnings are off: they also set
          the usage-bar and float-bar colours (#816). The Mac hides them. */}
      <SettingsGroup footer={<p>{t("ThresholdsCaption")}</p>}>
        <Field label={t("WarningAtPercentUsed")}>
          <NumberInput
            value={settings.highUsageThreshold}
            min={0}
            max={100}
            step={5}
            disabled={saving}
            onChange={(v) => set({ highUsageThreshold: v })}
          />
        </Field>
        <Field label={t("CriticalAtPercentUsed")}>
          <NumberInput
            value={settings.criticalUsageThreshold}
            min={0}
            max={100}
            step={5}
            disabled={saving}
            onChange={(v) => set({ criticalUsageThreshold: v })}
          />
        </Field>
      </SettingsGroup>

      <SettingsGroup>
        <Field label={t("SoundEnabled")} description={t("SoundEnabledHelper")}>
          <Toggle
            variant="switch"
            checked={settings.soundEnabled}
            ariaLabel={t("SoundEnabled")}
            disabled={saving}
            onChange={(v) => set({ soundEnabled: v })}
          />
        </Field>
        {settings.soundEnabled && (
          <Field
            label={t("NotificationSoundTheme")}
            description={t("NotificationSoundThemeHelper")}
          >
            <Select
              variant="menu"
              value={settings.notificationSoundTheme}
              disabled={saving}
              ariaLabel={t("NotificationSoundTheme")}
              minWidth={NOTIFICATION_SOUND_THEME_SELECT_MIN_WIDTH}
              options={NOTIFICATION_SOUND_THEME_OPTIONS.map((option) => ({
                value: option.value,
                label: t(option.labelKey),
              }))}
              onChange={(value) => {
                if (isNotificationSoundTheme(value)) {
                  set({ notificationSoundTheme: value });
                }
              }}
            />
          </Field>
        )}
        {settings.soundEnabled &&
          NOTIFICATION_SOUND_EVENTS.map((sound) => {
            const path = settings.notificationSoundPaths[sound.pathKey];
            const label = t(sound.labelKey);
            return (
              <Field key={sound.event} label={label} description={t(sound.helperKey)}>
                <div className="notification-sound-row">
                  <button
                    type="button"
                    className="shortcut-capture__button shortcut-capture__button--ghost notification-sound-file"
                    aria-label={`${label}: ${path ? `${fileName(path)}, ` : ""}${t("NotificationSoundChooseFile")}`}
                    title={path ?? t("NotificationSoundUsesTheme")}
                    disabled={saving}
                    onClick={() => onChooseSound(sound.pathKey)}
                  >
                    {path ? fileName(path) : t("NotificationSoundChooseFile")}
                  </button>
                  <button
                    type="button"
                    className="shortcut-capture__button shortcut-capture__button--ghost"
                    aria-label={`${label}: ${t("NotificationTestSound")}`}
                    disabled={saving || playingSound !== null}
                    onClick={() => onTestSound(sound.event)}
                  >
                    {playingSound === sound.event
                      ? t("NotificationTestSoundPlaying")
                      : t("NotificationTestSound")}
                  </button>
                  {path && (
                    <button
                      type="button"
                      className="shortcut-capture__button shortcut-capture__button--ghost"
                      aria-label={`${label}: ${t("NotificationSoundClearFile")}`}
                      disabled={saving}
                      onClick={() => onClearSound(sound.pathKey)}
                    >
                      {t("NotificationSoundClearFile")}
                    </button>
                  )}
                </div>
              </Field>
            );
          })}
        {soundError && (
          <p className="settings-group__error" role="alert">
            {soundError}
          </p>
        )}
      </SettingsGroup>

    </>
  );
}
