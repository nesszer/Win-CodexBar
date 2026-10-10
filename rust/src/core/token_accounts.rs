//! Token Account Multi-Support
//!
//! Store and manage multiple accounts/tokens per provider.
//! Supports parallel fetching and account switching.

use crate::core::{ProviderId, SourceMode};
use crate::process_environment::ProcessEnvironment;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use uuid::Uuid;

/// How to inject a token into a fetch request
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TokenInjection {
    /// Inject as Cookie header value
    CookieHeader,
    /// Inject as environment variable
    Environment { key: String },
    /// Accept either an API key or a Cookie header, as with OpenCode Go.
    EnvironmentOrCookie { key: String },
}

/// Credential route selected by a labeled account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenAccountKind {
    Cookie,
    ApiKey,
}

/// Support definition for a provider's token accounts
#[derive(Debug, Clone)]
pub struct TokenAccountSupport {
    /// Display title for the UI
    pub title: &'static str,
    /// Subtitle/description for the UI
    pub subtitle: &'static str,
    /// Placeholder text for input field
    pub placeholder: &'static str,
    /// How tokens are injected
    pub injection: TokenInjection,
    /// Whether manual cookie source is required
    pub requires_manual_cookie_source: bool,
    /// Cookie name to use when normalizing (e.g., "sessionKey")
    pub cookie_name: Option<&'static str>,
}

impl TokenAccountSupport {
    fn cookie(subtitle: &'static str, placeholder: &'static str) -> Self {
        Self {
            title: "Session tokens",
            subtitle,
            placeholder,
            injection: TokenInjection::CookieHeader,
            requires_manual_cookie_source: true,
            cookie_name: None,
        }
    }

    fn api_key(
        title: &'static str,
        env_key: &str,
        subtitle: &'static str,
        placeholder: &'static str,
    ) -> Self {
        Self {
            title,
            subtitle,
            placeholder,
            injection: TokenInjection::Environment {
                key: env_key.to_string(),
            },
            requires_manual_cookie_source: false,
            cookie_name: None,
        }
    }

    fn with_cookie_name(mut self, cookie_name: &'static str) -> Self {
        self.cookie_name = Some(cookie_name);
        self
    }

