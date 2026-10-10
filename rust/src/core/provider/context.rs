//! Fetch inputs: the requested [`SourceMode`] and the [`FetchContext`].

/// Data source mode for fetching usage
///
/// Conventions for providers whose transport is not an OAuth flow: they
/// reuse `OAuth` as the persisted token/API lane (an API key, hub token, or
/// other credential), because the source enum is shared with the settings
/// UI. `Auto` may dispatch to that lane as well.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceMode {
    /// Automatically choose the best available source
    #[default]
    Auto,
    /// Use OAuth API; also the token/API lane for non-OAuth providers
    OAuth,
    /// Use web API with browser cookies
    Web,
    /// Use CLI probe
    Cli,
}

impl SourceMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Some(SourceMode::Auto),
            "oauth" => Some(SourceMode::OAuth),
            "web" => Some(SourceMode::Web),
            "cli" => Some(SourceMode::Cli),
            _ => None,
        }
    }
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

    /// The cookie source is manual and no cookie is stored, or (for providers
    /// whose cookie source only scopes the session) cookies are off. The
    /// provider decides what this means; Replicate fails closed instead of
    /// importing a browser account the user did not select, and Charm Hyper
    /// skips its session lane.
    pub manual_cookie_missing: bool,

    /// API key for providers that require authentication
    pub api_key: Option<String>,

    /// Type of the explicitly selected labeled token account, if any.
    pub token_account_kind: Option<crate::core::TokenAccountKind>,

    /// A selected account is an identity boundary: providers must not retry
    /// another ambient credential or account after its credential fails.
    pub token_account_isolated: bool,

    /// Optional provider workspace/project scope from persisted settings.
    pub workspace_id: Option<String>,

    /// Optional Copilot seat AI-credit allowance supplied by the app settings.
    /// The provider keeps the credit counter unknown when this is absent.
    pub seat_credit_entitlement: Option<f64>,

    /// Optional provider API/web region from persisted settings.
    pub api_region: Option<String>,

    /// Optional provider gateway URL, used by local gateway-backed providers.
    pub gateway_url: Option<String>,

    /// When true, Auto mode prefers web before local (token-account scope,
    /// manual cookie source, etc.). Workspace overrides are checked separately.
    pub auto_prefer_web: bool,

    /// The user chose automatic browser cookie import for a provider whose
    /// cookies only enrich its API usage (`Provider::cookies_only_enrich_usage`).
    /// False for the default manual-without-cookie state and for Off, so those
    /// never read a browser.
    pub browser_cookie_import: bool,

    /// Foreground usage reads (`codexbar usage`, `codexbar serve`) set this so
    /// providers join slow optional enrichment with the full optional-item
    /// timeout budget measured from task start; background/UI polls keep the
    /// short join grace instead (upstream 0.48.0
    /// `requiresOptionalUsageCompleteness`, #2583).
    pub requires_optional_usage_completeness: bool,

    /// The user opted in to the requested provider's optional detail breakdown
    /// (LiteLLM model activity, Claude workspace spend). Scoped to the provider
    /// being fetched; false everywhere the setting is not consulted.
    pub optional_details_enabled: bool,
}

impl Default for FetchContext {
    fn default() -> Self {
        Self {
            source_mode: SourceMode::Auto,
            include_credits: true,
            web_timeout: 60,
            verbose: false,
            manual_cookie_header: None,
            manual_cookie_missing: false,
            api_key: None,
            token_account_kind: None,
            token_account_isolated: false,
            workspace_id: None,
            seat_credit_entitlement: None,
            api_region: None,
            gateway_url: None,
            auto_prefer_web: false,
            browser_cookie_import: false,
            requires_optional_usage_completeness: false,
            optional_details_enabled: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_source_mode_from_str() {
        assert_eq!(SourceMode::parse("auto"), Some(SourceMode::Auto));
        assert_eq!(SourceMode::parse("oauth"), Some(SourceMode::OAuth));
        assert_eq!(SourceMode::parse("web"), Some(SourceMode::Web));
        assert_eq!(SourceMode::parse("cli"), Some(SourceMode::Cli));
        assert_eq!(SourceMode::parse("AUTO"), Some(SourceMode::Auto));
        assert_eq!(SourceMode::parse("invalid"), None);
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
}
