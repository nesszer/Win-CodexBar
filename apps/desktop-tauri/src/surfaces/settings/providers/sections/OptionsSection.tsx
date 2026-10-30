import { Fragment } from "react";
import type {
  ClaudeSwapAccountSource,
  ProviderDetail,
  SettingsSnapshot,
  SettingsUpdate,
} from "../../../../types/bridge";
import type { LocaleKey } from "../../../../i18n/keys";
import { Field, Select, SettingsGroup, Toggle } from "../../../../components/FormControls";
import { AutoResumeSection } from "./AutoResumeSection";
import { OptionalDetailsSection } from "./OptionalDetailsSection";
import { ClaudeSwapPathField } from "./credentials/ClaudeSwapPathField";
import { AntigravityProfileHomesField } from "./AntigravityProfileHomesField";
import { OpenAiLiveUsageSection } from "./OpenAiLiveUsageSection";

export type OptionSettings = Pick<
  SettingsSnapshot,
  | "antigravityAdditionalProfileHomes"
  | "openAiLiveUsageEnabled"
  | "openAiModelRateLimits"
  | "codexOpenaiWebExtras"
  | "codexHistoricalTracking"
  | "codexExternalOauthSourcesAllowed"
  | "codexLocalSessionCostLedgerEnabled"
  | "codexOpencodeLocalHistoryEnabled"
  | "codexOpenaiWebBatterySaverEnabled"
  | "claudeAvoidKeychainPrompts"
  | "claudeAllowReadingClaudeCodeCredentials"
  | "claudeSwapEnabled"
  | "claudeSwapExecutablePath"
  | "claudeSwapSource"
  | "claudeDesktopCookiesEnabled"
  | "disableKeychainAccess"
>;

/** Cookie sources the backend treats as automatic for Claude web usage. */
const AUTOMATIC_COOKIE_SOURCES = new Set(["auto", "browser", "web"]);

interface Props {
  detail: Pick<
    ProviderDetail,
    | "id"
    | "cookieSource"
    | "autoResumeAfterQuotaReset"
    | "autoResumeSupported"
    | "optionalDetailsEnabled"
    | "optionalDetailsSupported"
  >;
  settings: OptionSettings;
  disabled: boolean;
  t: (key: LocaleKey) => string;
  onSettingsChange: (patch: SettingsUpdate) => void;
  /** Reload the provider detail after a provider-scoped command saved. */
  onReload: () => void;
}

const CLAUDE_SWAP_SOURCES: ClaudeSwapAccountSource[] = ["claude-swap", "tcr"];

interface OptionRow {
  key: string;
  label: LocaleKey;
  help: LocaleKey;
  checked: boolean;
  patch: (on: boolean) => SettingsUpdate;
  /** Saved but not read on Windows yet; adds the "Not used on Windows yet" line. */
  inert?: boolean;
  locked?: boolean;
}

function codexRows(settings: OptionSettings): OptionRow[] {
  // Windows keeps OpenAI web extras on by default (the Mac default is off).
  const webExtras = settings.codexOpenaiWebExtras ?? true;
  const rows: OptionRow[] = [
    {
      key: "local-cost",
      label: "ProviderCodexLocalCostEstimates",
      help: "ProviderCodexLocalCostEstimatesHelp",
      checked: settings.codexLocalSessionCostLedgerEnabled ?? false,
      patch: (on) => ({ codexLocalSessionCostLedgerEnabled: on }),
      inert: true,
    },
    {
      key: "opencode-history",
      label: "ProviderCodexOpenCodeLocalHistory",
      help: "ProviderCodexOpenCodeLocalHistoryHelp",
      checked: settings.codexOpencodeLocalHistoryEnabled ?? false,
      patch: (on) => ({ codexOpencodeLocalHistoryEnabled: on }),
    },
    {
      key: "history",
      label: "ProviderCodexHistoricalTracking",
      help: "ProviderCodexHistoryHelp",
      checked: settings.codexHistoricalTracking ?? false,
      patch: (on) => ({ codexHistoricalTracking: on }),
      inert: true,
    },
    {
      key: "web-extras",
      label: "ProviderCodexOpenAiWebExtras",
      help: "ProviderCodexOpenAiWebExtrasHelp",
      checked: webExtras,
      patch: (on) => ({ codexOpenaiWebExtras: on }),
    },
    {
      key: "external-oauth",
      label: "ProviderCodexExternalOauth",
      help: "ProviderCodexExternalOauthHelp",
      checked: settings.codexExternalOauthSourcesAllowed ?? false,
      patch: (on) => ({ codexExternalOauthSourcesAllowed: on }),
    },
  ];
  if (webExtras) {
    rows.push({
      key: "battery-saver",
      label: "ProviderCodexBatterySaver",
      help: "ProviderCodexBatterySaverHelp",
      checked: settings.codexOpenaiWebBatterySaverEnabled ?? false,
      patch: (on) => ({ codexOpenaiWebBatterySaverEnabled: on }),
      inert: true,
    });
  }
  return rows;
}