    /// Get token account support for a provider
    pub fn for_provider(provider: ProviderId) -> Option<Self> {
        match provider {
            ProviderId::Claude => Some(
                Self::cookie(
                    "Store Claude sessionKey cookies for settings-page usage. OAuth tokens are kept as a legacy fallback.",
                    "Paste sessionKey value or Cookie: sessionKey=...",
                )
                .with_cookie_name("sessionKey"),
            ),
            ProviderId::Zai => Some(Self::api_key(
                "API tokens",
                "Z_AI_API_KEY",
                "Stored locally in token-accounts.json. Team usage can use workspace_id as organization|project.",
                "Paste token...",
            )),
            ProviderId::Cursor => Some(Self::cookie(
                "Store multiple Cursor Cookie headers.",
                "Cookie: ...",
            )),
            ProviderId::OpenCode => Some(Self::cookie(
                "Store multiple OpenCode Cookie headers.",
                "Cookie: ...",
            )),
            ProviderId::Factory => Some(Self::cookie(
                "Store multiple Factory Cookie headers.",
                "Cookie: ...",
            )),
            ProviderId::Alibaba => Some(Self::cookie(
                "Store multiple Alibaba Cookie headers.",
                "Cookie: ...",
            )),
            ProviderId::AlibabaTokenPlan => Some(Self::cookie(
                "Store multiple Alibaba Token Plan Cookie headers.",
                "Cookie: cna=...; login_aliyunid_csrf=...",
            )),
            ProviderId::MiniMax => Some(Self::cookie(
                "Store multiple MiniMax Cookie headers.",
                "Cookie: ...",
            )),
            ProviderId::Augment => Some(Self::cookie(
                "Store multiple Augment Cookie headers.",
                "Cookie: ...",
            )),
            ProviderId::Amp => Some(Self::cookie(
                "Store multiple Amp Cookie headers.",
                "Cookie: ...",
            )),
            ProviderId::Ollama => Some(
                Self::cookie(
                    "Store multiple Ollama Cookie headers or __Secure-session values.",
                    "__Secure-session value or Cookie: ...",
                )
                .with_cookie_name("__Secure-session"),
            ),
            ProviderId::T3Chat => Some(Self::cookie(
                "Store multiple T3 Chat Cookie headers or full browser cURL captures.",
                "Cookie: ... or curl ... -H 'Cookie: ...'",
            )),
            ProviderId::ZoomMate => Some(Self::cookie(
                "Store multiple ZoomMate Cookie headers or credits/status cURL captures.",
                "Cookie: ... or curl 'https://ai.zoom.us/.../credits/status' -H 'Authorization: Bearer ...'",
            )),
            ProviderId::Mistral => Some(Self::cookie(
                "Store multiple Mistral Cookie headers.",
                "Cookie: ...",
            )),
            ProviderId::Manus => Some(
                Self::cookie(
                    "Store multiple Manus session_id values.",
                    "session_id value or Cookie: ...",
                )
                .with_cookie_name("session_id"),
            ),
            ProviderId::MiMo => Some(Self::cookie(
                "Store multiple Xiaomi MiMo Cookie headers.",
                "Cookie: api-platform_serviceToken=...; userId=...",
            )),
            ProviderId::CommandCode => Some(
                Self::cookie(
                    "Store multiple Command Code Cookie headers or Better Auth values.",
                    "Cookie: __Secure-commandcode_prod_.session_token=... or better-auth value",
                )
                .with_cookie_name("__Secure-better-auth.session_token"),
            ),
            ProviderId::Qoder => Some(Self::cookie(
                "Store multiple Qoder Cookie headers.",
                "Cookie: ...",
            )),
            ProviderId::CodeBuddy => Some(Self::cookie(
                "Store CodeBuddy CN Cookie headers (from plans-usage DevTools cURL).",
                "Cookie: session=...; ... (or paste full Cookie header)",
            )),
            ProviderId::Sakana => Some(Self::cookie(
                "Store multiple Sakana Console Cookie headers.",
                "Cookie: ...",
            )),
            ProviderId::Notion => Some(
                Self::cookie(
                    "Store multiple Notion Cookie headers or token_v2 values.",
                    "Cookie: token_v2=... or paste the token_v2 value",
                )
                .with_cookie_name("token_v2"),
            ),
            ProviderId::Replicate => Some(
                Self::cookie(
                    "Store multiple Replicate Cookie headers from the billing page.",
                    "Cookie: sessionid=...; ...",
                )
                .with_cookie_name("sessionid"),
            ),
            ProviderId::Sub2Api => Some(Self::api_key(
                "Group API keys",
                "SUB2API_API_KEY",
                "Store multiple sub2api group API keys with labels such as Claude, Codex, or Gemini.",
                "sk-...",
            )),
            ProviderId::DeepInfra => Some(Self::api_key(
                "API keys",
                "DEEPINFRA_API_KEY",
                "Store multiple DeepInfra API keys.",
                "API key from deepinfra.com/dash",
            )),
            ProviderId::HuggingFace => Some(Self::api_key(
                "API tokens",
                "CODEXBAR_HUGGINGFACE_API_KEY",
                "Store multiple Hugging Face access tokens.",
                "Paste a Hugging Face access token",
            )),
            ProviderId::AiAnd => Some(Self::api_key(
                "API keys",
                "AIAND_API_KEY",
                "Store multiple ai& API keys.",
                "API key from console.aiand.com",
            )),
            ProviderId::ZenMux => Some(Self::api_key(
                "API keys",
                "ZENMUX_MANAGEMENT_API_KEY",
                "Store multiple ZenMux Management API keys.",
                "Management API key",
            )),
            ProviderId::ClinePass => Some(Self::api_key(
                "API keys",
                "CLINE_API_KEY",
                "Store multiple ClinePass API keys. Without one, CodexBar reads your existing Cline session (run cline auth) without copying it.",
                "API key",
            )),
            ProviderId::Neuralwatt => Some(Self::api_key(
                "API keys",
                "NEURALWATT_API_KEY",
                "Store multiple Neuralwatt API keys.",
                "API key",
            )),
            ProviderId::Grok => Some(Self {
                title: "Grok credentials",
                requires_manual_cookie_source: false,
                ..Self::cookie(
                    "Store SuperGrok bearer tokens or grok.com Cookie headers.",
                    "Bearer token or Cookie: ...",
                )
            }),
            ProviderId::Xai => Some(Self::api_key(
                "Management API keys",
                "XAI_MANAGEMENT_API_KEY",
                "Store multiple xAI Management API keys. Team ID is set separately under provider settings.",
                "xai-... Management API key from console.x.ai",
            )),
            // Upstream 0.45 #2271: labeled OpenRouter API keys via token accounts.
            ProviderId::OpenRouter => Some(Self::api_key(
                "API keys",
                "OPENROUTER_API_KEY",
                "Store multiple OpenRouter API keys.",
                "sk-or-v1-...",
            )),
            ProviderId::Copilot => Some(Self::api_key(
                "GitHub accounts",
                "GITHUB_TOKEN",
                "Store GitHub OAuth tokens for Copilot plan usage.",
                "Sign in with GitHub or paste a GitHub OAuth token...",
            )),
            ProviderId::Kimi => Some(Self {
                title: "Web sessions",
                ..Self::cookie(
                    "Store labeled Kimi kimi-auth web sessions.",
                    "kimi-auth value or Cookie: kimi-auth=...",
                )
                .with_cookie_name("kimi-auth")
            }),
            ProviderId::Doubao => Some(Self::api_key(
                "Ark API keys",
                "ARK_API_KEY",
                "Store labeled Volcengine Ark API keys.",
                "Ark API key",
            )),
            ProviderId::OpenCodeGo => Some(Self {
                injection: TokenInjection::EnvironmentOrCookie {
                    key: "OPENCODE_API_KEY".to_string(),
                },
                ..Self::api_key(
                    "API keys or sessions",
                    "OPENCODE_API_KEY",
                    "Store labeled OpenCode Go API keys or Cookie headers.",
                    "API key or Cookie: ...",
                )
            }),
            // Upstream 0.67: labeled Aixy API keys via token accounts.
            ProviderId::Aixy => Some(Self::api_key(
                "API keys",
                "AIXY_API_KEY",
                "Store multiple Aixy API keys.",
                "Paste Aixy API key…",
            )),
            // These providers don't support token accounts
            ProviderId::Codex
            | ProviderId::Pi
            | ProviderId::Gemini
            | ProviderId::Antigravity
            | ProviderId::Kiro
            | ProviderId::VertexAI
            | ProviderId::KimiK2
            | ProviderId::JetBrains
            | ProviderId::Warp
            | ProviderId::AzureOpenAI
            | ProviderId::NanoGPT
            | ProviderId::Infini
            | ProviderId::Perplexity
            | ProviderId::Abacus
            | ProviderId::Kilo
            | ProviderId::Bedrock
            | ProviderId::Codebuff
            | ProviderId::CodeRabbit
            | ProviderId::DeepSeek
            | ProviderId::Windsurf
            | ProviderId::StepFun
            | ProviderId::Venice
            | ProviderId::OpenAIApi
            | ProviderId::ElevenLabs
            | ProviderId::Deepgram
            | ProviderId::Groq
            | ProviderId::Helmcode
            | ProviderId::V0
            | ProviderId::TypeSafe
            | ProviderId::LLMProxy
            | ProviderId::Chutes
            | ProviderId::LiteLLM
            | ProviderId::Poe
            | ProviderId::Devin
            | ProviderId::Zed
            | ProviderId::CrossModel
            | ProviderId::LongCat
            | ProviderId::Wayfinder
            | ProviderId::QwenCloud
            | ProviderId::Fireworks
            | ProviderId::Meta
            | ProviderId::Nous
            | ProviderId::Muse
            | ProviderId::AtlasCloud
            | ProviderId::Hyper
            | ProviderId::GitKraken
            | ProviderId::Bifrost
            | ProviderId::LLMMan
            | ProviderId::DevPass
            | ProviderId::XKiro
            | ProviderId::Raycast
            | ProviderId::Vercel => None,
        }
    }

