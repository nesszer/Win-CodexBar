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
}

impl ApiKeys {
    /// Get the API keys file path
    pub fn keys_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("CodexBar").join("api_keys.json"))
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
        self.keys.get(provider_id).map(|e| e.api_key.as_str())
    }

    /// Set API key for a provider
    pub fn set(&mut self, provider_id: &str, api_key: &str, label: Option<&str>) {
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M").to_string();
        self.keys.insert(
            provider_id.to_string(),
            ApiKeyEntry {
                api_key: api_key.to_string(),
                saved_at: now,
                label: label.map(|s| s.to_string()),
            },
        );
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
            .map(|(id, entry)| {
                let provider_name = ProviderId::from_cli_name(id)
                    .map(|p| p.display_name().to_string())
                    .unwrap_or_else(|| id.clone());

                // Mask the key for display (show first 4 and last 4 chars)
                let masked = if entry.api_key.len() > 12 {
                    format!(
                        "{}...{}",
                        &entry.api_key[..4],
                        &entry.api_key[entry.api_key.len() - 4..]
                    )
                } else if entry.api_key.len() > 4 {
                    format!("{}...", &entry.api_key[..4])
                } else {
                    "****".to_string()
                };

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
    pub requires_api_key: bool,
    pub api_key_env_var: Option<&'static str>,
    pub api_key_help: Option<&'static str>,
    pub config_file_path: Option<&'static str>,
    pub dashboard_url: Option<&'static str>,
}

/// Get configuration info for providers that need API keys
pub fn get_api_key_providers() -> Vec<ProviderConfigInfo> {
    vec![
        ProviderConfigInfo {
            id: ProviderId::Alibaba,
            name: "Alibaba Coding Plan",
            requires_api_key: true,
            api_key_env_var: Some("ALIBABA_CODING_PLAN_API_KEY"),
            api_key_help: Some("Get your Coding Plan API key from Alibaba Model Studio / Bailian"),
            config_file_path: Some("~/.codexbar/config.json"),
            dashboard_url: Some(
                "https://modelstudio.console.alibabacloud.com/ap-southeast-1/?tab=coding-plan#/efm/detail",
            ),
        },
        ProviderConfigInfo {
            id: ProviderId::Amp,
            name: "Amp (Sourcegraph)",
            requires_api_key: true,
            api_key_env_var: Some("SRC_ACCESS_TOKEN"),
            api_key_help: Some("Get your token from Sourcegraph → Settings → Access Tokens"),
            config_file_path: Some("~/.amp/config.json"),
            dashboard_url: Some("https://sourcegraph.com/cody/manage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Synthetic,
            name: "Synthetic",
            requires_api_key: true,
            api_key_env_var: Some("SYNTHETIC_API_KEY"),
            api_key_help: Some("Get your API key from Synthetic → Account → API Keys"),
            config_file_path: Some("~/.synthetic/config.json"),
            dashboard_url: Some("https://synthetic.computer/account"),
        },
        ProviderConfigInfo {
            id: ProviderId::Copilot,
            name: "GitHub Copilot (legacy token)",
            requires_api_key: true,
            api_key_env_var: Some("GITHUB_TOKEN"),
            api_key_help: Some(
                "Optional fallback. Prefer Providers → Copilot → Sign in with GitHub.",
            ),
            config_file_path: None,
            dashboard_url: Some("https://github.com/settings/copilot"),
        },
        ProviderConfigInfo {
            id: ProviderId::Zai,
            name: "z.ai",
            requires_api_key: true,
            api_key_env_var: Some("ZAI_API_TOKEN"),
            api_key_help: Some("Get your API token from z.ai Dashboard → Settings"),
            config_file_path: None,
            dashboard_url: Some("https://z.ai/dashboard"),
        },
        ProviderConfigInfo {
            id: ProviderId::Warp,
            name: "Warp",
            requires_api_key: true,
            api_key_env_var: Some("WARP_API_KEY"),
            api_key_help: Some(
                "Get your API key from Warp → Settings → API Keys (docs.warp.dev/reference/cli/api-keys)",
            ),
            config_file_path: None,
            dashboard_url: Some("https://docs.warp.dev/reference/cli/api-keys"),
        },
        ProviderConfigInfo {
            id: ProviderId::Ollama,
            name: "Ollama",
            requires_api_key: false,
            api_key_env_var: Some("OLLAMA_API_KEY / OLLAMA_KEY"),
            api_key_help: Some(
                "Optional: use an Ollama API key for Cloud validation, or browser cookies for usage.",
            ),
            config_file_path: None,
            dashboard_url: Some("https://ollama.com/settings"),
        },
        ProviderConfigInfo {
            id: ProviderId::AzureOpenAI,
            name: "Azure OpenAI",
            requires_api_key: true,
            api_key_env_var: Some(
                "AZURE_OPENAI_API_KEY + AZURE_OPENAI_ENDPOINT + AZURE_OPENAI_DEPLOYMENT",
            ),
            api_key_help: Some(
                "Use env vars, or save JSON {api_key, endpoint, deployment, api_version}; composite api_key|endpoint|deployment[|api_version] also works.",
            ),
            config_file_path: Some("~/.codexbar/api_keys.json"),
            dashboard_url: Some("https://ai.azure.com"),
        },
        ProviderConfigInfo {
            id: ProviderId::OpenRouter,
            name: "OpenRouter",
            requires_api_key: true,
            api_key_env_var: Some("OPENROUTER_API_KEY"),
            api_key_help: Some("Get your API key from openrouter.ai/settings/keys"),
            config_file_path: None,
            dashboard_url: Some("https://openrouter.ai/settings/credits"),
        },
        ProviderConfigInfo {
            id: ProviderId::NanoGPT,
            name: "NanoGPT",
            requires_api_key: true,
            api_key_env_var: Some("NANOGPT_API_KEY"),
            api_key_help: Some("Get your API key from nano-gpt.com/api"),
            config_file_path: None,
            dashboard_url: Some("https://nano-gpt.com/api"),
        },
        ProviderConfigInfo {
            id: ProviderId::Infini,
            name: "Infini AI",
            requires_api_key: true,
            api_key_env_var: Some("INFINI_API_KEY"),
            api_key_help: Some("Get your API key from Infini Cloud → Settings → API Keys"),
            config_file_path: None,
            dashboard_url: Some("https://cloud.infini-ai.com"),
        },
        ProviderConfigInfo {
            id: ProviderId::Kimi,
            name: "Kimi Code API",
            requires_api_key: true,
            api_key_env_var: Some("KIMI_CODE_API_KEY"),
            api_key_help: Some(
                "Get your Kimi Code API key from Kimi. Optional HTTPS proxy base URL: KIMI_CODE_BASE_URL.",
            ),
            config_file_path: None,
            dashboard_url: Some("https://platform.moonshot.cn/console/api-keys"),
        },
        ProviderConfigInfo {
            id: ProviderId::Kilo,
            name: "Kilo",
            requires_api_key: true,
            api_key_env_var: Some("KILO_API_KEY"),
            api_key_help: Some("Get your API key from Kilo, or sign in with Kilo CLI."),
            config_file_path: Some("~/.local/share/kilo/auth.json"),
            dashboard_url: Some("https://app.kilo.ai/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Bedrock,
            name: "AWS Bedrock",
            requires_api_key: true,
            api_key_env_var: Some(
                "AWS_ACCESS_KEY_ID:AWS_SECRET_ACCESS_KEY[:AWS_SESSION_TOKEN] or AWS_PROFILE",
            ),
            api_key_help: Some(
                "Paste access_key:secret_key[:session_token], JSON credentials, profile:name, or use AWS env vars/AWS CLI profiles.",
            ),
            config_file_path: None,
            dashboard_url: Some("https://console.aws.amazon.com/bedrock"),
        },
        ProviderConfigInfo {
            id: ProviderId::Codebuff,
            name: "Codebuff",
            requires_api_key: true,
            api_key_env_var: Some("CODEBUFF_API_KEY"),
            api_key_help: Some(
                "Get your API key from Codebuff, or sign in with Codebuff/Manicode.",
            ),
            config_file_path: Some("~/.config/manicode/credentials.json"),
            dashboard_url: Some("https://www.codebuff.com/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::DeepSeek,
            name: "DeepSeek",
            requires_api_key: true,
            api_key_env_var: Some("DEEPSEEK_API_KEY"),
            api_key_help: Some("Get your API key from platform.deepseek.com."),
            config_file_path: None,
            dashboard_url: Some("https://platform.deepseek.com/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Doubao,
            name: "Doubao / Volcengine Ark",
            requires_api_key: true,
            api_key_env_var: Some("ARK_API_KEY"),
            api_key_help: Some("Get your API key from Volcengine Ark."),
            config_file_path: None,
            dashboard_url: Some("https://console.volcengine.com/ark/region:ark+cn-beijing/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Crof,
            name: "Crof",
            requires_api_key: true,
            api_key_env_var: Some("CROF_API_KEY"),
            api_key_help: Some("Get your API key from Crof."),
            config_file_path: None,
            dashboard_url: Some("https://crof.ai"),
        },
        ProviderConfigInfo {
            id: ProviderId::StepFun,
            name: "StepFun",
            requires_api_key: true,
            api_key_env_var: Some("STEPFUN_OASIS_TOKEN"),
            api_key_help: Some("Paste an existing Oasis-Token from StepFun login."),
            config_file_path: None,
            dashboard_url: Some("https://platform.stepfun.com/dashboard"),
        },
        ProviderConfigInfo {
            id: ProviderId::Venice,
            name: "Venice",
            requires_api_key: true,
            api_key_env_var: Some("VENICE_API_KEY"),
            api_key_help: Some("Get your API key from Venice settings."),
            config_file_path: None,
            dashboard_url: Some("https://venice.ai/settings/api"),
        },
        ProviderConfigInfo {
            id: ProviderId::OpenAIApi,
            name: "OpenAI",
            requires_api_key: true,
            api_key_env_var: Some("OPENAI_ADMIN_KEY / OPENAI_API_KEY"),
            api_key_help: Some(
                "Use an OpenAI Admin API key for usage, or a platform key for legacy billing balance.",
            ),
            config_file_path: None,
            dashboard_url: Some("https://platform.openai.com/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Grok,
            name: "Grok",
            requires_api_key: false,
            api_key_env_var: None,
            api_key_help: Some("Uses Grok browser cookies or ~/.grok/auth.json."),
            config_file_path: Some("~/.grok/auth.json"),
            dashboard_url: Some("https://grok.com/settings/subscription"),
        },
        ProviderConfigInfo {
            id: ProviderId::ElevenLabs,
            name: "ElevenLabs",
            requires_api_key: true,
            api_key_env_var: Some("ELEVENLABS_API_KEY"),
            api_key_help: Some("Get your API key from ElevenLabs Settings > API Keys."),
            config_file_path: None,
            dashboard_url: Some("https://elevenlabs.io/app/settings/api-keys"),
        },
        ProviderConfigInfo {
            id: ProviderId::Deepgram,
            name: "Deepgram",
            requires_api_key: true,
            api_key_env_var: Some("DEEPGRAM_API_KEY"),
            api_key_help: Some("Use a Deepgram API key with Management API access."),
            config_file_path: None,
            dashboard_url: Some("https://console.deepgram.com/usage"),
        },
        ProviderConfigInfo {
            id: ProviderId::Groq,
            name: "Groq",
            requires_api_key: true,
            api_key_env_var: Some("GROQ_API_KEY"),
            api_key_help: Some("Groq metrics require Enterprise Prometheus metrics access."),
            config_file_path: None,
            dashboard_url: Some("https://console.groq.com/settings/metrics"),
        },
        ProviderConfigInfo {
            id: ProviderId::LLMProxy,
            name: "LLM Proxy",
            requires_api_key: true,
            api_key_env_var: Some("LLM_PROXY_API_KEY + LLM_PROXY_BASE_URL"),
            api_key_help: Some("Set an LLM Proxy API key and base URL for quota-stats."),
            config_file_path: None,
            dashboard_url: None,
        },
        ProviderConfigInfo {
            id: ProviderId::Chutes,
            name: "Chutes",
            requires_api_key: true,
            api_key_env_var: Some("CHUTES_API_KEY"),
            api_key_help: Some(
                "Paste a Chutes API key. Optional API URL override: CHUTES_API_URL.",
            ),
            config_file_path: None,
            dashboard_url: Some("https://chutes.ai"),
        },
        ProviderConfigInfo {
            id: ProviderId::LiteLLM,
            name: "LiteLLM",
            requires_api_key: true,
            api_key_env_var: Some("LITELLM_API_KEY + LITELLM_BASE_URL"),
            api_key_help: Some(
                "Paste a LiteLLM key and set the base URL in provider extras or LITELLM_BASE_URL.",
            ),
            config_file_path: None,
            dashboard_url: None,
        },
        ProviderConfigInfo {
            id: ProviderId::Poe,
            name: "Poe",
            requires_api_key: true,
            api_key_env_var: Some("POE_API_KEY"),
            api_key_help: Some("Get your API key from Poe API settings."),
            config_file_path: None,
            dashboard_url: Some("https://poe.com/settings/subscription"),
        },
        ProviderConfigInfo {
            id: ProviderId::Devin,
            name: "Devin",
            requires_api_key: true,
            api_key_env_var: Some("DEVIN_BEARER_TOKEN + DEVIN_ORG"),
            api_key_help: Some(
                "Paste a Devin bearer token and set the organization in provider extras or DEVIN_ORG.",
            ),
            config_file_path: None,
            dashboard_url: Some("https://app.devin.ai/settings/billing"),
        },
        ProviderConfigInfo {
            id: ProviderId::Zed,
            name: "Zed",
            requires_api_key: true,
            api_key_env_var: Some("ZED_CREDENTIALS"),
            api_key_help: Some(
                "Paste Zed credentials as `user_id access_token`; optional API URL in provider extras.",
            ),
            config_file_path: Some("~/.config/zed/settings.json"),
            dashboard_url: Some("https://zed.dev/account"),
        },
    ]
}