function claudeRows(
  settings: OptionSettings,
  cookieSource: string | null,
): OptionRow[] {
  const rows: OptionRow[] = [
    {
      key: "claude-code-credentials",
      label: "ProviderClaudeAllowReadingClaudeCodeCredentials",
      help: "ProviderClaudeAllowReadingClaudeCodeCredentialsHelp",
      checked: settings.claudeAllowReadingClaudeCodeCredentials ?? false,
      patch: (on) => ({ claudeAllowReadingClaudeCodeCredentials: on }),
    },
    {
      // Its help already says the option is not enforced on Windows yet.
      key: "avoid-keychain",
      label: "ProviderClaudeAvoidKeychainPrompts",
      help: "ProviderClaudeAvoidKeychainPromptsHelp",
      checked: settings.claudeAvoidKeychainPrompts,
      patch: (on) => ({ claudeAvoidKeychainPrompts: on }),
      // Advanced > "Disable all Credential Manager access" overrides it.
      locked: settings.disableKeychainAccess,
    },
    {
      key: "claude-swap",
      label: "ClaudeSwapEnable",
      help: settings.claudeSwapSource === "tcr" ? "ClaudeTcrHint" : "ClaudeSwapHint",
      checked: settings.claudeSwapEnabled ?? false,
      patch: (on) => ({ claudeSwapEnabled: on }),
    },
  ];
  // The backend reads the desktop session only for the Automatic cookie source.
  if (cookieSource !== null && AUTOMATIC_COOKIE_SOURCES.has(cookieSource)) {
    rows.push({
      key: "claude-desktop-cookies",
      label: "ProviderClaudeDesktopCookies",
      help: "ProviderClaudeDesktopCookiesHelp",
      checked: settings.claudeDesktopCookiesEnabled ?? false,
      patch: (on) => ({ claudeDesktopCookiesEnabled: on }),
    });
  }
  return rows;
}

/**
 * Options group (macOS `settingsToggles`), last in the pane: the provider's
 * persisted switches, then the Windows-only automatic session resume.
 */
export function OptionsSection({
  detail,
  settings,
  disabled,
  t,
  onSettingsChange,
  onReload,
}: Props) {
  const rows =
    detail.id === "codex"
      ? codexRows(settings)
      : detail.id === "claude"
        ? claudeRows(settings, detail.cookieSource)
        : [];

  return (
    <SettingsGroup
      className="provider-detail-group provider-options-group"
      title={t("ProviderOptionsTitle")}
    >
      {/* Mac lists Claude's "Show workspace spend" first. */}
      <OptionalDetailsSection
        providerId={detail.id}
        enabled={detail.optionalDetailsEnabled}
        available={detail.optionalDetailsSupported}
        disabled={disabled}
        t={t}
        onChanged={onReload}
      />
      {rows.map((row) => (
        <Fragment key={row.key}>
          <Field
            label={t(row.label)}
            description={
              row.inert
                ? `${t(row.help)} ${t("ProviderOptionNotUsedOnWindows")}`
                : t(row.help)
            }
          >
            <Toggle
              variant="switch"
              checked={row.checked}
              disabled={disabled || row.locked}
              onChange={(on) => onSettingsChange(row.patch(on))}
            />
          </Field>
          {/* macOS attaches the executable path to this toggle as an inline
              field and shows the Source picker only while the toggle is on. */}
          {row.key === "claude-swap" && row.checked && (
            <>
              <ClaudeSwapPathField
                value={settings.claudeSwapExecutablePath ?? ""}
                source={settings.claudeSwapSource ?? "claude-swap"}
                disabled={disabled}
                t={t}
                onSettingsChange={onSettingsChange}
              />
              <Field label={t("ClaudeSwapSource")}>
                <Select
                  variant="menu"
                  value={settings.claudeSwapSource ?? "claude-swap"}
                  // Tool names, not prose: they stay untranslated in every locale.
                  options={CLAUDE_SWAP_SOURCES.map((source) => ({ value: source, label: source }))}
                  disabled={disabled}
                  onChange={(value) =>
                    onSettingsChange({ claudeSwapSource: value as ClaudeSwapAccountSource })
                  }
                />
              </Field>
            </>
          )}
        </Fragment>
      ))}
      {detail.id === "openaiapi" && (
        <OpenAiLiveUsageSection
          settings={settings}
          disabled={disabled}
          t={t}
          onSettingsChange={onSettingsChange}
        />
      )}
      {detail.id === "antigravity" && (
        <AntigravityProfileHomesField
          value={settings.antigravityAdditionalProfileHomes}
          disabled={disabled}
          t={t}
          onSettingsChange={onSettingsChange}
        />
      )}
      <AutoResumeSection
        providerId={detail.id}
        enabled={detail.autoResumeAfterQuotaReset}
        available={detail.autoResumeSupported}
        disabled={disabled}
        t={t}
        onChanged={onReload}
      />
    </SettingsGroup>
  );
}