    /// Check if a provider supports token accounts
    pub fn is_supported(provider: ProviderId) -> bool {
        Self::for_provider(provider).is_some()
    }

    /// Get environment override for a token
    pub fn env_override(provider: ProviderId, token: &str) -> Option<HashMap<String, String>> {
        let support = Self::for_provider(provider)?;
        if provider == ProviderId::Grok
            && let Some(token) = Self::normalized_grok_oauth_token(token)
        {
            let mut map = HashMap::new();
            map.insert("CODEXBAR_GROK_OAUTH_TOKEN".to_string(), token);
            return Some(map);
        }
        match &support.injection {
            TokenInjection::Environment { key } => {
                let mut map = HashMap::new();
                map.insert(key.clone(), token.to_string());
                Some(map)
            }
            TokenInjection::EnvironmentOrCookie { key } => {
                let api_key = Self::normalized_opencodego_api_key(token)?;
                let mut map = HashMap::new();
                map.insert(key.clone(), api_key);
                Some(map)
            }
            TokenInjection::CookieHeader => {
                // Check for Claude OAuth token
                if provider == ProviderId::Claude
                    && let Some(normalized) = Self::normalized_claude_oauth_token(token)
                    && Self::is_claude_oauth_token(&normalized)
                {
                    let mut map = HashMap::new();
                    map.insert("CODEXBAR_CLAUDE_OAUTH_TOKEN".to_string(), normalized);
                    return Some(map);
                }
                None
            }
        }
    }

    fn normalized_opencodego_api_key(token: &str) -> Option<String> {
        let token = token.trim();
        let token = if token.len() >= 2
            && ((token.starts_with('"') && token.ends_with('"'))
                || (token.starts_with('\'') && token.ends_with('\'')))
        {
            token[1..token.len() - 1].trim()
        } else {
            token
        };
        if token.is_empty()
            || token
                .chars()
                .any(|ch| ch.is_whitespace() || matches!(ch, '=' | ':'))
        {
            return None;
        }
        Some(token.to_string())
    }

    pub fn account_kind(provider: ProviderId, token: &str) -> TokenAccountKind {
        if provider == ProviderId::OpenCodeGo {
            if Self::normalized_opencodego_api_key(token).is_some() {
                TokenAccountKind::ApiKey
            } else {
                TokenAccountKind::Cookie
            }
        } else if matches!(
            Self::for_provider(provider).map(|support| support.injection),
            Some(TokenInjection::Environment { .. })
        ) {
            TokenAccountKind::ApiKey
        } else {
            TokenAccountKind::Cookie
        }
    }

    /// Normalize a cookie header for a provider
    pub fn normalized_cookie_header(provider: ProviderId, token: &str) -> String {
        let trimmed = token.trim();
        let Some(support) = Self::for_provider(provider) else {
            return trimmed.to_string();
        };

        let Some(cookie_name) = support.cookie_name else {
            return trimmed.to_string();
        };

        let lower = trimmed.to_lowercase();
        if lower.contains("cookie:") || trimmed.contains('=') {
            return trimmed.to_string();
        }

        format!("{}={}", cookie_name, trimmed)
    }

