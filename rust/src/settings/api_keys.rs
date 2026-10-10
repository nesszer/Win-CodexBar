use super::*;

/// API key storage for providers that need tokens
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ApiKeys {
    /// Provider ID -> API key mapping
    pub keys: HashMap<String, ApiKeyEntry>,
}

/// A single API key entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyEntry {
    pub api_key: String,
    pub saved_at: String,
    /// Optional label for the key (e.g., "Personal", "Work")
    #[serde(default)]
    pub label: Option<String>,
    /// Azure OpenAI API-version override kept alongside the credential.
    /// `None` inherits `AZURE_OPENAI_API_VERSION` and the provider default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_version: Option<String>,
}

impl ApiKeys {
    /// Get the API keys file path (beside a `CODEXBAR_CONFIG` settings file).
    pub fn keys_path() -> Option<PathBuf> {
        config_store_dir().map(|dir| dir.join("api_keys.json"))
    }

    /// Load API keys from disk
    pub fn load() -> Self {
        if let Some(path) = Self::keys_path()
            && path.exists()
            && let Ok(content) = crate::secure_file::read_string(&path)
        {
            return serde_json::from_str(&content).unwrap_or_default();
        }
        Self::default()
    }

    /// Save API keys to disk
    pub fn save(&self) -> anyhow::Result<()> {
        let path = Self::keys_path()
            .ok_or_else(|| anyhow::anyhow!("Could not determine API keys path"))?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let json = serde_json::to_string_pretty(self)?;
        crate::secure_file::write_string(&path, &json)?;

        Ok(())
    }

    /// Get API key for a provider
    pub fn get(&self, provider_id: &str) -> Option<&str> {
        self.keys
            .get(provider_id)
            .map(|e| e.api_key.as_str())
            .filter(|key| !key.is_empty())
    }

    /// Set API key for a provider
    pub fn set(&mut self, provider_id: &str, api_key: &str, label: Option<&str>) {
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string();
        let api_version = self
            .keys
            .get(provider_id)
            .and_then(|entry| entry.api_version.clone());
        self.keys.insert(
            provider_id.to_string(),
            ApiKeyEntry {
                api_key: api_key.to_string(),
                saved_at: now,
                label: label.map(|s| s.to_string()),
                api_version,
            },
        );
    }

