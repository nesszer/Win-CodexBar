import { useEffect, useState } from "react";
import type { LocaleKey } from "../../../../../i18n/keys";
import {
  getProviderWorkspaceId,
  setProviderWorkspaceId,
} from "../../../../../lib/tauri";

/** Fired on `window` after a workspace ID is saved; `detail` is the provider id. */
export const PROVIDER_WORKSPACE_ID_SAVED_EVENT = "codexbar:provider-workspace-id-saved";

interface Props {
  providerId?: string;
  t: (key: LocaleKey) => string;
}

/**
 * Workspace, organization or base-URL field for API providers that need one
 * next to their key (OpenAI API project ID, LiteLLM base URL, and so on).
 * Renders nothing for other providers.
 */
export function OpenAiExtras({ providerId = "openaiapi", t }: Props) {
  const [projectId, setProjectId] = useState("");
  const [savedProjectId, setSavedProjectId] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const config = extraConfig(providerId, t);

  useEffect(() => {
    if (!WORKSPACE_EXTRA_IDS[providerId]) return;
    let cancelled = false;
    void getProviderWorkspaceId(providerId)
      .then((value) => {
        if (!cancelled) {
          setProjectId(value ?? "");
          setSavedProjectId(value ?? "");
        }
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [providerId]);

  const saveProjectId = async () => {
    setBusy(true);
    setError(null);
    try {
      const next = projectId.trim();
      await setProviderWorkspaceId(providerId, next);
      setSavedProjectId(next);
      window.dispatchEvent(
        new CustomEvent<string>(PROVIDER_WORKSPACE_ID_SAVED_EVENT, { detail: providerId }),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (config) {
    return (
      <section className="provider-detail-section">
        <h4>{config.title}</h4>
        <label className="provider-detail-field">
          <span className="provider-detail-field__label">
            {config.label}
          </span>
          <input
            className="provider-detail-field__input"
            value={projectId}
            placeholder={config.placeholder}
            spellCheck={false}
            onChange={(event) => setProjectId(event.target.value)}
          />
        </label>
        <div className="provider-detail-helper">
          {config.help}
        </div>
        <div className="provider-detail-actions">
          <button
            type="button"
            className="credential-btn credential-btn--primary"
            disabled={busy || projectId.trim() === savedProjectId}
            onClick={saveProjectId}
          >
            {t("Save")}
          </button>
        </div>
        {error && <div className="provider-detail-error">{error}</div>}
      </section>
    );
  }

  return null;
}

/** Providers that show a workspace, organization or base-URL field next to their key. */
export function hasWorkspaceExtra(providerId: string): boolean {
  return WORKSPACE_EXTRA_IDS[providerId] === true;
}

const WORKSPACE_EXTRA_IDS: Record<string, true> = {
  openaiapi: true,
  litellm: true,
  llmman: true,
  devin: true,
  opencodego: true,
  zed: true,
  sub2api: true,
  llmproxy: true,
  xai: true,
  fireworks: true,
  gitkraken: true,
  exa: true,
  cosmic: true,
  flora: true,
};

function extraConfig(providerId: string, t: Props["t"]) {
  switch (providerId) {
    case "openaiapi":
      return {
        title: t("OpenAiAdminApiTitle"),
        label: t("OpenAiProjectIdLabel"),
        placeholder: t("OpenAiProjectIdPlaceholder"),
        help: t("OpenAiProjectIdHelp"),
      };
    case "litellm":
      return {
        title: t("LiteLlmApiTitle"),
        label: t("LiteLlmBaseUrlLabel"),
        placeholder: t("LiteLlmBaseUrlPlaceholder"),
        help: t("LiteLlmBaseUrlHelp"),
      };
    case "llmman":
      return {
        title: t("LLMManApiTitle"),
        label: t("LLMManBaseUrlLabel"),
        placeholder: t("LLMManBaseUrlPlaceholder"),
        help: t("LLMManBaseUrlHelp"),
      };
    case "devin":
      return {
        title: t("DevinApiTitle"),
        label: t("DevinOrganizationLabel"),
        placeholder: t("DevinOrganizationPlaceholder"),
        help: t("DevinOrganizationHelp"),
      };
    case "opencodego":
      return {
        title: t("OpenCodeGoWorkspaceTitle"),
        label: t("OpenCodeGoWorkspaceLabel"),
        placeholder: "wrk_...",
        help: t("OpenCodeGoWorkspaceHelp"),
      };
    case "zed":
      return {
        title: t("ZedApiTitle"),
        label: t("ZedApiUrlLabel"),
        placeholder: t("ZedApiUrlPlaceholder"),
        help: t("ZedApiUrlHelp"),
      };
    case "sub2api":
      return {
        title: t("Sub2ApiTitle"),
        label: t("Sub2ApiBaseUrlLabel"),
        placeholder: t("Sub2ApiBaseUrlPlaceholder"),
        help: t("Sub2ApiBaseUrlHelp"),
      };
    case "llmproxy":
      return {
        title: t("LlmProxyTitle"),
        label: t("LlmProxyBaseUrlLabel"),
        placeholder: t("LlmProxyBaseUrlPlaceholder"),
        help: t("LlmProxyBaseUrlHelp"),
      };
    case "xai":
      return {
        title: "xAI team",
        label: "Team ID",
        placeholder: "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
        help: "Required. Shown in the xAI Console URL and team settings. Or set XAI_TEAM_ID. Pair with a Management API key (not an inference key).",
      };
    case "fireworks":
      return {
        title: "Fireworks account",
        label: "Account slug",
        placeholder: "your-account-slug",
        help: "From app.fireworks.ai/accounts/<slug>. Or set FIREWORKS_ACCOUNT_SLUG. Pair with a Fireworks API key to read 30-day rated billing spend.",
      };
    case "gitkraken":
      return {
        title: "GitKraken organization",
        label: "Organization ID (optional)",
        placeholder: "organization-id",
        help: "Adds the organization ID to shared-pool usage requests. Or set GITKRAKEN_ORG_ID.",
      };
    case "exa":
      return {
        title: t("ExaApiTitle"),
        label: t("ExaApiKeyIdLabel"),
        placeholder: t("ExaApiKeyIdPlaceholder"),
        help: t("ExaApiKeyIdHelp"),
      };
    case "cosmic":
      return {
        title: t("CosmicProjectIdTitle"),
        label: t("CosmicProjectIdLabel"),
        placeholder: t("CosmicProjectIdPlaceholder"),
        help: t("CosmicProjectIdHelp"),
      };
    case "flora":
      return {
        title: "FLORA workspace",
        label: "Workspace ID",
        placeholder: "ws_…",
        help: "Required. The ws_… ID of the workspace the key can bill. Or set FLORA_WORKSPACE_ID.",
      };
    default:
      return null;
  }
}