    fn normalized_grok_oauth_token(token: &str) -> Option<String> {
        let mut value = token.trim();
        if value.len() >= 7 && value[..7].eq_ignore_ascii_case("bearer ") {
            value = value[7..].trim();
        }
        if value.is_empty()
            || value.contains('=')
            || value.to_ascii_lowercase().starts_with("cookie:")
            || value.to_ascii_lowercase().starts_with("xai-")
        {
            return None;
        }
        Some(value.to_string())
    }

    /// Check if a token is a Claude OAuth token
    pub fn is_claude_oauth_token(token: &str) -> bool {
        let Some(trimmed) = Self::normalized_claude_oauth_token(token) else {
            return false;
        };
        let lower = trimmed.to_lowercase();
        if lower.contains("cookie:") || trimmed.contains('=') {
            return false;
        }
        lower.starts_with("sk-ant-oat")
    }

    /// Normalize a Claude OAuth token
    fn normalized_claude_oauth_token(token: &str) -> Option<String> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            return None;
        }
        let lower = trimmed.to_lowercase();
        if lower.starts_with("bearer ") {
            Some(trimmed[7..].trim().to_string())
        } else {
            Some(trimmed.to_string())
        }
    }
}

/// A single token account for a provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAccount {
    /// Unique identifier
    pub id: Uuid,
    /// User-provided label
    pub label: String,
    /// The token/cookie value
    pub token: String,
    /// Stable external identity supplied by the provider, when available
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_identifier: Option<String>,
    /// When this account was added (Unix timestamp in seconds)
    pub added_at: i64,
    /// When this account was last used (Unix timestamp in seconds)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used: Option<i64>,
}

impl TokenAccount {
    /// Create a new token account
    pub fn new(label: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            label: label.into(),
            token: token.into(),
            external_identifier: None,
            added_at: Utc::now().timestamp(),
            last_used: None,
        }
    }

    /// Mark this account as used
    pub fn mark_used(&mut self) {
        self.last_used = Some(Utc::now().timestamp());
    }

    /// Get display name
    pub fn display_name(&self) -> &str {
        &self.label
    }

    /// Get added_at as DateTime
    pub fn added_at_datetime(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.added_at, 0).unwrap_or_else(Utc::now)
    }

    /// Get last_used as DateTime
    pub fn last_used_datetime(&self) -> Option<DateTime<Utc>> {
        self.last_used
            .and_then(|ts| DateTime::from_timestamp(ts, 0))
    }
}

/// Account data for a provider
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderAccountData {
    /// File format version
    #[serde(default = "default_version")]
    pub version: u32,
    /// List of accounts
    pub accounts: Vec<TokenAccount>,
    /// Index of the active account
    #[serde(default)]
    pub active_index: usize,
}

fn default_version() -> u32 {
    1
}

impl ProviderAccountData {
    /// Create new empty account data
    pub fn new() -> Self {
        Self {
            version: 1,
            accounts: Vec::new(),
            active_index: 0,
        }
    }

    /// Get the clamped active index
    pub fn clamped_active_index(&self) -> usize {
        if self.accounts.is_empty() {
            return 0;
        }
        self.active_index.min(self.accounts.len() - 1)
    }

    /// Get the active account
    pub fn active_account(&self) -> Option<&TokenAccount> {
        self.accounts.get(self.clamped_active_index())
    }

    /// Add a new account
    pub fn add_account(&mut self, account: TokenAccount) {
        self.accounts.push(account);
    }

    /// Remove an account by ID
    pub fn remove_account(&mut self, id: Uuid) -> Option<TokenAccount> {
        let pos = self.accounts.iter().position(|a| a.id == id)?;
        let removed = self.accounts.remove(pos);
        // Adjust active index if needed
        if self.active_index >= self.accounts.len() && !self.accounts.is_empty() {
            self.active_index = self.accounts.len() - 1;
        }
        Some(removed)
    }

    /// Set the active account by index
    pub fn set_active(&mut self, index: usize) {
        self.active_index = index.min(self.accounts.len().saturating_sub(1));
    }

    /// Set the active account by ID
    pub fn set_active_by_id(&mut self, id: Uuid) -> bool {
        if let Some(pos) = self.accounts.iter().position(|a| a.id == id) {
            self.active_index = pos;
            true
        } else {
            false
        }
    }

    /// Get account count
    pub fn count(&self) -> usize {
        self.accounts.len()
    }
}

/// File format for storing all provider accounts
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenAccountsFile {
    version: u32,
    providers: HashMap<String, ProviderAccountData>,
}

/// Token account store for persisting accounts to disk
pub struct TokenAccountStore {
    file_path: PathBuf,
}

/// Errors that can occur with token account storage
#[derive(Debug, thiserror::Error)]
pub enum TokenAccountError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl TokenAccountStore {
    /// Create a new store with the default path
    pub fn new() -> Self {
        Self {
            file_path: Self::default_path(),
        }
    }

    /// Create a store with a custom path
    pub fn with_path(path: PathBuf) -> Self {
        Self { file_path: path }
    }

