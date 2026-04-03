//! Provider trait and registry - defines the interface all providers must implement

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};
use thiserror::Error;

use super::ProviderFetchResult;

/// Unique identifier for a provider
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Codex,
    Claude,
    Cursor,
    Factory,
    Gemini,
    Antigravity,
    Copilot,
    Zai,
    MiniMax,
    Kiro,
    VertexAI,
    Augment,
    OpenCode,
    Kimi,
    KimiK2,
    Amp,
    Warp,
    Ollama,
    OpenRouter,
    Synthetic,
    JetBrains,
    Alibaba,
    NanoGPT,
    Infini,
}

impl ProviderId {
    /// Get all provider IDs
    pub fn all() -> &'static [ProviderId] {
        &[
            ProviderId::Codex,
            ProviderId::Claude,
            ProviderId::Cursor,
            ProviderId::Factory,
            ProviderId::Gemini,
            ProviderId::Antigravity,
            ProviderId::Copilot,
            ProviderId::Zai,
            ProviderId::MiniMax,
            ProviderId::Kiro,
            ProviderId::VertexAI,
            ProviderId::Augment,
            ProviderId::OpenCode,
            ProviderId::Kimi,
            ProviderId::KimiK2,
            ProviderId::Amp,
            ProviderId::Warp,
            ProviderId::Ollama,
            ProviderId::OpenRouter,
            ProviderId::Synthetic,
            ProviderId::JetBrains,
            ProviderId::Alibaba,
            ProviderId::NanoGPT,
            ProviderId::Infini,
        ]
    }

    /// Get the CLI name for this provider
    pub fn cli_name(&self) -> &'static str {
        match self {
            ProviderId::Codex => "codex",
            ProviderId::Claude => "claude",
            ProviderId::Cursor => "cursor",
            ProviderId::Factory => "factory",
            ProviderId::Gemini => "gemini",
            ProviderId::Antigravity => "antigravity",
            ProviderId::Copilot => "copilot",
            ProviderId::Zai => "zai",
            ProviderId::MiniMax => "minimax",
            ProviderId::Kiro => "kiro",
            ProviderId::VertexAI => "vertexai",
            ProviderId::Augment => "augment",
            ProviderId::OpenCode => "opencode",
            ProviderId::Kimi => "kimi",
            ProviderId::KimiK2 => "kimik2",
            ProviderId::Amp => "amp",
            ProviderId::Warp => "warp",
            ProviderId::Ollama => "ollama",
            ProviderId::OpenRouter => "openrouter",
            ProviderId::Synthetic => "synthetic",
            ProviderId::JetBrains => "jetbrains",
            ProviderId::Alibaba => "alibaba",
            ProviderId::NanoGPT => "nanogpt",
            ProviderId::Infini => "infini",
        }
    }

    /// Get the display name for this provider
    pub fn display_name(&self) -> &'static str {
        match self {
            ProviderId::Codex => "Codex",
            ProviderId::Claude => "Claude",
            ProviderId::Cursor => "Cursor",
            ProviderId::Factory => "Factory",
            ProviderId::Gemini => "Gemini",
            ProviderId::Antigravity => "Antigravity",
            ProviderId::Copilot => "Copilot",
            ProviderId::Zai => "z.ai",
            ProviderId::MiniMax => "MiniMax",
            ProviderId::Kiro => "Kiro",
            ProviderId::VertexAI => "Vertex AI",
            ProviderId::Augment => "Augment",
            ProviderId::OpenCode => "OpenCode",
            ProviderId::Kimi => "Kimi",
            ProviderId::KimiK2 => "Kimi K2",
            ProviderId::Amp => "Amp",
            ProviderId::Warp => "Warp",
            ProviderId::Ollama => "Ollama",
            ProviderId::OpenRouter => "OpenRouter",
            ProviderId::Synthetic => "Synthetic",
            ProviderId::JetBrains => "JetBrains AI",
            ProviderId::Alibaba => "Alibaba",
            ProviderId::NanoGPT => "NanoGPT",
            ProviderId::Infini => "Infini",
        }
    }

    /// Get the cookie domain for this provider.
    /// Returns the domain used for cookie extraction, or None if the provider
    /// doesn't use cookies for authentication.
    pub fn cookie_domain(&self) -> Option<&'static str> {
        match self {
            ProviderId::Claude => Some("claude.ai"),
            ProviderId::Cursor => Some("cursor.com"),
            ProviderId::Factory => Some("app.factory.ai"),
            ProviderId::Codex => Some("chatgpt.com"),
            ProviderId::Gemini => Some("aistudio.google.com"),
            ProviderId::Kiro => Some("kiro.dev"),
            ProviderId::Kimi => Some("kimi.moonshot.cn"),
            ProviderId::KimiK2 => Some("platform.moonshot.cn"),
            ProviderId::MiniMax => Some("platform.minimaxi.com"),
            ProviderId::OpenCode => Some("opencode.ai"),
            ProviderId::Augment => Some("app.augmentcode.com"),
            ProviderId::Amp => Some("sourcegraph.com"),
            ProviderId::Antigravity => Some("antigravity.ai"),
            ProviderId::Alibaba => Some("tongyi.aliyun.com"),
            ProviderId::Ollama => Some("ollama.com"),
            // Token-based providers (don't use cookies)
            ProviderId::Copilot => None,
            ProviderId::Zai => None,
            ProviderId::VertexAI => None,
            ProviderId::JetBrains => None,
            ProviderId::Synthetic => None,
            ProviderId::Warp => None,
            ProviderId::OpenRouter => None,
            ProviderId::NanoGPT => None,
            ProviderId::Infini => None,
        }
    }

    /// Parse from CLI name string
    pub fn from_cli_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "codex" | "openai" => Some(ProviderId::Codex),
            "claude" | "anthropic" => Some(ProviderId::Claude),
            "cursor" => Some(ProviderId::Cursor),
            "factory" | "droid" => Some(ProviderId::Factory),
            "gemini" | "google" => Some(ProviderId::Gemini),
            "antigravity" => Some(ProviderId::Antigravity),
            "copilot" | "github" => Some(ProviderId::Copilot),
            "zai" | "z.ai" | "zed" => Some(ProviderId::Zai),
            "minimax" => Some(ProviderId::MiniMax),
            "kiro" | "aws" => Some(ProviderId::Kiro),
            "vertexai" | "vertex" => Some(ProviderId::VertexAI),
            "augment" => Some(ProviderId::Augment),
            "opencode" => Some(ProviderId::OpenCode),
            "kimi" | "moonshot" => Some(ProviderId::Kimi),
            "kimik2" | "kimi-k2" | "k2" => Some(ProviderId::KimiK2),
            "amp" | "sourcegraph" => Some(ProviderId::Amp),
            "warp" | "warp-ai" | "warp-terminal" => Some(ProviderId::Warp),
            "ollama" => Some(ProviderId::Ollama),
            "openrouter" | "or" => Some(ProviderId::OpenRouter),
            "synthetic" => Some(ProviderId::Synthetic),
            "jetbrains" | "jetbrains-ai" | "intellij" => Some(ProviderId::JetBrains),
            "alibaba" | "tongyi" | "qianwen" | "qwen" => Some(ProviderId::Alibaba),
            "nanogpt" | "nano-gpt" => Some(ProviderId::NanoGPT),
            "infini" | "infini-ai" => Some(ProviderId::Infini),
            _ => None,
        }
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.cli_name())
    }
}

