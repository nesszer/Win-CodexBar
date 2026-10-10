use super::*;

/// Build a `FetchContext` for a provider using persisted cookies/keys.
pub(crate) fn build_fetch_context(
    id: ProviderId,
    settings: &Settings,
    cookies: &ManualCookies,
    api_keys: &ApiKeys,
    token_accounts: &HashMap<ProviderId, ProviderAccountData>,
) -> FetchContext {
    let provider = instantiate_provider(id);
    let cookie_source = settings.cookie_source(id);
    let stored_cookie = cookies.get(id.cli_name()).map(|s| s.to_string());
    let stored_api_key = api_keys.get(id.cli_name()).map(|s| s.to_string());
    let token_override = token_accounts
        .get(&id)
        .and_then(|data| data.active_account())
        .cloned()
        .map(|account| TokenAccountOverride::from_account(id, account));
    let active_token_cookie = token_override
        .as_ref()
        .and_then(|override_data| override_data.cookie_header.clone());
    let defer_provider_browser_cookie_lookup = provider.owns_browser_cookie_resolution()
        && active_token_cookie.is_none()
        && stored_cookie.is_none();
    let active_token_env = token_override
        .as_ref()
        .and_then(|override_data| override_data.env_override.as_ref());
    let active_token_api_key = active_token_env.and_then(|env| env.values().next().cloned());
    let usage_source = SourceMode::parse(settings.usage_source(id)).unwrap_or_default();
    let token_account_kind = token_override.as_ref().map(|account| account.kind);
    // Selected token-account key overrides a stored provider apiKey (upstream #2271 / #1183).
    let api_key = active_token_api_key.clone().or(stored_api_key);
    let has_kimi_code_api_key =
        id == ProviderId::Kimi && api_key.as_deref().is_some_and(|key| !key.trim().is_empty());
    let has_opencodego_api_key = id == ProviderId::OpenCodeGo
        && api_key.as_deref().is_some_and(|key| !key.trim().is_empty());

    // Providers whose cookies only enrich an API result keep the configured
    // usage source. Off and the default manual-without-cookie state never read
    // a browser; only Automatic imports one (`browser_cookie_import`).
    let cookies_only_enrich_usage = provider.cookies_only_enrich_usage();
    let browser_cookie_import =
        cookies_only_enrich_usage && matches!(cookie_source, "auto" | "browser" | "web");
    // #433: an explicitly selected, non-empty Claude manual cookie is
    // authoritative. Do not let an active OAuth token account silently
    // replace it; this keeps tray refresh behavior aligned with diagnose,
    // whose Claude Auto path tries the supplied Web cookie before OAuth.
    let manual_cookie_wins = cookie_source == "manual"
        && provider.manual_cookie_precedes_token_account()
        && stored_cookie
            .as_deref()
            .is_some_and(|cookie| !cookie.trim().is_empty());
    let (mut source_mode, mut cookie_header, fails_closed_without_cookie) = if id
        .cookie_domain()
        .is_none()
    {
        // The #725 wallet rule (#725 keeps Auto for wallet providers) folds into
        // the account-source decision; other no-cookie providers still honor
        // cookie_source below (the account-source pack's fix for the #619 merge).
        // A wallet provider keeps its saved usage source even with a selected
        // token account: rewriting it to OAuth would skip the wallet read
        // (#725 / upstream base-source resolver).
        let keeps_auto =
            usage_source == SourceMode::Auto && provider.token_account_preserves_auto_source();
        if active_token_env.is_some() && keeps_auto {
            (usage_source, None, false)
        } else if active_token_env.is_some()
            && provider.web_is_opt_in()
            && usage_source != SourceMode::Web
        {
            // Opt-in web providers keep their default credential lane
            // unless the usage source is explicitly Web.
            (usage_source, None, false)
        } else if manual_cookie_wins {
            (SourceMode::Web, stored_cookie.clone(), false)
        } else if active_token_env.is_some() {
            (SourceMode::OAuth, None, false)
        } else {
            (usage_source, None, false)
        }
    } else if cookies_only_enrich_usage {
        let cookie_header = (cookie_source == "manual")
            .then(|| active_token_cookie.clone().or(stored_cookie))
            .flatten();
        (usage_source, cookie_header, false)
    } else {
        match cookie_source {
            _ if manual_cookie_wins => (SourceMode::Web, stored_cookie.clone(), false),
            _ if active_token_env.is_some() => (SourceMode::OAuth, None, false),
            // Opt-in web providers keep their default credential lane
            // unless the usage source is explicitly Web; a stored or
            // browser cookie must not turn Auto into Web.
            _ if provider.web_is_opt_in() && usage_source != SourceMode::Web => {
                (usage_source, None, false)
            }
            // Charm Hyper: the cookie source only picks the session, and
            // the usage source keeps routing. Off and an empty Manual
            // source never import a browser session, while Auto keeps its
            // API-key fallback.
            "off" | "manual" if provider.cookie_source_scopes_session_only() => {
                let cookie_header = if cookie_source == "manual" {
                    active_token_cookie.clone().or(stored_cookie)
                } else {
                    None
                };
                let source_mode = if provider.available_sources().contains(&usage_source) {
                    usage_source
                } else {
                    SourceMode::Auto
                };
                let cookie_missing = cookie_header.is_none();
                (source_mode, cookie_header, cookie_missing)
            }
            "off" if provider_uses_oauth_without_cookies(id, usage_source) => {
                (SourceMode::OAuth, None, false)
            }
            "off"
                if (has_kimi_code_api_key || has_opencodego_api_key)
                    && usage_source == SourceMode::Auto =>
            {
                (SourceMode::Auto, None, false)
            }
            // Cookie-off must never scrape browser cookies (Droid/Factory). Map to
            // Cli (API-only in the provider) so Auto does not fall through to web.
            "off" => (SourceMode::Cli, None, false),
            "manual" => {
                let cookie_header = active_token_cookie.clone().or(stored_cookie);
                let fails_closed_without_cookie = cookie_header
                    .as_deref()
                    .is_none_or(|header| header.trim().is_empty())
                    && provider.manual_empty_cookie_policy()
                        == ManualEmptyCookiePolicy::FailClosedWeb;
                let source_mode = if (has_kimi_code_api_key || has_opencodego_api_key)
                    && usage_source == SourceMode::Auto
                {
                    SourceMode::Auto
                } else if let Some(mode) = grok_source_mode_for_manual_cookie(id, usage_source) {
                    // Grok Switch writes ~/.grok/auth.json. Leftover grok.com
                    // cookies must not force Web, or Weekly/notifications keep
                    // showing the previous browser account.
                    mode
                } else if cookie_header.is_some() {
                    SourceMode::Web
                } else if fails_closed_without_cookie {
                    // The provider owns this policy; Web with no header means
                    // it fails closed instead of importing a browser account
                    // the user did not select.
                    SourceMode::Web
                } else if provider_uses_oauth_without_cookies(id, usage_source) {
                    SourceMode::OAuth
                } else {
                    SourceMode::Cli
                };
                (source_mode, cookie_header, fails_closed_without_cookie)
            }
            // `browser` is accepted as a legacy alias from older settings.
            "auto" | "browser" | "web" => {
                // Claude resolves its cached cookie and browser fallback inside
                // the provider; other providers retain the shell fallback.
                let cookie_header = active_token_cookie.clone().or(stored_cookie).or_else(|| {
                    (!defer_provider_browser_cookie_lookup)
                        .then(|| browser_cookie_header(id, settings))
                        .flatten()
                });
                (usage_source, cookie_header, false)
            }
            _ => (usage_source, stored_cookie, false),
        }
    };

    // Cookie-web providers (Cursor, OpenCode, …) reject SourceMode::Cli. The shell
    // historically mapped "manual + no cookie" to Cli, which surfaces as
    // "Source mode 'Cli' not supported". Remap to Web and try browser cookies
    // unless the user explicitly disabled cookies ("off"). Providers whose
    // cookie source only scopes the session (Charm Hyper) or only enriches an
    // API result (Muse browser team quota) own this contract in the provider,
    // so the shell must not remap their source mode.
    if source_mode == SourceMode::Cli
        && cookie_source != "off"
        && !provider.supports_cli()
        && !provider.cookie_source_scopes_session_only()
        && !cookies_only_enrich_usage
    {
        if cookie_header
            .as_deref()
            .map(str::trim)
            .is_none_or(|s| s.is_empty())
        {
            cookie_header = browser_cookie_header(id, settings);
        }
        source_mode = SourceMode::Web;
    }

    // The inverse case: some providers never fetch over the web and reject
    // SourceMode::Web outright (Codex since the 0.54 port only does OAuth/PAT/CLI).
    // The default cookie source is "manual", so a pasted chatgpt.com cookie flipped
    // Codex into Web and every refresh failed with
    // "Source mode 'Web' not supported for this provider". Fall back to the
    // configured usage source (or Auto) instead of handing the provider a mode it
    // advertises as unsupported.
    let available_sources = provider.available_sources();
    if source_mode == SourceMode::Web && !available_sources.contains(&SourceMode::Web) {
        source_mode = if available_sources.contains(&usage_source) {
            usage_source
        } else {
            SourceMode::Auto
        };
    }

    let workspace_id = settings.workspace_id(id).trim().to_string();
    let api_region = settings.api_region(id).trim().to_string();
    // Every gateway-style provider (Wayfinder, Bifrost, Aixy) stores its base
    // URL here; providers without one report an empty string.
    let gateway_url = Some(settings.gateway_url(id))
        .filter(|url| !url.is_empty())
        .map(str::to_owned);
    // Local-first Auto providers (OpenCode Go) flip to web-first when a
    // token account or manual cookie source scopes the session to web creds.
    let auto_prefer_web = token_override.is_some() || cookie_source == "manual";

    // These upstream account types are explicit identity selections. Keep the
    // provider's saved region/source settings intact, but project the selected
    // credential into the route required by that account.
    let (cookie_header, api_key) = match (id, token_account_kind, usage_source) {
        (ProviderId::Kimi, Some(_), _) => (active_token_cookie.clone(), None),
        (ProviderId::Doubao, Some(_), _) => (None, active_token_api_key.clone()),
        (
            ProviderId::OpenCodeGo,
            Some(codexbar::core::TokenAccountKind::ApiKey),
            SourceMode::Auto,
        ) => (None, active_token_api_key.clone()),
        (ProviderId::OpenCodeGo, Some(codexbar::core::TokenAccountKind::ApiKey), _) => {
            (cookie_header, api_key)
        }
        (
            ProviderId::OpenCodeGo,
            Some(codexbar::core::TokenAccountKind::Cookie),
            SourceMode::Auto,
        ) => (active_token_cookie.clone(), api_key),
        _ => (cookie_header, api_key),
    };
    let source_mode = token_override
        .as_ref()
        .and_then(|account| account.effective_source_mode(usage_source))
        .unwrap_or(source_mode);
    let token_account_isolated = token_override.is_some()
        && matches!(
            id,
            ProviderId::Kimi | ProviderId::Doubao | ProviderId::OpenCodeGo
        );

    FetchContext {
        source_mode,
        manual_cookie_header: cookie_header,
        manual_cookie_missing: fails_closed_without_cookie,
        api_key,
        token_account_kind,
        token_account_isolated,
        workspace_id: (!workspace_id.is_empty()).then_some(workspace_id),
        seat_credit_entitlement: settings.seat_credit_entitlement(id),
        api_region: (!api_region.is_empty()).then_some(api_region),
        gateway_url,
        auto_prefer_web: auto_prefer_web
            && !(id == ProviderId::OpenCodeGo
                && token_account_kind == Some(codexbar::core::TokenAccountKind::ApiKey)),
        browser_cookie_import,
        optional_details_enabled: settings.optional_details_enabled(id),
        ..FetchContext::default()
    }
}

