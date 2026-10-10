//! The [`Provider`] trait and the shell policies a provider can select.

use async_trait::async_trait;

use super::{FetchContext, ProviderError, ProviderId, ProviderMetadata, SourceMode};
use crate::core::ProviderFetchResult;
use crate::core::ProviderStateKind;

/// How the shell should treat a failed refresh when a prior good snapshot exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastGoodFailurePolicy {
    Replace,
    Preserve,
    PreserveOnce,
    PreserveOnceThenSurface,
}

/// How the shell should treat a manual cookie source with no cookie present.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManualEmptyCookiePolicy {
    /// Remap to the shell's generic browser-cookie attempt.
    Fallback,
    /// Keep `SourceMode::Web` with no header so the provider fails closed
    /// instead of importing a browser account the user did not select.
    FailClosedWeb,
}

/// Trait that all providers must implement
#[allow(
    clippy::double_must_use,
    reason = "async-trait marks its boxed futures #[must_use]"
)]
#[async_trait]
pub trait Provider: Send + Sync {
    /// Get the provider's unique identifier
    fn id(&self) -> ProviderId;

    /// Get provider metadata
    fn metadata(&self) -> &ProviderMetadata {
        self.id().metadata()
    }

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

    /// Check if web API (cookies) is supported. Defaults to whether
    /// [`Self::available_sources`] lists [`SourceMode::Web`].
    fn supports_web(&self) -> bool {
        self.available_sources().contains(&SourceMode::Web)
    }

    /// Check if CLI probe is supported. Defaults to whether
    /// [`Self::available_sources`] lists [`SourceMode::Cli`].
    fn supports_cli(&self) -> bool {
        self.available_sources().contains(&SourceMode::Cli)
    }

    /// Detect the version of the CLI tool (if applicable)
    fn detect_version(&self) -> Option<String> {
        None
    }

    /// Whether an explicitly selected manual cookie outranks a token-account override.
    fn manual_cookie_precedes_token_account(&self) -> bool {
        false
    }

    /// Whether a selected token account leaves an `Auto` usage source as `Auto`.
    ///
    /// The shell normally maps a token account with an environment override to
    /// the OAuth (API-only) lane. A provider whose `Auto` source adds an
    /// optional extra on top of the API credential, such as the Hugging Face
    /// prepaid wallet, opts in so the account token does not silently drop it.
    /// An explicitly chosen non-Auto usage source still maps to OAuth.
    fn token_account_preserves_auto_source(&self) -> bool {
        false
    }

    /// How the shell treats a manual cookie source with no cookie present.
    ///
    /// `Fallback` lets the shell remap to its generic browser-cookie attempt.
    /// `FailClosedWeb` keeps `SourceMode::Web` without any header, so the
    /// provider fails closed instead of importing a browser account the user
    /// did not select.
    fn manual_empty_cookie_policy(&self) -> ManualEmptyCookiePolicy {
        ManualEmptyCookiePolicy::Fallback
    }

    /// Whether Automatic metric selection should prefer an exhausted quota lane.
    fn automatic_metric_prioritizes_exhausted_window(&self) -> bool {
        true
    }

    /// Whether an explicit (non-Automatic) metric preference whose lane is
    /// unavailable should still fall through to Automatic selection. Providers
    /// with Automatic-only fallback lanes (seat credits) override this to
    /// `false` so an explicit choice is never silently replaced by fallback
    /// progress.
    fn explicit_preference_falls_through_to_automatic(&self) -> bool {
        true
    }

    /// Whether Automatic metric selection is a dead end when the primary lane
    /// is informational and no secondary lane exists. Providers with
    /// Automatic-only fallback lanes (seat credits) or a named extra lane that
    /// can be the only reported quota (Kimi's monthly pool) override this to
    /// `false` so that lane can still fill in.
    fn automatic_metric_missing_core_is_terminal(&self) -> bool {
        true
    }

    /// Whether Automatic metric selection shows the secondary lane whenever
    /// neither core lane is exhausted, instead of the fuller lane. An
    /// exhausted primary or secondary lane still wins, primary first.
    fn automatic_metric_prefers_secondary_window(&self) -> bool {
        false
    }

    /// Id of the extra rate window that holds this provider's monthly plan
    /// allowance. The `MonthlyPlan` menu bar metric selects that window
    /// (upstream 0.70.0 #4072). `None` means the provider offers no Monthly
    /// Plan metric.
    fn monthly_plan_window_id(&self) -> Option<&'static str> {
        None
    }

    /// Label for the primary lane in the menu bar metric picker when the
    /// lane is not a session window (upstream `menuBarLayoutPrimaryLabel`).
    /// `None` keeps the generic session label.
    fn menu_bar_primary_label(&self) -> Option<&'static str> {
        None
    }

    /// Whether cookies only enrich an API-backed result instead of being a
    /// usage source of their own. The shell then keeps the configured usage
    /// source, forwards a manual cookie as-is, and reads a browser only when
    /// the cookie source is Automatic (`FetchContext::browser_cookie_import`);
    /// the default manual state with no cookie never triggers a browser read.
    fn cookies_only_enrich_usage(&self) -> bool {
        false
    }

    /// Whether browser-cookie discovery/recovery is owned by the provider.
    fn owns_browser_cookie_resolution(&self) -> bool {
        false
    }

    /// Whether the web lane is used only when the usage source is explicitly
    /// `web`. A cookie domain otherwise lets the shell turn Auto into Web
    /// (manual cookie present or browser import), which would replace a
    /// provider's default non-web credential.
    fn web_is_opt_in(&self) -> bool {
        false
    }

    /// Whether the cookie source only scopes the browser session the
    /// provider may use, leaving the selected usage source in charge of
    /// routing.
    ///
    /// When true, the shell keeps the usage source for every cookie source:
    /// `off` and a `manual` source without a stored cookie pass no header and
    /// set [`FetchContext::manual_cookie_missing`], so the provider skips the
    /// session and Auto can still use an API key. Otherwise the shell remaps
    /// the source mode from the cookie source.
    fn cookie_source_scopes_session_only(&self) -> bool {
        false
    }

    /// How the shell should treat a failed refresh when a prior good snapshot exists.
    fn last_good_failure_policy(&self, _error: &str) -> LastGoodFailurePolicy {
        LastGoodFailurePolicy::Replace
    }

    /// Whether this provider can safely retain its last good snapshot on a
    /// classified transport failure.
    fn retains_last_good_on_transport_failure(&self) -> bool {
        false
    }

    /// Typed variant used before an error is sanitized for the frontend.
    ///
    /// Providers that need message-based distinctions can keep overriding the
    /// string method. Transport retention is selected by the provider
    /// capability and the typed error classification above.
    fn last_good_failure_policy_for_error(&self, error: &ProviderError) -> LastGoodFailurePolicy {
        if matches!(error, ProviderError::OAuthTransient(_)) {
            return LastGoodFailurePolicy::Preserve;
        }
        if self.retains_last_good_on_transport_failure() && error.is_transport_failure() {
            return LastGoodFailurePolicy::Preserve;
        }
        self.last_good_failure_policy(&error.to_string())
    }

    /// Presentation-safe availability state for a refresh error. The default
    /// maps `ProviderError` variants, treating `NotInstalled` as a missing
    /// credential (most providers raise it for a missing API key or auth
    /// file). Override only when a variant carries provider-specific meaning
    /// that differs — e.g. a local language-server probe or CLI/binary
    /// presence check whose "not installed" means the runtime is simply
    /// not running; prefer a message-contains guard when the provider also
    /// raises credential-flavored `NotInstalled` errors.
    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        error.state_kind()
    }
}