/// Data source mode for fetching usage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceMode {
    /// Automatically choose the best available source
    #[default]
    Auto,
    /// Use OAuth API
    OAuth,
    /// Use web API with browser cookies
    Web,
    /// Use CLI probe
    Cli,
}

impl SourceMode {
    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Some(SourceMode::Auto),
            "oauth" => Some(SourceMode::OAuth),
            "web" => Some(SourceMode::Web),
            "cli" => Some(SourceMode::Cli),
            _ => None,
        }
    }
}

/// Metadata about a provider
#[derive(Debug, Clone)]
pub struct ProviderMetadata {
    pub id: ProviderId,
    pub display_name: &'static str,
    pub session_label: &'static str,
    pub weekly_label: &'static str,
    pub supports_opus: bool,
    pub supports_credits: bool,
    pub default_enabled: bool,
    pub is_primary: bool,
    pub dashboard_url: Option<&'static str>,
    pub status_page_url: Option<&'static str>,
}

/// Errors that can occur when fetching provider data
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("Provider not installed: {0}")]
    NotInstalled(String),

    #[error("Authentication required")]
    AuthRequired,

    #[error("OAuth error: {0}")]
    OAuth(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Timeout")]
    Timeout,

    #[error("Source mode '{0:?}' not supported for this provider")]
    UnsupportedSource(SourceMode),

    #[error("No cookies available for web API")]
    NoCookies,

    #[error("{0}")]
    Other(String),
}

/// Context passed to provider fetch operations
#[derive(Debug, Clone)]
pub struct FetchContext {
    /// Source mode to use
    pub source_mode: SourceMode,

    /// Whether to include credits/cost data
    pub include_credits: bool,

    /// Timeout for web operations in seconds
    pub web_timeout: u64,

    /// Whether to enable verbose logging
    pub verbose: bool,