fn browser_cookie_header(id: ProviderId, settings: &Settings) -> Option<String> {
    provider_cookie_domain(id, settings).and_then(|domain| {
        codexbar::browser::cookies::get_cookie_header(domain)
            .ok()
            .filter(|h| !h.is_empty())
    })
}

fn provider_uses_oauth_without_cookies(id: ProviderId, usage_source: SourceMode) -> bool {
    match id {
        ProviderId::Claude => usage_source != SourceMode::Cli,
        ProviderId::Grok => matches!(usage_source, SourceMode::Auto | SourceMode::OAuth),
        _ => false,
    }
}

/// Keep Grok Auto/OAuth/Cli on the switched login instead of rewriting a
/// leftover manual cookie as Web. Auto remains Auto even when its manual
/// cookie is empty; the provider uses that context to skip browser refresh.
fn grok_source_mode_for_manual_cookie(
    id: ProviderId,
    usage_source: SourceMode,
) -> Option<SourceMode> {
    if id != ProviderId::Grok {
        return None;
    }
    match usage_source {
        SourceMode::Cli => Some(SourceMode::Cli),
        SourceMode::Auto => Some(SourceMode::Auto),
        SourceMode::OAuth => Some(SourceMode::OAuth),
        _ => None,
    }
}

pub(crate) fn provider_cookie_domain(id: ProviderId, settings: &Settings) -> Option<&'static str> {
    if id == ProviderId::MiniMax {
        return Some(
            codexbar::providers::MiniMaxProvider::cookie_domain_for_region(Some(
                settings.api_region(id),
            )),
        );
    }
    if id == ProviderId::Alibaba {
        return Some(
            codexbar::providers::AlibabaProvider::cookie_domain_for_region(Some(
                settings.api_region(id),
            )),
        );
    }
    id.cookie_domain()
}

const DEFAULT_PROVIDER_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(35);
const SLOW_PROVIDER_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(75);
const OPTIONAL_LITELLM_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(40);
const MAX_CONTEXT_FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(65);

pub(crate) fn provider_fetch_timeout(id: ProviderId, ctx: &FetchContext) -> std::time::Duration {
    let provider_timeout = match id {
        ProviderId::Claude | ProviderId::Codex | ProviderId::Copilot => SLOW_PROVIDER_FETCH_TIMEOUT,
        ProviderId::LiteLLM if ctx.optional_details_enabled => OPTIONAL_LITELLM_FETCH_TIMEOUT,
        _ => DEFAULT_PROVIDER_FETCH_TIMEOUT,
    };
    let context_timeout = std::time::Duration::from_secs(ctx.web_timeout.saturating_add(5));
    provider_timeout.max(context_timeout.min(MAX_CONTEXT_FETCH_TIMEOUT))
}
