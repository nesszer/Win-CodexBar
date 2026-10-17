import { useEffect, useState } from "react";
import { Field, Toggle } from "../../../../components/FormControls";
import { OpenAITokensPerMinuteChart } from "../../../../components/OpenAITokensPerMinuteChart";
import type { LocaleKey } from "../../../../i18n/keys";
import {
  type LiveWindowMinutes,
  formatTokens,
  LIVE_WINDOW_MINUTES,
} from "../../../../lib/openaiThroughput";
import {
  getOpenAiLiveUsage,
  getOpenAiOrgRateLimits,
  getProviderWorkspaceId,
} from "../../../../lib/tauri";
import type {
  ModelRateLimitSnapshot,
  OrgRateLimitsSnapshot,
  ProviderThroughputSnapshot,
  SettingsUpdate,
} from "../../../../types/bridge";
import { PROVIDER_WORKSPACE_ID_SAVED_EVENT } from "./credentials/OpenAiExtras";
import { OpenAiModelLimitsEditor } from "./OpenAiModelLimitsEditor";

type T = (key: LocaleKey) => string;

export interface OpenAiLiveUsageSettings {
  /** Absent on older shells; the shell defaults it on. */
  openAiLiveUsageEnabled?: boolean;
  openAiModelRateLimits?: ModelRateLimitSnapshot[];
}

interface Props {
  settings: OpenAiLiveUsageSettings;
  disabled: boolean;
  t: T;
  onSettingsChange: (patch: SettingsUpdate) => void;
}

const NO_LIMITS: readonly ModelRateLimitSnapshot[] = [];

const WINDOW_LABELS: Record<LiveWindowMinutes, LocaleKey> = {
  15: "OpenAiLiveWindow15",
  60: "OpenAiLiveWindow60",
  360: "OpenAiLiveWindow360",
};

/**
 * OpenAI API Live Usage controls in Settings > Providers > OpenAI (upstream
 * 0.75.0 #4495): the refresh switch, the per-model limits editor, and an
 * on-demand Tokens/min window with the organization project caps.
 */
export function OpenAiLiveUsageSection({ settings, disabled, t, onSettingsChange }: Props) {
  const limits = settings.openAiModelRateLimits ?? NO_LIMITS;
  // Usage and org caps depend on the saved project scope; a new scope remounts the
  // view so loaded data and any pending load from the old scope are discarded.
  const [scopeVersion, setScopeVersion] = useState(0);
  useEffect(() => {
    const onSaved = (event: Event) => {
      if ((event as CustomEvent<string>).detail === "openaiapi") {
        setScopeVersion((version) => version + 1);
      }
    };
    window.addEventListener(PROVIDER_WORKSPACE_ID_SAVED_EVENT, onSaved);
    return () => window.removeEventListener(PROVIDER_WORKSPACE_ID_SAVED_EVENT, onSaved);
  }, []);
  return (
    <>
      <Field
        label={t("OpenAiLiveUsageToggle")}
        description={t("OpenAiLiveUsageToggleHelp")}
      >
        <Toggle
          variant="switch"
          checked={settings.openAiLiveUsageEnabled ?? true}
          disabled={disabled}
          onChange={(on) => onSettingsChange({ openAiLiveUsageEnabled: on })}
        />
      </Field>
      <OpenAiModelLimitsEditor
        limits={limits}
        disabled={disabled}
        t={t}
        onSave={(next) => onSettingsChange({ openAiModelRateLimits: next })}
      />
      <LiveUsageView key={scopeVersion} limits={limits} disabled={disabled} t={t} />
    </>
  );
}