    /// Manual cookie header (for testing)
    pub manual_cookie_header: Option<String>,

    /// API key for providers that require authentication
    pub api_key: Option<String>,
}

impl Default for FetchContext {
    fn default() -> Self {
        Self {
            source_mode: SourceMode::Auto,
            include_credits: true,
            web_timeout: 60,
            verbose: false,
            manual_cookie_header: None,
            api_key: None,
        }
    }
}

/// Trait that all providers must implement
#[async_trait]
pub trait Provider: Send + Sync {
    /// Get the provider's unique identifier
    fn id(&self) -> ProviderId;

    /// Get provider metadata
    fn metadata(&self) -> &ProviderMetadata;

    /// Fetch usage data from this provider
    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError>;

    /// Get the available source modes for this provider
    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto]
    }

    /// Check if OAuth is supported
    fn supports_oauth(&self) -> bool {
        false
    }

    /// Check if web API (cookies) is supported
    fn supports_web(&self) -> bool {
        false
    }

    /// Check if CLI probe is supported
    fn supports_cli(&self) -> bool {
        false
    }

    /// Detect the version of the CLI tool (if applicable)
    fn detect_version(&self) -> Option<String> {
        None
    }
}

/// Registry of all available providers
pub struct ProviderRegistry {
    providers: RwLock<HashMap<ProviderId, Box<dyn Provider>>>,
}

