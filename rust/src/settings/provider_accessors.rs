//! Per-provider configuration accessors and legacy field-name aliases.

use super::*;

impl Settings {
    // ── Per-provider configuration accessors ─────────────────────────
    //
    // These thin wrappers around `provider_configs` apply provider-specific
    // defaults (e.g. cookie/usage source defaults to `"auto"`) so callers
    // never have to reach into the raw `Option<String>` fields. The
    // `*_str` / boolean / setter pairs intentionally mirror the names of
    // the legacy flat fields so call-site migration is mechanical.

    /// Read-only access to a provider's stored config, if any.
    pub fn provider_config(&self, id: ProviderId) -> Option<&ProviderConfig> {
        self.provider_configs.get(&id)
    }

    /// Mutable access to a provider's config, lazily creating an empty
    /// entry if none exists.
    pub fn provider_config_mut(&mut self, id: ProviderId) -> &mut ProviderConfig {
        self.provider_configs.entry(id).or_default()
    }

    /// Cookie source for `id`. Kimi and Charm Hyper follow upstream's
    /// automatic default; providers with no specific default retain the
    /// legacy manual default.
    pub fn cookie_source(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.cookie_source.as_deref())
            .unwrap_or(match id {
                ProviderId::Kimi | ProviderId::Hyper => "auto",
                _ => DEFAULT_COOKIE_SOURCE,
            })
    }

    pub fn set_cookie_source(&mut self, id: ProviderId, source: impl Into<String>) {
        self.provider_config_mut(id).cookie_source = Some(source.into());
    }

    /// Usage source for `id`, or the default `"auto"` if unset.
    pub fn usage_source(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.usage_source.as_deref())
            .unwrap_or(DEFAULT_PROVIDER_SOURCE)
    }

    pub fn set_usage_source(&mut self, id: ProviderId, source: impl Into<String>) {
        self.provider_config_mut(id).usage_source = Some(source.into());
    }

    /// API region for `id`, or the provider-specific default if unset.
    pub fn api_region(&self, id: ProviderId) -> &str {
        if id == ProviderId::AlibabaTokenPlan && !self.alibaba_token_plan_region.trim().is_empty() {
            return self.alibaba_token_plan_region.as_str();
        }
        self.provider_configs
            .get(&id)
            .and_then(|c| c.api_region.as_deref())
            .unwrap_or_else(|| default_api_region(id))
    }

    pub fn set_api_region(&mut self, id: ProviderId, region: impl Into<String>) {
        let region = region.into();
        if id == ProviderId::AlibabaTokenPlan {
            self.alibaba_token_plan_region = region.clone();
        }
        self.provider_config_mut(id).api_region = Some(region);
    }

    /// Manual cookie header for `id`, or `""` if unset.
    pub fn manual_cookie_header(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.manual_cookie_header.as_deref())
            .unwrap_or("")
    }

    /// API token for `id`, or `""` if unset.
    pub fn api_token(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.api_token.as_deref())
            .unwrap_or("")
    }

    pub fn management_api_token(&self, id: ProviderId) -> Option<&str> {
        self.provider_configs
            .get(&id)
            .and_then(|config| config.management_api_token.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn set_management_api_token(&mut self, id: ProviderId, token: Option<String>) {
        let token = token
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.provider_config_mut(id).management_api_token = token;
    }

    /// Workspace ID override for `id`, or `""` if unset.
    pub fn workspace_id(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.workspace_id.as_deref())
            .unwrap_or("")
    }

    pub fn set_workspace_id(&mut self, id: ProviderId, value: impl Into<String>) {
        self.provider_config_mut(id).workspace_id = Some(value.into());
    }

    /// Optional user-entered allowance for Copilot seat AI credits.
    ///
    /// GitHub reports the absolute `credits_used` counter but does not expose
    /// a documented included-credit ceiling, so callers must keep an absent
    /// or non-positive value as unknown rather than inventing a denominator.
    ///
    /// This setter is the single owner of the positive-finite invariant:
    /// invalid values are rejected instead of silently dropped, while the
    /// getter keeps defensively filtering values persisted by older builds.
    pub fn seat_credit_entitlement(&self, id: ProviderId) -> Option<f64> {
        self.provider_configs
            .get(&id)
            .and_then(|config| config.seat_credit_entitlement)
            .filter(|value| value.is_finite() && *value > 0.0)
    }

    pub fn set_seat_credit_entitlement(
        &mut self,
        id: ProviderId,
        value: Option<f64>,
    ) -> Result<(), String> {
        if let Some(value) = value
            && (!value.is_finite() || value <= 0.0)
        {
            return Err(
                "Copilot seat AI-credit allowance must be a finite number greater than zero"
                    .to_string(),
            );
        }
        self.provider_config_mut(id).seat_credit_entitlement = value;
        Ok(())
    }

    /// Wayfinder gateway URL, defaulting to the local loopback gateway.
    pub fn gateway_url(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.gateway_url.as_deref())
            .unwrap_or_else(|| {
                if id == ProviderId::Wayfinder {
                    crate::providers::wayfinder::DEFAULT_GATEWAY_URL
                } else {
                    ""
                }
            })
    }

    pub fn set_gateway_url(&mut self, id: ProviderId, value: impl Into<String>) {
        self.provider_config_mut(id).gateway_url = Some(value.into());
    }

    /// IDE base path override for `id`, or `""` if unset.
    pub fn ide_base_path(&self, id: ProviderId) -> &str {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.ide_base_path.as_deref())
            .unwrap_or("")
    }

    pub fn set_ide_base_path(&mut self, id: ProviderId, value: impl Into<String>) {
        self.provider_config_mut(id).ide_base_path = Some(value.into());
    }

    /// Codex `openai_web_extras` toggle, default `true`.
    pub fn openai_web_extras(&self, id: ProviderId) -> bool {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.openai_web_extras)
            .unwrap_or(DEFAULT_CODEX_OPENAI_WEB_EXTRAS)
    }

    /// Codex Spark rows are visible by default.
    pub fn spark_usage_visible(&self, id: ProviderId) -> bool {
        self.provider_configs
            .get(&id)
            .and_then(|c| c.spark_usage_visible)
            .unwrap_or(DEFAULT_CODEX_SPARK_USAGE_VISIBLE)
    }

    pub fn set_spark_usage_visible(&mut self, id: ProviderId, value: bool) {
        self.provider_config_mut(id).spark_usage_visible = Some(value);
    }

    /// Return the persisted hidden usage-item IDs for `id`.
    pub fn hidden_usage_item_ids(&self, id: ProviderId) -> Vec<String> {
        self.provider_configs
            .get(&id)
            .and_then(|config| config.hidden_usage_item_ids.as_ref())
            .map_or_else(Vec::new, |ids| normalize_hidden_usage_item_ids(ids.clone()))
    }

    /// Persist an explicit presentation-only usage-item visibility list.
    pub fn set_hidden_usage_item_ids(&mut self, id: ProviderId, ids: Vec<String>) {
        let hidden = normalize_hidden_usage_item_ids(ids);
        self.provider_config_mut(id).hidden_usage_item_ids = Some(hidden);
    }

    /// Add or remove `items` from the provider's hidden usage-item list.
    ///
    /// `visible = false` hides the items; `visible = true` un-hides them. This
    /// is the single write path for usage-item visibility; it only touches the
    /// presentation list and never the legacy per-provider boolean flags.
    pub fn toggle_hidden_items(&mut self, id: ProviderId, items: &[&str], visible: bool) {
        let mut hidden = self.hidden_usage_item_ids(id);
        if visible {
            hidden.retain(|item| !items.contains(&item.as_str()));
        } else {
            hidden.extend(items.iter().map(|item| (*item).to_string()));
        }
        self.set_hidden_usage_item_ids(id, hidden);
    }

    /// Update the old Claude Daily Routines flag without touching the
    /// usage-item list; the flag is presentation-only and kept for callers
    /// that still read the boolean directly.
    pub fn set_claude_daily_routines_usage_visible(&mut self, value: bool) {
        self.claude_daily_routines_usage_visible = value;
    }

    /// Per-provider historical-tracking toggle (currently codex-only).
    pub fn historical_tracking(&self, id: ProviderId) -> bool {
        self.provider_configs
            .get(&id)
            .map(|c| c.historical_tracking)
            .unwrap_or(false)
    }

    /// Per-provider "avoid keychain prompts" toggle (currently claude-only).
    pub fn avoid_keychain_prompts(&self, id: ProviderId) -> bool {
        self.provider_configs
            .get(&id)
            .map(|c| c.avoid_keychain_prompts)
            .unwrap_or(false)
    }

    pub fn set_avoid_keychain_prompts(&mut self, id: ProviderId, value: bool) {
        self.provider_config_mut(id).avoid_keychain_prompts = value;
    }

    /// Whether the desktop shell may reopen a captured CLI session after its
    /// provider quota becomes available again. This is intentionally opt-in.
    pub fn auto_resume_after_quota_reset(&self, id: ProviderId) -> bool {
        self.provider_configs
            .get(&id)
            .map(|config| config.auto_resume_after_quota_reset)
            .unwrap_or(false)
    }

    pub fn set_auto_resume_after_quota_reset(&mut self, id: ProviderId, value: bool) {
        self.provider_config_mut(id).auto_resume_after_quota_reset = value;
    }

    // ── Legacy field-name aliases ────────────────────────────────────
    //
    // Keep the names of the old flat per-provider fields available as
    // accessor methods so existing call sites only need a `()` (read) or
    // `set_` prefix (write). New code should prefer the typed accessors
    // above.

    pub fn codex_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Codex)
    }
    pub fn claude_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Claude)
    }
    pub fn cursor_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Cursor)
    }
    pub fn opencode_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::OpenCode)
    }
    pub fn factory_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Factory)
    }
    pub fn alibaba_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Alibaba)
    }
    pub fn kimi_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Kimi)
    }
    pub fn minimax_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::MiniMax)
    }
    pub fn augment_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Augment)
    }
    pub fn amp_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Amp)
    }
    pub fn ollama_cookie_source(&self) -> &str {
        self.cookie_source(ProviderId::Ollama)
    }

    pub fn claude_usage_source(&self) -> &str {
        self.usage_source(ProviderId::Claude)
    }
    pub fn codex_usage_source(&self) -> &str {
        self.usage_source(ProviderId::Codex)
    }

    pub fn alibaba_api_region(&self) -> &str {
        self.api_region(ProviderId::Alibaba)
    }
    pub fn zai_api_region(&self) -> &str {
        self.api_region(ProviderId::Zai)
    }
    pub fn minimax_api_region(&self) -> &str {
        self.api_region(ProviderId::MiniMax)
    }

    pub fn alibaba_cookie_header(&self) -> &str {
        self.manual_cookie_header(ProviderId::Alibaba)
    }
    pub fn kimi_manual_cookie_header(&self) -> &str {
        self.manual_cookie_header(ProviderId::Kimi)
    }
    pub fn augment_cookie_header(&self) -> &str {
        self.manual_cookie_header(ProviderId::Augment)
    }
    pub fn amp_cookie_header(&self) -> &str {
        self.manual_cookie_header(ProviderId::Amp)
    }
    pub fn ollama_cookie_header(&self) -> &str {
        self.manual_cookie_header(ProviderId::Ollama)
    }
    pub fn minimax_cookie_header(&self) -> &str {
        self.manual_cookie_header(ProviderId::MiniMax)
    }

    pub fn opencode_workspace_id(&self) -> &str {
        self.workspace_id(ProviderId::OpenCode)
    }
    pub fn minimax_api_token(&self) -> &str {
        self.api_token(ProviderId::MiniMax)
    }
    pub fn jetbrains_ide_base_path(&self) -> &str {
        self.ide_base_path(ProviderId::JetBrains)
    }
    pub fn set_jetbrains_ide_base_path(&mut self, v: impl Into<String>) {
        self.set_ide_base_path(ProviderId::JetBrains, v)
    }

    pub fn codex_openai_web_extras(&self) -> bool {
        self.openai_web_extras(ProviderId::Codex)
    }
    pub fn codex_spark_usage_visible(&self) -> bool {
        self.spark_usage_visible(ProviderId::Codex)
    }
    pub fn set_codex_spark_usage_visible(&mut self, v: bool) {
        self.set_spark_usage_visible(ProviderId::Codex, v)
    }
    pub fn codex_historical_tracking(&self) -> bool {
        self.historical_tracking(ProviderId::Codex)
    }
    pub fn claude_avoid_keychain_prompts(&self) -> bool {
        self.avoid_keychain_prompts(ProviderId::Claude)
    }
    pub fn set_claude_avoid_keychain_prompts(&mut self, v: bool) {
        self.set_avoid_keychain_prompts(ProviderId::Claude, v)
    }

    /// Claude-only: whether the external claude-swap (`cswap`) adapter is
    /// enabled. Disabled by default.
    pub fn claude_swap_enabled(&self) -> bool {
        self.provider_configs
            .get(&ProviderId::Claude)
            .map(|config| config.claude_swap_enabled)
            .unwrap_or(false)
    }

    pub fn set_claude_swap_enabled(&mut self, value: bool) {
        self.provider_config_mut(ProviderId::Claude)
            .claude_swap_enabled = value;
    }

    /// Claude-only: configured claude-swap executable path, or `""` when unset.
    pub fn claude_swap_executable_path(&self) -> &str {
        self.provider_configs
            .get(&ProviderId::Claude)
            .and_then(|config| config.claude_swap_executable_path.as_deref())
            .unwrap_or("")
    }

    pub fn set_claude_swap_executable_path(&mut self, value: impl Into<String>) {
        let trimmed = value.into().trim().to_string();
        let config = self.provider_config_mut(ProviderId::Claude);
        config.claude_swap_executable_path = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }
}