    /// Get the default storage path (beside a `CODEXBAR_CONFIG` settings file).
    pub fn default_path() -> PathBuf {
        crate::settings::config_store_dir()
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".codexbar")
            })
            .join("token-accounts.json")
    }

    /// Load all accounts from disk
    pub fn load(&self) -> Result<HashMap<ProviderId, ProviderAccountData>, TokenAccountError> {
        if !self.file_path.exists() {
            return Ok(HashMap::new());
        }

        let data = crate::secure_file::read_string(&self.file_path)?;
        let file: TokenAccountsFile = serde_json::from_str(&data)?;

        let mut result = HashMap::new();
        for (key, value) in file.providers {
            if let Some(provider) = ProviderId::from_cli_name(&key) {
                result.insert(provider, value);
            }
        }
        Ok(result)
    }

    /// Save all accounts to disk
    pub fn save(
        &self,
        accounts: &HashMap<ProviderId, ProviderAccountData>,
    ) -> Result<(), TokenAccountError> {
        // Ensure directory exists
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let providers: HashMap<String, ProviderAccountData> = accounts
            .iter()
            .map(|(k, v)| (k.cli_name().to_string(), v.clone()))
            .collect();

        let file = TokenAccountsFile {
            version: 1,
            providers,
        };

        let json = serde_json::to_string_pretty(&file)?;
        crate::secure_file::write_string(&self.file_path, &json)?;
        Ok(())
    }

    /// Load accounts for a specific provider
    pub fn load_provider(
        &self,
        provider: ProviderId,
    ) -> Result<ProviderAccountData, TokenAccountError> {
        let all = self.load()?;
        Ok(all.get(&provider).cloned().unwrap_or_default())
    }

    /// Save accounts for a specific provider
    pub fn save_provider(
        &self,
        provider: ProviderId,
        data: &ProviderAccountData,
    ) -> Result<(), TokenAccountError> {
        let mut all = self.load()?;
        all.insert(provider, data.clone());
        self.save(&all)
    }
}

impl Default for TokenAccountStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Override for temporarily using a different token during fetch
#[derive(Debug, Clone)]
pub struct TokenAccountOverride {
    /// The provider being overridden
    pub provider: ProviderId,
    /// The account being used
    pub account: TokenAccount,
    /// Environment variables to set (`Debug` renders only the entry count)
    pub env_override: ProcessEnvironment<Option<HashMap<String, String>>>,
    /// Cookie header to use
    pub cookie_header: Option<String>,
    pub kind: TokenAccountKind,
}

impl TokenAccountOverride {
    /// Create an override from an account
    pub fn from_account(provider: ProviderId, account: TokenAccount) -> Self {
        let kind = TokenAccountSupport::account_kind(provider, &account.token);
        let env_override = TokenAccountSupport::env_override(provider, &account.token);
        let cookie_header = if env_override.is_none() {
            Some(TokenAccountSupport::normalized_cookie_header(
                provider,
                &account.token,
            ))
        } else {
            None
        };

        Self {
            provider,
            account,
            env_override: env_override.into(),
            cookie_header,
            kind,
        }
    }