impl ProviderRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
        }
    }

    /// Register a provider
    pub fn register(&self, provider: Box<dyn Provider>) {
        let id = provider.id();
        let mut providers = self.providers.write().unwrap();
        providers.insert(id, provider);
    }

    /// Get a provider by ID
    pub fn get(&self, id: ProviderId) -> Option<Box<dyn Provider>> {
        let providers = self.providers.read().unwrap();
        // We need to clone here, but since Provider is not Clone, we'll return None for now
        // In practice, we'll use Arc<dyn Provider> instead
        providers
            .get(&id)
            .map(|_| todo!("Use Arc<dyn Provider> instead"))
    }

    /// Get all registered provider IDs
    pub fn all_ids(&self) -> Vec<ProviderId> {
        let providers = self.providers.read().unwrap();
        providers.keys().copied().collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global provider registry
pub static REGISTRY: LazyLock<ProviderRegistry> = LazyLock::new(ProviderRegistry::new);

/// Get the CLI name map for argument parsing
pub fn cli_name_map() -> HashMap<&'static str, ProviderId> {
    let mut map = HashMap::new();
    for id in ProviderId::all() {
        map.insert(id.cli_name(), *id);
    }
    // Add aliases
    map.insert("openai", ProviderId::Codex);
    map.insert("anthropic", ProviderId::Claude);
    map.insert("windsurf", ProviderId::Factory);
    map.insert("codeium", ProviderId::Factory);
    map.insert("google", ProviderId::Gemini);
    map.insert("github", ProviderId::Copilot);
    map.insert("zed", ProviderId::Zai);
    map.insert("aws", ProviderId::Kiro);
    map.insert("vertex", ProviderId::VertexAI);
    map.insert("sourcegraph", ProviderId::Amp);
    map.insert("warp-ai", ProviderId::Warp);
    map.insert("warp-terminal", ProviderId::Warp);
    map.insert("or", ProviderId::OpenRouter);
    map.insert("tongyi", ProviderId::Alibaba);
    map.insert("qianwen", ProviderId::Alibaba);
    map.insert("qwen", ProviderId::Alibaba);
    map.insert("infini-ai", ProviderId::Infini);
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_id_all() {
        let all = ProviderId::all();
        assert_eq!(all.len(), 24);
        assert!(all.contains(&ProviderId::Claude));
        assert!(all.contains(&ProviderId::Codex));
        assert!(all.contains(&ProviderId::Kimi));
        assert!(all.contains(&ProviderId::KimiK2));
        assert!(all.contains(&ProviderId::Amp));
        assert!(all.contains(&ProviderId::Synthetic));
        assert!(all.contains(&ProviderId::JetBrains));
        assert!(all.contains(&ProviderId::NanoGPT));
        assert!(all.contains(&ProviderId::Infini));
    }

    #[test]
    fn test_provider_id_cli_name() {
        assert_eq!(ProviderId::Claude.cli_name(), "claude");
        assert_eq!(ProviderId::Codex.cli_name(), "codex");
        assert_eq!(ProviderId::Factory.cli_name(), "factory");
        assert_eq!(ProviderId::Zai.cli_name(), "zai");
    }

    #[test]
    fn test_provider_id_display_name() {
        assert_eq!(ProviderId::Claude.display_name(), "Claude");
        assert_eq!(ProviderId::Factory.display_name(), "Factory");
        assert_eq!(ProviderId::Zai.display_name(), "z.ai");
    }

    #[test]
    fn test_provider_id_from_cli_name() {
        assert_eq!(
            ProviderId::from_cli_name("claude"),
            Some(ProviderId::Claude)
        );
        assert_eq!(
            ProviderId::from_cli_name("anthropic"),
            Some(ProviderId::Claude)
        );
        assert_eq!(
            ProviderId::from_cli_name("CLAUDE"),
            Some(ProviderId::Claude)
        );
        assert_eq!(ProviderId::from_cli_name("codex"), Some(ProviderId::Codex));
        assert_eq!(ProviderId::from_cli_name("openai"), Some(ProviderId::Codex));
        assert_eq!(
            ProviderId::from_cli_name("factory"),
            Some(ProviderId::Factory)
        );
        assert_eq!(ProviderId::from_cli_name("zed"), Some(ProviderId::Zai));
        assert_eq!(ProviderId::from_cli_name("unknown"), None);
    }

    #[test]
    fn test_provider_id_display() {
        assert_eq!(format!("{}", ProviderId::Claude), "claude");
        assert_eq!(format!("{}", ProviderId::Codex), "codex");
    }

    #[test]
    fn test_source_mode_from_str() {
        assert_eq!(SourceMode::from_str("auto"), Some(SourceMode::Auto));
        assert_eq!(SourceMode::from_str("oauth"), Some(SourceMode::OAuth));
        assert_eq!(SourceMode::from_str("web"), Some(SourceMode::Web));
        assert_eq!(SourceMode::from_str("cli"), Some(SourceMode::Cli));
        assert_eq!(SourceMode::from_str("AUTO"), Some(SourceMode::Auto));
        assert_eq!(SourceMode::from_str("invalid"), None);
    }

    #[test]
    fn test_fetch_context_default() {
        let ctx = FetchContext::default();
        assert_eq!(ctx.source_mode, SourceMode::Auto);
        assert!(ctx.include_credits);
        assert_eq!(ctx.web_timeout, 60);
        assert!(!ctx.verbose);
        assert!(ctx.manual_cookie_header.is_none());
        assert!(ctx.api_key.is_none());
    }

    #[test]
    fn test_cli_name_map() {
        let map = cli_name_map();
        assert_eq!(map.get("claude"), Some(&ProviderId::Claude));
        assert_eq!(map.get("anthropic"), Some(&ProviderId::Claude));
        assert_eq!(map.get("codex"), Some(&ProviderId::Codex));
        assert_eq!(map.get("openai"), Some(&ProviderId::Codex));
    }

    #[test]
    fn test_provider_id_cookie_domain() {
        // Cookie-based providers
        assert_eq!(ProviderId::Claude.cookie_domain(), Some("claude.ai"));
        assert_eq!(ProviderId::Cursor.cookie_domain(), Some("cursor.com"));
        assert_eq!(ProviderId::Factory.cookie_domain(), Some("app.factory.ai"));
        assert_eq!(ProviderId::Codex.cookie_domain(), Some("chatgpt.com"));
        assert_eq!(
            ProviderId::Gemini.cookie_domain(),
            Some("aistudio.google.com")
        );
        assert_eq!(ProviderId::Kiro.cookie_domain(), Some("kiro.dev"));
        assert_eq!(ProviderId::Kimi.cookie_domain(), Some("kimi.moonshot.cn"));
        assert_eq!(ProviderId::OpenCode.cookie_domain(), Some("opencode.ai"));

        // Token-based providers (no cookies)
        assert_eq!(ProviderId::Copilot.cookie_domain(), None);
        assert_eq!(ProviderId::Zai.cookie_domain(), None);
        assert_eq!(ProviderId::VertexAI.cookie_domain(), None);
        assert_eq!(ProviderId::JetBrains.cookie_domain(), None);
    }

    #[test]
    fn test_provider_id_alibaba() {
        assert_eq!(ProviderId::Alibaba.cli_name(), "alibaba");
        assert_eq!(ProviderId::Alibaba.display_name(), "Alibaba");
        assert_eq!(
            ProviderId::Alibaba.cookie_domain(),
            Some("tongyi.aliyun.com")
        );
        assert_eq!(
            ProviderId::from_cli_name("alibaba"),
            Some(ProviderId::Alibaba)
        );
        assert_eq!(
            ProviderId::from_cli_name("tongyi"),
            Some(ProviderId::Alibaba)
        );
        assert_eq!(
            ProviderId::from_cli_name("qianwen"),
            Some(ProviderId::Alibaba)
        );
        assert_eq!(ProviderId::from_cli_name("qwen"), Some(ProviderId::Alibaba));
    }
}