    /// Get a provider-specific API-version override, if one is stored.
    pub fn api_version(&self, provider_id: &str) -> Option<&str> {
        self.keys
            .get(provider_id)
            .and_then(|entry| entry.api_version.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    /// Store or clear a provider-specific API-version override.
    ///
    /// The override may be chosen before an API key is saved; it is then kept on a
    /// key-less entry that `get`, `has_key` and `get_all_for_display` ignore.
    pub fn set_api_version(&mut self, provider_id: &str, api_version: Option<String>) {
        let api_version = api_version
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        match self.keys.get_mut(provider_id) {
            Some(entry) => {
                entry.api_version = api_version;
                if entry.api_key.is_empty() && entry.api_version.is_none() {
                    self.keys.remove(provider_id);
                }
            }
            None => {
                if api_version.is_some() {
                    self.keys.insert(
                        provider_id.to_string(),
                        ApiKeyEntry {
                            api_key: String::new(),
                            saved_at: chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string(),
                            label: None,
                            api_version,
                        },
                    );
                }
            }
        }
    }

    /// Remove API key for a provider
    pub fn remove(&mut self, provider_id: &str) {
        self.keys.remove(provider_id);
    }

    /// Check if a provider has an API key configured
    pub fn has_key(&self, provider_id: &str) -> bool {
        self.keys
            .get(provider_id)
            .map(|e| !e.api_key.is_empty())
            .unwrap_or(false)
    }

    /// Get all saved API keys for UI display (with masked values)
    pub fn get_all_for_display(&self) -> Vec<SavedApiKeyInfo> {
        self.keys
            .iter()
            .filter(|(_, entry)| !entry.api_key.is_empty())
            .map(|(id, entry)| {
                let provider_name = ProviderId::from_cli_name(id)
                    .map(|p| p.display_name().to_string())
                    .unwrap_or_else(|| id.clone());

                let masked = mask_api_key(&entry.api_key);

                SavedApiKeyInfo {
                    provider_id: id.clone(),
                    provider: provider_name,
                    masked_key: masked,
                    saved_at: entry.saved_at.clone(),
                    label: entry.label.clone(),
                }
            })
            .collect()
    }
}

fn mask_api_key(api_key: &str) -> String {
    let chars: Vec<char> = api_key.chars().collect();
    if chars.len() > 12 {
        let prefix: String = chars.iter().take(4).collect();
        let suffix: String = chars.iter().skip(chars.len() - 4).collect();
        format!("{prefix}...{suffix}")
    } else if chars.len() > 4 {
        let prefix: String = chars.iter().take(4).collect();
        format!("{prefix}...")
    } else {
        "****".to_string()
    }
}

/// Info about a saved API key for UI display
#[derive(Debug, Clone, Serialize)]
pub struct SavedApiKeyInfo {
    pub provider_id: String,
    pub provider: String,
    pub masked_key: String,
    pub saved_at: String,
    pub label: Option<String>,
}

/// Provider configuration info
#[derive(Debug, Clone)]
pub struct ProviderConfigInfo {
    pub id: ProviderId,
    pub name: &'static str,
    pub api_key_env_var: Option<&'static str>,
    pub api_key_help: Option<&'static str>,
    pub dashboard_url: Option<&'static str>,
}

/// Get configuration info for providers that need API keys
pub fn get_api_key_providers() -> Vec<ProviderConfigInfo> {
    vec![
        ProviderConfigInfo {
            id: ProviderId::Alibaba,
            name: "Alibaba Coding Plan",
            api_key_env_var: Some("ALIBABA_CODING_PLAN_API_KEY"),
            api_key_help: Some("Get your Coding Plan API key from Alibaba Model Studio / Bailian"),
            dashboard_url: Some(
                "https://modelstudio.console.alibabacloud.com/ap-southeast-1/?tab=coding-plan#/efm/detail",
            ),
        },
        ProviderConfigInfo {
            id: ProviderId::Amp,
            name: "Amp (Sourcegraph)",
            api_key_env_var: Some("SRC_ACCESS_TOKEN"),
            api_key_help: Some("Get your token from Sourcegraph → Settings → Access Tokens"),
            dashboard_url: Some("https://sourcegraph.com/cody/manage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Copilot,
            name: "GitHub Copilot (legacy token)",
            api_key_env_var: Some("GITHUB_TOKEN"),
            api_key_help: Some(
                "Optional fallback. Prefer Providers → Copilot → Sign in with GitHub.",
            ),
            dashboard_url: Some("https://github.com/settings/copilot"),
        },
        ProviderConfigInfo {
            id: ProviderId::Zai,
            name: "z.ai",
            api_key_env_var: Some("Z_AI_API_KEY or ZAI_API_TOKEN"),
            api_key_help: Some(
                "Get your API token from z.ai Dashboard. BigModel team usage can set Z_AI_BIGMODEL_ORGANIZATION + Z_AI_BIGMODEL_PROJECT, or provider workspace_id as organization|project.",
            ),
            dashboard_url: Some("https://z.ai/manage-apikey/coding-plan/personal/my-plan"),
        },
        ProviderConfigInfo {
            id: ProviderId::Warp,
            name: "Warp",
            api_key_env_var: Some("WARP_API_KEY"),
            api_key_help: Some(
                "Get your API key from Warp → Settings → API Keys (docs.warp.dev/reference/cli/api-keys)",
            ),
            dashboard_url: Some("https://docs.warp.dev/reference/cli/api-keys"),
        },
        ProviderConfigInfo {
            id: ProviderId::Ollama,
            name: "Ollama",
            api_key_env_var: Some("OLLAMA_API_KEY / OLLAMA_KEY"),
            api_key_help: Some(
                "Optional: use an Ollama API key for Cloud validation, or browser cookies for usage.",
            ),
            dashboard_url: Some("https://ollama.com/settings"),
        },
        ProviderConfigInfo {
            id: ProviderId::MiniMax,
            name: "MiniMax",
            api_key_env_var: Some("MINIMAX_API_KEY"),
            api_key_help: Some(
                "Optional: a MiniMax API key reads real coding-plan quota via the console's remains API, bypassing the client-rendered usage/plan pages that browser cookies alone cannot scrape.",
            ),
            dashboard_url: Some(
                "https://platform.minimax.io/user-center/basic-information/interface-key",
            ),
        },
        ProviderConfigInfo {
            id: ProviderId::AzureOpenAI,
            name: "Azure OpenAI",
            api_key_env_var: Some(
                "AZURE_OPENAI_API_KEY + AZURE_OPENAI_ENDPOINT + AZURE_OPENAI_DEPLOYMENT",
            ),
            api_key_help: Some(
                "Use env vars, or save JSON {api_key, endpoint, deployment, api_version}; composite api_key|endpoint|deployment[|api_version] also works.",
            ),
            dashboard_url: Some("https://ai.azure.com"),
        },
        ProviderConfigInfo {
            id: ProviderId::OpenRouter,
            name: "OpenRouter",
            api_key_env_var: Some("OPENROUTER_API_KEY"),
            api_key_help: Some(
                "Required. Enter a regular API key or a Management API key here. Management keys also enable account Activity on the official OpenRouter API.",
            ),
            dashboard_url: Some("https://openrouter.ai/settings/credits"),
        },
        ProviderConfigInfo {
            id: ProviderId::NanoGPT,
            name: "NanoGPT",
            api_key_env_var: Some("NANOGPT_API_KEY"),
            api_key_help: Some("Get your API key from nano-gpt.com/api"),
            dashboard_url: Some("https://nano-gpt.com/api"),
        },
        ProviderConfigInfo {
            id: ProviderId::Infini,
            name: "Infini AI",
            api_key_env_var: Some("INFINI_API_KEY"),
            api_key_help: Some("Get your API key from Infini Cloud → Settings → API Keys"),
            dashboard_url: Some("https://cloud.infini-ai.com"),
        },
        ProviderConfigInfo {
            id: ProviderId::Kimi,
            name: "Kimi Code API",
            api_key_env_var: Some("KIMI_CODE_API_KEY"),
            api_key_help: Some(
                "Get your Kimi Code API key from Kimi. Optional HTTPS proxy base URL: KIMI_CODE_BASE_URL.",
            ),
            dashboard_url: Some("https://platform.moonshot.cn/console/api-keys"),
        },
        ProviderConfigInfo {
            id: ProviderId::Kilo,
            name: "Kilo",
            api_key_env_var: Some("KILO_API_KEY"),
            api_key_help: Some("Get your API key from Kilo, or run `kilo auth login`."),
            dashboard_url: Some("https://app.kilo.ai/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Bedrock,
            name: "AWS Bedrock",
            api_key_env_var: Some(
                "AWS_ACCESS_KEY_ID:AWS_SECRET_ACCESS_KEY[:AWS_SESSION_TOKEN] or AWS_PROFILE",
            ),
            api_key_help: Some(
                "Paste access_key:secret_key[:session_token], JSON credentials, profile:name, or use AWS env vars/AWS CLI profiles.",
            ),
            dashboard_url: Some("https://console.aws.amazon.com/bedrock"),
        },
        ProviderConfigInfo {
            id: ProviderId::Codebuff,
            name: "Codebuff",
            api_key_env_var: Some("CODEBUFF_API_KEY"),
            api_key_help: Some(
                "Get your API key from Codebuff, or sign in with Codebuff/Manicode.",
            ),
            dashboard_url: Some("https://www.codebuff.com/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::DeepSeek,
            name: "DeepSeek",
            api_key_env_var: Some("DEEPSEEK_API_KEY"),
            api_key_help: Some("Get your API key from platform.deepseek.com."),
            dashboard_url: Some("https://platform.deepseek.com/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::DeepInfra,
            name: "DeepInfra",
            api_key_env_var: Some("DEEPINFRA_API_KEY"),
            api_key_help: Some(
                "Get your API key from deepinfra.com/dash. Also accepts DEEPINFRA_TOKEN.",
            ),
            dashboard_url: Some("https://deepinfra.com/dash"),
        },
        ProviderConfigInfo {
            id: ProviderId::HuggingFace,
            name: "Hugging Face",
            api_key_env_var: Some(
                "CODEXBAR_HUGGINGFACE_API_KEY / HF_TOKEN / HUGGING_FACE_HUB_TOKEN",
            ),
            api_key_help: Some(
                "Add a Hugging Face access token here, set HF_TOKEN, or run `hf auth login`.",
            ),
            dashboard_url: Some("https://huggingface.co/settings/billing"),
        },
        ProviderConfigInfo {
            id: ProviderId::V0,
            name: "v0",
            api_key_env_var: Some("V0_API_KEY"),
            api_key_help: Some(
                "Add a v0 Platform API key. An optional scope can use the provider workspace field or V0_SCOPE.",
            ),
            dashboard_url: Some("https://v0.app/settings/billing"),
        },
        ProviderConfigInfo {
            id: ProviderId::Fireworks,
            name: "Fireworks",
            api_key_env_var: Some("FIREWORKS_API_KEY"),
            api_key_help: Some(
                "Get your API key from app.fireworks.ai. Also set the account slug from \
                 app.fireworks.ai/accounts/<slug> (FIREWORKS_ACCOUNT_SLUG).",
            ),
            dashboard_url: Some("https://app.fireworks.ai"),
        },
        ProviderConfigInfo {
            id: ProviderId::AiAnd,
            name: "ai&",
            api_key_env_var: Some("AIAND_API_KEY"),
            api_key_help: Some("Get your API key from console.aiand.com."),
            dashboard_url: Some("https://console.aiand.com"),
        },
        ProviderConfigInfo {
            id: ProviderId::AtlasCloud,
            name: "Atlas Cloud",
            api_key_env_var: Some("ATLASCLOUD_API_KEY"),
            api_key_help: Some(
                "Get an API key from Atlas Cloud and set it in Preferences or ATLASCLOUD_API_KEY.",
            ),
            dashboard_url: Some(crate::providers::atlascloud::DASHBOARD_URL),
        },
        ProviderConfigInfo {
            id: ProviderId::ZenMux,
            name: "ZenMux",
            api_key_env_var: Some("ZENMUX_MANAGEMENT_API_KEY"),
            api_key_help: Some(
                "Use a ZenMux Management API key (not an inference key). Also accepts ZENMUX_API_KEY.",
            ),
            dashboard_url: Some("https://zenmux.ai/platform/management"),
        },
        ProviderConfigInfo {
            id: ProviderId::ClinePass,
            name: "ClinePass",
            api_key_env_var: Some("CLINE_API_KEY"),
            api_key_help: Some(
                "Paste an API key, or run cline auth. Reads the existing Cline session without copying it. Also accepts CLINEPASS_API_KEY.",
            ),
            dashboard_url: Some("https://app.cline.bot/dashboard/subscription?personal=true"),
        },
        ProviderConfigInfo {
            id: ProviderId::Neuralwatt,
            name: "Neuralwatt",
            api_key_env_var: Some("NEURALWATT_API_KEY"),
            api_key_help: Some("Get your API key from portal.neuralwatt.com."),
            dashboard_url: Some("https://portal.neuralwatt.com/dashboard"),
        },
        ProviderConfigInfo {
            id: ProviderId::DevPass,
            name: "DevPass",
            api_key_env_var: Some("DEVPASS_API_KEY"),
            api_key_help: Some(
                "Use a regular LLM Gateway API key. Publishable keys and end-user sessions cannot read plan state.",
            ),
            dashboard_url: Some("https://devpass.llmgateway.io/dashboard"),
        },
        ProviderConfigInfo {
            id: ProviderId::XKiro,
            name: "xKiro",
            api_key_env_var: Some("XKIRO_API_KEY"),
            api_key_help: Some(
                "Create an API key at xkiro.com. It is sent only to api.xkiro.com and reads the free usage endpoint.",
            ),
            dashboard_url: Some("https://xkiro.com"),
        },
        ProviderConfigInfo {
            id: ProviderId::Vercel,
            name: "Vercel AI Gateway",
            api_key_env_var: Some("AI_GATEWAY_API_KEY"),
            api_key_help: Some(
                "Add a Vercel AI Gateway API key to show the team's credit balance and lifetime spend.",
            ),
            dashboard_url: Some("https://vercel.com/d?to=%2F%5Bteam%5D%2F%7E%2Fai-gateway"),
        },
        ProviderConfigInfo {
            id: ProviderId::Doubao,
            name: "Doubao / Volcengine Ark",
            api_key_env_var: Some(
                "ARK_API_KEY or VOLCENGINE_ACCESS_KEY_ID + VOLCENGINE_SECRET_ACCESS_KEY",
            ),
            api_key_help: Some(
                "Use ARK_API_KEY for chat probe fallback, or paste Coding Plan credentials as access_key|secret_key|region (region defaults to cn-beijing).",
            ),
            dashboard_url: Some("https://console.volcengine.com/ark/region:ark+cn-beijing/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::StepFun,
            name: "StepFun",
            api_key_env_var: Some("STEPFUN_OASIS_TOKEN"),
            api_key_help: Some("Paste an existing Oasis-Token from StepFun login."),
            dashboard_url: Some("https://platform.stepfun.com/dashboard"),
        },
        ProviderConfigInfo {
            id: ProviderId::Venice,
            name: "Venice",
            api_key_env_var: Some("VENICE_API_KEY"),
            api_key_help: Some("Get your API key from Venice settings."),
            dashboard_url: Some("https://venice.ai/settings/api"),
        },
        ProviderConfigInfo {
            id: ProviderId::OpenAIApi,
            name: "OpenAI",
            api_key_env_var: Some("OPENAI_ADMIN_KEY / OPENAI_API_KEY"),
            api_key_help: Some(
                "Use an OpenAI Admin API key for usage, or a platform key for legacy billing balance.",
            ),
            dashboard_url: Some("https://platform.openai.com/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Grok,
            name: "Grok",
            api_key_env_var: None,
            api_key_help: Some("Uses Grok browser cookies or ~/.grok/auth.json."),
            dashboard_url: Some("https://grok.com/settings/subscription"),
        },
        ProviderConfigInfo {
            id: ProviderId::Xai,
            name: "xAI",
            api_key_env_var: Some("XAI_MANAGEMENT_API_KEY"),
            api_key_help: Some(
                "Create a Management API key at console.x.ai under Settings > Management Keys (inference keys are rejected). Team ID goes in provider workspace settings or XAI_TEAM_ID.",
            ),
            dashboard_url: Some("https://console.x.ai"),
        },
        ProviderConfigInfo {
            id: ProviderId::ElevenLabs,
            name: "ElevenLabs",
            api_key_env_var: Some("ELEVENLABS_API_KEY / XI_API_KEY"),
            api_key_help: Some("Get your API key from ElevenLabs Settings > API Keys."),
            dashboard_url: Some("https://elevenlabs.io/app/settings/api-keys"),
        },
        ProviderConfigInfo {
            id: ProviderId::Deepgram,
            name: "Deepgram",
            api_key_env_var: Some("DEEPGRAM_API_KEY"),
            api_key_help: Some("Use a Deepgram API key with Management API access."),
            dashboard_url: Some("https://console.deepgram.com/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Groq,
            name: "Groq",
            api_key_env_var: Some("GROQ_API_KEY"),
            api_key_help: Some(
                "Usage & spend come from your console.groq.com browser session automatically. \
                 An API key is optional and only adds Enterprise Prometheus metrics.",
            ),
            dashboard_url: Some("https://console.groq.com/settings/metrics"),
        },
        ProviderConfigInfo {
            id: ProviderId::LLMProxy,
            name: "LLM Proxy",
            api_key_env_var: Some("LLM_PROXY_API_KEY + LLM_PROXY_BASE_URL"),
            api_key_help: Some(
                "Set an LLM Proxy API key and base URL (Settings or LLM_PROXY_BASE_URL) for quota-stats.",
            ),
            dashboard_url: None,
        },
        ProviderConfigInfo {
            id: ProviderId::Chutes,
            name: "Chutes",
            api_key_env_var: Some("CHUTES_API_KEY"),
            api_key_help: Some(
                "Paste a Chutes API key. Optional API URL override: CHUTES_API_URL.",
            ),
            dashboard_url: Some("https://chutes.ai"),
        },
        ProviderConfigInfo {
            id: ProviderId::LiteLLM,
            name: "LiteLLM",
            api_key_env_var: Some("LITELLM_API_KEY + LITELLM_BASE_URL"),
            api_key_help: Some(
                "Paste a LiteLLM key and set the base URL in provider extras or LITELLM_BASE_URL.",
            ),
            dashboard_url: None,
        },
        ProviderConfigInfo {
            id: ProviderId::LLMMan,
            name: "llmman",
            api_key_env_var: Some("LLMMAN_API_KEY + LLMMAN_HOST"),
            api_key_help: Some(
                "Optional: an open local daemon needs no key. Set the base URL in provider extras or LLMMAN_HOST (default http://127.0.0.1:17434).",
            ),
            dashboard_url: None,
        },
        ProviderConfigInfo {
            id: ProviderId::Poe,
            name: "Poe",
            api_key_env_var: Some("POE_API_KEY"),
            api_key_help: Some("Get your API key from Poe API settings."),
            dashboard_url: Some("https://poe.com/settings/subscription"),
        },
        ProviderConfigInfo {
            id: ProviderId::Devin,
            name: "Devin",
            api_key_env_var: Some("DEVIN_BEARER_TOKEN + DEVIN_ORG"),
            api_key_help: Some(
                "Paste a Devin bearer token and set the organization in provider extras or DEVIN_ORG.",
            ),
            dashboard_url: Some("https://app.devin.ai/settings/billing"),
        },
        ProviderConfigInfo {
            id: ProviderId::Zed,
            name: "Zed",
            api_key_env_var: Some("ZED_CREDENTIALS"),
            api_key_help: Some(
                "Paste Zed credentials as `user_id access_token`; optional API URL in provider extras.",
            ),
            dashboard_url: Some("https://zed.dev/account"),
        },
        ProviderConfigInfo {
            id: ProviderId::CrossModel,
            name: "CrossModel",
            api_key_env_var: Some("CROSSMODEL_API_KEY"),
            api_key_help: Some(
                "Paste a CrossModel API key. Optional API URL override: CROSSMODEL_API_URL.",
            ),
            dashboard_url: Some("https://crossmodel.ai"),
        },
        ProviderConfigInfo {
            id: ProviderId::Sub2Api,
            name: "sub2api",
            api_key_env_var: Some("SUB2API_API_KEY"),
            api_key_help: Some(
                "Paste a group API key and set the base URL in provider extras or SUB2API_BASE_URL. HTTPS required (loopback HTTP allowed for local dev).",
            ),
            dashboard_url: None,
        },
        ProviderConfigInfo {
            id: ProviderId::Factory,
            name: "Droid (Factory)",
            api_key_env_var: Some("FACTORY_API_KEY"),
            api_key_help: Some(
                "Get your API key from Factory → Settings → API Keys. Optional fallback: %USERPROFILE%\\.factory\\.env. Auto mode tries the key first, then browser cookies.",
            ),
            dashboard_url: Some("https://app.factory.ai/settings/api-keys"),
        },
        ProviderConfigInfo {
            id: ProviderId::Meta,
            name: "Meta",
            api_key_env_var: Some("MODEL_API_KEY / META_API_KEY"),
            api_key_help: Some("Create key in Meta Model API dashboard"),
            dashboard_url: Some("https://dev.meta.ai/docs"),
        },
        ProviderConfigInfo {
            id: ProviderId::Hyper,
            name: "Charm Hyper",
            api_key_env_var: Some("HYPER_API_KEY"),
            api_key_help: Some(
                "Fallback when no hyper.charm.land session is available. Save an API key here or set HYPER_API_KEY.",
            ),
            dashboard_url: Some("https://hyper.charm.land"),
        },
        ProviderConfigInfo {
            id: ProviderId::GitKraken,
            name: "GitKraken AI",
            api_key_env_var: Some("GITKRAKEN_API_TOKEN"),
            api_key_help: Some(
                "Save a GitKraken access token. Optional organization ID: provider extras or GITKRAKEN_ORG_ID.",
            ),
            dashboard_url: Some("https://gitkraken.dev/account#ai-usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Bifrost,
            name: "Bifrost",
            api_key_env_var: Some("BIFROST_API_KEY"),
            api_key_help: Some(
                "Save a Bifrost virtual key and configure the gateway base URL in provider settings. Or set BIFROST_API_KEY and BIFROST_BASE_URL.",
            ),
            dashboard_url: None,
        },
        ProviderConfigInfo {
            id: ProviderId::Aixy,
            name: "Aixy",
            api_key_env_var: Some("AIXY_API_KEY"),
            api_key_help: Some(
                "Save an Aixy API key. Leave the Base URL empty for the hosted gateway, or set it for a self-hosted one. Or set AIXY_API_KEY and AIXY_BASE_URL.",
            ),
            dashboard_url: Some("https://dash.aixy-gateway.com"),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::{ApiKeys, get_api_key_providers};
    use crate::core::ProviderId;

    #[test]
    fn elevenlabs_lists_both_api_key_environment_names() {
        let provider = get_api_key_providers()
            .into_iter()
            .find(|provider| provider.id == ProviderId::ElevenLabs)
            .unwrap();
        assert_eq!(
            provider.api_key_env_var,
            Some("ELEVENLABS_API_KEY / XI_API_KEY")
        );
    }

    #[test]
    fn api_version_survives_api_key_update() {
        let mut keys = ApiKeys::default();
        keys.set("azureopenai", "key", Some("work"));
        keys.set_api_version("azureopenai", Some("v1".to_string()));
        keys.set("azureopenai", "new-key", None);

        assert_eq!(keys.get("azureopenai"), Some("new-key"));
        assert_eq!(keys.api_version("azureopenai"), Some("v1"));
    }

    #[test]
    fn api_version_set_before_api_key_is_kept() {
        let mut keys = ApiKeys::default();
        keys.set_api_version("azureopenai", Some("v1".to_string()));

        assert_eq!(keys.api_version("azureopenai"), Some("v1"));
        assert_eq!(keys.get("azureopenai"), None);
        assert!(!keys.has_key("azureopenai"));
        assert!(keys.get_all_for_display().is_empty());

        keys.set("azureopenai", "key", None);
        assert_eq!(keys.get("azureopenai"), Some("key"));
        assert_eq!(keys.api_version("azureopenai"), Some("v1"));
    }

    #[test]
    fn clearing_api_version_without_api_key_drops_the_entry() {
        let mut keys = ApiKeys::default();
        keys.set_api_version("azureopenai", Some("v1".to_string()));
        keys.set_api_version("azureopenai", None);

        assert!(keys.keys.is_empty());
    }

    #[test]
    fn clearing_api_version_removes_the_override() {
        let mut keys = ApiKeys::default();
        keys.set("azureopenai", "key", None);
        keys.set_api_version("azureopenai", Some("2025-01-01".to_string()));
        keys.set_api_version("azureopenai", None);

        assert_eq!(keys.api_version("azureopenai"), None);
    }
}