function LiveUsageView({
  limits,
  disabled,
  t,
}: {
  limits: readonly ModelRateLimitSnapshot[];
  disabled: boolean;
  t: T;
}) {
  const [windowMinutes, setWindowMinutes] = useState<LiveWindowMinutes>(60);
  const [usageBusy, setUsageBusy] = useState(false);
  const [usageError, setUsageError] = useState<string | null>(null);
  const [usage, setUsage] = useState<ProviderThroughputSnapshot | null>(null);
  const [orgBusy, setOrgBusy] = useState(false);
  const [orgError, setOrgError] = useState<string | null>(null);
  const [orgScoped, setOrgScoped] = useState(false);
  const [org, setOrg] = useState<OrgRateLimitsSnapshot | null>(null);

  const loadUsage = async () => {
    setUsageBusy(true);
    setUsageError(null);
    try {
      setUsage(await getOpenAiLiveUsage(windowMinutes));
    } catch (error) {
      setUsage(null);
      setUsageError(String(error));
    } finally {
      setUsageBusy(false);
    }
  };

  const loadOrg = async () => {
    setOrgBusy(true);
    setOrgError(null);
    setOrgScoped(false);
    try {
      // A saved project scope limits the view to that project, so org caps stay hidden.
      const projectId = (await getProviderWorkspaceId("openaiapi"))?.trim() ?? "";
      if (projectId.length > 0) {
        setOrg(null);
        setOrgScoped(true);
        return;
      }
      setOrg(await getOpenAiOrgRateLimits());
    } catch (error) {
      setOrg(null);
      setOrgError(String(error));
    } finally {
      setOrgBusy(false);
    }
  };

  return (
    <section className="openai-live">
      <h4>{t("OpenAiLiveUsageTitle")}</h4>
      <label className="provider-detail-field">
        <span className="provider-detail-field__label">{t("OpenAiLiveWindowLabel")}</span>
        <select
          className="provider-detail-field__input"
          value={windowMinutes}
          disabled={disabled || usageBusy}
          onChange={(event) =>
            setWindowMinutes(Number(event.target.value) as LiveWindowMinutes)
          }
        >
          {LIVE_WINDOW_MINUTES.map((minutes) => (
            <option key={minutes} value={minutes}>
              {t(WINDOW_LABELS[minutes])}
            </option>
          ))}
        </select>
      </label>
      <div className="provider-detail-actions">
        <button
          type="button"
          className="credential-btn credential-btn--primary"
          disabled={disabled || usageBusy}
          onClick={loadUsage}
        >
          {usageBusy ? t("OpenAiLiveUsageLoading") : t("OpenAiLiveUsageLoad")}
        </button>
      </div>
      {usageError && (
        <div className="provider-detail-error" role="alert">
          {usageError}
        </div>
      )}
      {usage && (
        <OpenAITokensPerMinuteChart throughput={usage} t={t} limits={limits} showModels />
      )}

      <h5>{t("OpenAiOrgLimitsTitle")}</h5>
      <div className="provider-detail-actions">
        <button
          type="button"
          className="credential-btn"
          disabled={disabled || orgBusy}
          onClick={loadOrg}
        >
          {t("OpenAiOrgLimitsLoad")}
        </button>
      </div>
      {orgScoped && <div className="provider-detail-helper">{t("OpenAiOrgLimitsScoped")}</div>}
      {orgError && (
        <div className="provider-detail-error" role="alert">
          {orgError}
        </div>
      )}
      {org && <OrgLimitsList org={org} t={t} />}
    </section>
  );
}

function OrgLimitsList({ org, t }: { org: OrgRateLimitsSnapshot; t: T }) {
  if (org.byModel.length === 0) {
    return <div className="provider-detail-helper">{t("OpenAiOrgLimitsEmpty")}</div>;
  }
  return (
    <>
      <ul className="openai-tpm__models">
        {org.byModel.map((row) => (
          <li key={row.model}>
            <span className="openai-tpm__model-name">{row.model}</span>
            <span className="openai-tpm__muted">
              {t("OpenAiLimitTpmLabel")}: {formatTokens(row.tokensPerMinute)}
              {row.requestsPerMinute != null &&
                ` · ${t("OpenAiLimitRpmLabel")}: ${formatTokens(row.requestsPerMinute)}`}
            </span>
          </li>
        ))}
      </ul>
      {org.truncated && (
        <div className="provider-detail-helper">{t("OpenAiOrgLimitsTruncated")}</div>
      )}
    </>
  );
}