    /// Normalize source selection for account types whose credential requires
    /// a specific route. `None` leaves unrelated providers' source policy alone.
    pub fn effective_source_mode(&self, requested: SourceMode) -> Option<SourceMode> {
        match (self.provider, self.kind, requested) {
            (ProviderId::Kimi, _, _) => Some(SourceMode::Web),
            (ProviderId::Doubao, _, _) => Some(SourceMode::OAuth),
            (ProviderId::OpenCodeGo, TokenAccountKind::Cookie, SourceMode::Auto) => {
                Some(SourceMode::Web)
            }
            (ProviderId::OpenCodeGo, _, _) => Some(requested),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pins every provider's token-account metadata so table refactors stay byte-identical.
    #[test]
    fn for_provider_table_is_pinned() {
        let rows: Vec<String> = ProviderId::all()
            .iter()
            .filter_map(|&provider| {
                let support = TokenAccountSupport::for_provider(provider)?;
                let injection = match &support.injection {
                    TokenInjection::CookieHeader => "cookie".to_string(),
                    TokenInjection::Environment { key } => format!("env:{key}"),
                    TokenInjection::EnvironmentOrCookie { key } => format!("env_or_cookie:{key}"),
                };
                Some(format!(
                    "{provider:?}|{}|{}|{}|{injection}|{}|{:?}",
                    support.title,
                    support.subtitle,
                    support.placeholder,
                    support.requires_manual_cookie_source,
                    support.cookie_name
                ))
            })
            .collect();
        let mut expected: Vec<&str> = EXPECTED_SUPPORT_ROWS.to_vec();
        let mut actual: Vec<&str> = rows.iter().map(String::as_str).collect();
        expected.sort_unstable();
        actual.sort_unstable();
        assert_eq!(actual, expected);
    }

    const EXPECTED_SUPPORT_ROWS: &[&str] = &[
        "Claude|Session tokens|Store Claude sessionKey cookies for settings-page usage. OAuth tokens are kept as a legacy fallback.|Paste sessionKey value or Cookie: sessionKey=...|cookie|true|Some(\"sessionKey\")",
        "Zai|API tokens|Stored locally in token-accounts.json. Team usage can use workspace_id as organization|project.|Paste token...|env:Z_AI_API_KEY|false|None",
        "Cursor|Session tokens|Store multiple Cursor Cookie headers.|Cookie: ...|cookie|true|None",
        "OpenCode|Session tokens|Store multiple OpenCode Cookie headers.|Cookie: ...|cookie|true|None",
        "Factory|Session tokens|Store multiple Factory Cookie headers.|Cookie: ...|cookie|true|None",
        "Alibaba|Session tokens|Store multiple Alibaba Cookie headers.|Cookie: ...|cookie|true|None",
        "AlibabaTokenPlan|Session tokens|Store multiple Alibaba Token Plan Cookie headers.|Cookie: cna=...; login_aliyunid_csrf=...|cookie|true|None",
        "MiniMax|Session tokens|Store multiple MiniMax Cookie headers.|Cookie: ...|cookie|true|None",
        "Augment|Session tokens|Store multiple Augment Cookie headers.|Cookie: ...|cookie|true|None",
        "Amp|Session tokens|Store multiple Amp Cookie headers.|Cookie: ...|cookie|true|None",
        "Ollama|Session tokens|Store multiple Ollama Cookie headers or __Secure-session values.|__Secure-session value or Cookie: ...|cookie|true|Some(\"__Secure-session\")",
        "T3Chat|Session tokens|Store multiple T3 Chat Cookie headers or full browser cURL captures.|Cookie: ... or curl ... -H 'Cookie: ...'|cookie|true|None",
        "ZoomMate|Session tokens|Store multiple ZoomMate Cookie headers or credits/status cURL captures.|Cookie: ... or curl 'https://ai.zoom.us/.../credits/status' -H 'Authorization: Bearer ...'|cookie|true|None",
        "Mistral|Session tokens|Store multiple Mistral Cookie headers.|Cookie: ...|cookie|true|None",
        "Manus|Session tokens|Store multiple Manus session_id values.|session_id value or Cookie: ...|cookie|true|Some(\"session_id\")",
        "MiMo|Session tokens|Store multiple Xiaomi MiMo Cookie headers.|Cookie: api-platform_serviceToken=...; userId=...|cookie|true|None",
        "CommandCode|Session tokens|Store multiple Command Code Cookie headers or Better Auth values.|Cookie: __Secure-commandcode_prod_.session_token=... or better-auth value|cookie|true|Some(\"__Secure-better-auth.session_token\")",
        "Qoder|Session tokens|Store multiple Qoder Cookie headers.|Cookie: ...|cookie|true|None",
        "CodeBuddy|Session tokens|Store CodeBuddy CN Cookie headers (from plans-usage DevTools cURL).|Cookie: session=...; ... (or paste full Cookie header)|cookie|true|None",
        "Sakana|Session tokens|Store multiple Sakana Console Cookie headers.|Cookie: ...|cookie|true|None",
        "Notion|Session tokens|Store multiple Notion Cookie headers or token_v2 values.|Cookie: token_v2=... or paste the token_v2 value|cookie|true|Some(\"token_v2\")",
        "Replicate|Session tokens|Store multiple Replicate Cookie headers from the billing page.|Cookie: sessionid=...; ...|cookie|true|Some(\"sessionid\")",
        "Sub2Api|Group API keys|Store multiple sub2api group API keys with labels such as Claude, Codex, or Gemini.|sk-...|env:SUB2API_API_KEY|false|None",
        "DeepInfra|API keys|Store multiple DeepInfra API keys.|API key from deepinfra.com/dash|env:DEEPINFRA_API_KEY|false|None",
        "HuggingFace|API tokens|Store multiple Hugging Face access tokens.|Paste a Hugging Face access token|env:CODEXBAR_HUGGINGFACE_API_KEY|false|None",
        "AiAnd|API keys|Store multiple ai& API keys.|API key from console.aiand.com|env:AIAND_API_KEY|false|None",
        "ZenMux|API keys|Store multiple ZenMux Management API keys.|Management API key|env:ZENMUX_MANAGEMENT_API_KEY|false|None",
        "ClinePass|API keys|Store multiple ClinePass API keys. Without one, CodexBar reads your existing Cline session (run cline auth) without copying it.|API key|env:CLINE_API_KEY|false|None",
        "Neuralwatt|API keys|Store multiple Neuralwatt API keys.|API key|env:NEURALWATT_API_KEY|false|None",
        "Grok|Grok credentials|Store SuperGrok bearer tokens or grok.com Cookie headers.|Bearer token or Cookie: ...|cookie|false|None",
        "Xai|Management API keys|Store multiple xAI Management API keys. Team ID is set separately under provider settings.|xai-... Management API key from console.x.ai|env:XAI_MANAGEMENT_API_KEY|false|None",
        "OpenRouter|API keys|Store multiple OpenRouter API keys.|sk-or-v1-...|env:OPENROUTER_API_KEY|false|None",
        "Copilot|GitHub accounts|Store GitHub OAuth tokens for Copilot plan usage.|Sign in with GitHub or paste a GitHub OAuth token...|env:GITHUB_TOKEN|false|None",
        "Kimi|Web sessions|Store labeled Kimi kimi-auth web sessions.|kimi-auth value or Cookie: kimi-auth=...|cookie|true|Some(\"kimi-auth\")",
        "Doubao|Ark API keys|Store labeled Volcengine Ark API keys.|Ark API key|env:ARK_API_KEY|false|None",
        "OpenCodeGo|API keys or sessions|Store labeled OpenCode Go API keys or Cookie headers.|API key or Cookie: ...|env_or_cookie:OPENCODE_API_KEY|false|None",
        "Aixy|API keys|Store multiple Aixy API keys.|Paste Aixy API key…|env:AIXY_API_KEY|false|None",
    ];

    #[test]
    fn test_token_account_support() {
        assert!(TokenAccountSupport::is_supported(ProviderId::Claude));
        assert!(TokenAccountSupport::is_supported(ProviderId::Cursor));
        assert!(TokenAccountSupport::is_supported(ProviderId::Copilot));
        assert!(TokenAccountSupport::is_supported(ProviderId::OpenRouter));
        assert!(TokenAccountSupport::is_supported(ProviderId::Grok));
        assert!(TokenAccountSupport::is_supported(ProviderId::Kimi));
        assert!(TokenAccountSupport::is_supported(ProviderId::Doubao));
        assert!(TokenAccountSupport::is_supported(ProviderId::OpenCodeGo));
        assert!(TokenAccountSupport::is_supported(ProviderId::Aixy));
        assert!(!TokenAccountSupport::is_supported(ProviderId::Codex));
        assert!(!TokenAccountSupport::is_supported(ProviderId::Gemini));
        assert!(!TokenAccountSupport::is_supported(ProviderId::Hyper));
        assert!(!TokenAccountSupport::is_supported(ProviderId::GitKraken));
        assert!(!TokenAccountSupport::is_supported(ProviderId::Bifrost));
    }

    #[test]
    fn upstream_account_sources_normalize_and_classify_selected_credentials() {
        assert_eq!(
            TokenAccountSupport::normalized_cookie_header(ProviderId::Kimi, "selected-session"),
            "kimi-auth=selected-session"
        );
        assert_eq!(
            TokenAccountSupport::account_kind(ProviderId::Kimi, "selected-session"),
            TokenAccountKind::Cookie
        );
        assert_eq!(
            TokenAccountSupport::account_kind(ProviderId::Doubao, "ark-key"),
            TokenAccountKind::ApiKey
        );
        assert_eq!(
            TokenAccountSupport::account_kind(ProviderId::OpenCodeGo, "opencode-key"),
            TokenAccountKind::ApiKey
        );
        assert_eq!(
            TokenAccountSupport::account_kind(
                ProviderId::OpenCodeGo,
                "Cookie: session=opencode-session"
            ),
            TokenAccountKind::Cookie
        );
        assert_eq!(
            TokenAccountSupport::env_override(ProviderId::OpenCodeGo, "opencode-key")
                .and_then(|env| env.get("OPENCODE_API_KEY").cloned())
                .as_deref(),
            Some("opencode-key")
        );
        assert!(
            TokenAccountSupport::env_override(
                ProviderId::OpenCodeGo,
                "Cookie: session=opencode-session"
            )
            .is_none()
        );
        for malformed in ["", " ", "Cookie: broken", "auth=fixture", "two words"] {
            assert_eq!(
                TokenAccountSupport::account_kind(ProviderId::OpenCodeGo, malformed),
                TokenAccountKind::Cookie
            );
            assert!(TokenAccountSupport::env_override(ProviderId::OpenCodeGo, malformed).is_none());
        }
        assert_eq!(
            TokenAccountSupport::env_override(ProviderId::OpenCodeGo, " 'go_key' ")
                .and_then(|env| env.get("OPENCODE_API_KEY").cloned())
                .as_deref(),
            Some("go_key")
        );
    }

    #[test]
    fn selected_account_effective_source_normalization() {
        let cases = [
            (
                ProviderId::Kimi,
                "kimi-session",
                SourceMode::Auto,
                Some(SourceMode::Web),
            ),
            (
                ProviderId::Kimi,
                "kimi-session",
                SourceMode::OAuth,
                Some(SourceMode::Web),
            ),
            (
                ProviderId::Kimi,
                "kimi-session",
                SourceMode::Cli,
                Some(SourceMode::Web),
            ),
            (
                ProviderId::Doubao,
                "ark-key",
                SourceMode::Cli,
                Some(SourceMode::OAuth),
            ),
            (
                ProviderId::Doubao,
                "ark-key",
                SourceMode::Web,
                Some(SourceMode::OAuth),
            ),
            (
                ProviderId::OpenCodeGo,
                "Cookie: session=web",
                SourceMode::Auto,
                Some(SourceMode::Web),
            ),
            (
                ProviderId::OpenCodeGo,
                "Cookie: session=web",
                SourceMode::Cli,
                Some(SourceMode::Cli),
            ),
            (
                ProviderId::OpenCodeGo,
                "api-key",
                SourceMode::Auto,
                Some(SourceMode::Auto),
            ),
            (
                ProviderId::OpenCodeGo,
                "api-key",
                SourceMode::Web,
                Some(SourceMode::Web),
            ),
            (
                ProviderId::OpenCodeGo,
                "api-key",
                SourceMode::Cli,
                Some(SourceMode::Cli),
            ),
            (ProviderId::OpenRouter, "api-key", SourceMode::Auto, None),
        ];

        for (provider, token, requested, expected) in cases {
            let account =
                TokenAccountOverride::from_account(provider, TokenAccount::new("selected", token));
            assert_eq!(account.effective_source_mode(requested), expected);
        }
    }

    #[test]
    fn aixy_token_accounts_inject_api_key_env() {
        let support = TokenAccountSupport::for_provider(ProviderId::Aixy).unwrap();
        assert_eq!(support.title, "API keys");
        assert_eq!(support.placeholder, "Paste Aixy API key…");
        assert!(!support.requires_manual_cookie_source);
        let env = TokenAccountSupport::env_override(ProviderId::Aixy, "gak_fixture").unwrap();
        assert_eq!(
            env.get("AIXY_API_KEY").map(String::as_str),
            Some("gak_fixture")
        );
    }

    #[test]
    fn grok_token_accounts_route_bearer_and_cookie_credentials() {
        let bearer =
            TokenAccountSupport::env_override(ProviderId::Grok, "Bearer oauth-token").unwrap();
        assert_eq!(
            bearer.get("CODEXBAR_GROK_OAUTH_TOKEN").map(String::as_str),
            Some("oauth-token")
        );
        assert!(TokenAccountSupport::env_override(ProviderId::Grok, "Cookie: sso=abc").is_none());
        assert_eq!(
            TokenAccountSupport::normalized_cookie_header(ProviderId::Grok, "Cookie: sso=abc"),
            "Cookie: sso=abc"
        );
    }

    #[test]
    fn openrouter_token_accounts_inject_api_key_env() {
        let support = TokenAccountSupport::for_provider(ProviderId::OpenRouter).unwrap();
        assert_eq!(support.title, "API keys");
        assert_eq!(support.placeholder, "sk-or-v1-...");
        assert!(!support.requires_manual_cookie_source);
        match &support.injection {
            TokenInjection::Environment { key } => assert_eq!(key, "OPENROUTER_API_KEY"),
            other => panic!("expected environment injection, got {other:?}"),
        }

        let mut data = ProviderAccountData::new();
        data.add_account(TokenAccount::new("Personal", "sk-or-v1-personal"));
        data.add_account(TokenAccount::new("Work", "sk-or-v1-work"));
        data.set_active(1);

        let active = data.active_account().unwrap();
        assert_eq!(active.label, "Work");
        let env = TokenAccountSupport::env_override(ProviderId::OpenRouter, &active.token).unwrap();
        assert_eq!(
            env.get("OPENROUTER_API_KEY").map(String::as_str),
            Some("sk-or-v1-work")
        );

        let override_data =
            TokenAccountOverride::from_account(ProviderId::OpenRouter, active.clone());
        assert_eq!(
            override_data
                .env_override
                .as_ref()
                .and_then(|m| m.get("OPENROUTER_API_KEY"))
                .map(String::as_str),
            Some("sk-or-v1-work")
        );
        assert!(override_data.cookie_header.is_none());
    }

    #[test]
    fn test_claude_oauth_detection() {
        assert!(TokenAccountSupport::is_claude_oauth_token(
            "sk-ant-oat01-abc123"
        ));
        assert!(TokenAccountSupport::is_claude_oauth_token(
            "Bearer sk-ant-oat01-abc123"
        ));
        assert!(!TokenAccountSupport::is_claude_oauth_token(
            "sessionKey=abc123"
        ));
        assert!(!TokenAccountSupport::is_claude_oauth_token(
            "Cookie: foo=bar"
        ));
    }

    #[test]
    fn test_normalize_cookie_header() {
        let header =
            TokenAccountSupport::normalized_cookie_header(ProviderId::Claude, "abc123token");
        assert_eq!(header, "sessionKey=abc123token");

        let header = TokenAccountSupport::normalized_cookie_header(
            ProviderId::Claude,
            "sessionKey=already_formatted",
        );
        assert_eq!(header, "sessionKey=already_formatted");

        let header = TokenAccountSupport::normalized_cookie_header(ProviderId::Ollama, "abc123");
        assert_eq!(header, "__Secure-session=abc123");
    }

    #[test]
    fn test_provider_account_data() {
        let mut data = ProviderAccountData::new();
        assert_eq!(data.clamped_active_index(), 0);
        assert!(data.active_account().is_none());

        let account = TokenAccount::new("Test", "token123");
        let id = account.id;
        data.add_account(account);

        assert_eq!(data.count(), 1);
        assert!(data.active_account().is_some());
        assert_eq!(data.active_account().unwrap().label, "Test");

        data.remove_account(id);
        assert_eq!(data.count(), 0);
    }

    #[test]
    fn test_multiple_accounts() {
        let mut data = ProviderAccountData::new();
        data.add_account(TokenAccount::new("Account 1", "token1"));
        data.add_account(TokenAccount::new("Account 2", "token2"));

        assert_eq!(data.count(), 2);
        assert_eq!(data.active_account().unwrap().label, "Account 1");

        data.set_active(1);
        assert_eq!(data.active_account().unwrap().label, "Account 2");
    }
}
