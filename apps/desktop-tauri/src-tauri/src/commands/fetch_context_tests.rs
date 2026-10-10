//! `build_fetch_context` resolution: one table for the plain input/output
//! cases, named tests for the cases with bespoke asserts.

use std::collections::HashMap;

use codexbar::core::{
    FetchContext, ProviderAccountData, ProviderId, SourceMode, TokenAccount, TokenAccountKind,
    instantiate_provider,
};
use codexbar::settings::{ApiKeys, ManualCookies, Settings};

/// Settings, stored secrets and token accounts for one provider. Unset fields
/// keep the defaults a fresh install has.
#[derive(Clone, Copy, Default)]
pub(super) struct CtxInput<'a> {
    pub cookie_source: Option<&'a str>,
    pub usage_source: Option<&'a str>,
    pub region: Option<&'a str>,
    pub manual_cookie: Option<&'a str>,
    pub api_key: Option<&'a str>,
    /// `(label, token)` pairs added in order.
    pub accounts: &'a [(&'a str, &'a str)],
    pub active: Option<usize>,
}

pub(super) fn fetch_ctx(id: ProviderId, input: CtxInput<'_>) -> FetchContext {
    let mut settings = Settings::default();
    if let Some(source) = input.cookie_source {
        settings.set_cookie_source(id, source);
    }
    if let Some(source) = input.usage_source {
        settings.set_usage_source(id, source);
    }
    if let Some(region) = input.region {
        settings.set_api_region(id, region);
    }
    let mut cookies = ManualCookies::default();
    if let Some(header) = input.manual_cookie {
        cookies.set(id.cli_name(), header);
    }
    let mut api_keys = ApiKeys::default();
    if let Some(key) = input.api_key {
        api_keys.set(id.cli_name(), key, None);
    }
    let mut token_accounts = HashMap::new();
    if !input.accounts.is_empty() {
        let mut data = ProviderAccountData::new();
        for &(label, token) in input.accounts {
            data.add_account(TokenAccount::new(label, token));
        }
        if let Some(index) = input.active {
            data.set_active(index);
        }
        token_accounts.insert(id, data);
    }
    super::build_fetch_context(id, &settings, &cookies, &api_keys, &token_accounts)
}

/// Expected context fields; `None` leaves a field unchecked.
#[derive(Default)]
struct Expect<'a> {
    source: Option<SourceMode>,
    cookie: Option<Option<&'a str>>,
    api_key: Option<Option<&'a str>>,
    missing: Option<bool>,
    region: Option<Option<&'a str>>,
    browser_import: Option<bool>,
}

#[test]
fn fetch_context_resolves_each_case() {
    use SourceMode::{Auto, Cli, OAuth, Web};
    let cases: &[(&str, ProviderId, CtxInput, Expect)] = &[
        (
            // Cursor does not support Cli; empty manual cookie remaps to Web (browser attempt).
            "fetch_context_defaults_to_manual_cookies_without_browser_import",
            ProviderId::Cursor,
            CtxInput::default(),
            Expect {
                source: Some(Web),
                ..Default::default()
            },
        ),
        (
            // Explicit cookie-off keeps Cli (no browser scrape).
            "fetch_context_cursor_cookie_off_stays_cli",
            ProviderId::Cursor,
            CtxInput {
                cookie_source: Some("off"),
                ..Default::default()
            },
            Expect {
                source: Some(Cli),
                cookie: Some(None),
                ..Default::default()
            },
        ),
        (
            "fetch_context_grok_cookie_off_preserves_auto_oauth",
            ProviderId::Grok,
            CtxInput {
                cookie_source: Some("off"),
                usage_source: Some("auto"),
                ..Default::default()
            },
            Expect {
                source: Some(OAuth),
                cookie: Some(None),
                ..Default::default()
            },
        ),
        (
            "fetch_context_grok_cookie_off_preserves_explicit_oauth",
            ProviderId::Grok,
            CtxInput {
                cookie_source: Some("off"),
                usage_source: Some("oauth"),
                ..Default::default()
            },
            Expect {
                source: Some(OAuth),
                cookie: Some(None),
                ..Default::default()
            },
        ),
        (
            "fetch_context_grok_cookie_off_keeps_explicit_cli",
            ProviderId::Grok,
            CtxInput {
                cookie_source: Some("off"),
                usage_source: Some("cli"),
                ..Default::default()
            },
            Expect {
                source: Some(Cli),
                cookie: Some(None),
                ..Default::default()
            },
        ),
        (
            "fetch_context_grok_empty_manual_preserves_auto_without_browser_import",
            ProviderId::Grok,
            CtxInput {
                cookie_source: Some("manual"),
                usage_source: Some("auto"),
                ..Default::default()
            },
            Expect {
                source: Some(Auto),
                cookie: Some(None),
                ..Default::default()
            },
        ),
        (
            "fetch_context_grok_manual_cookie_keeps_auto_for_switched_login",
            ProviderId::Grok,
            CtxInput {
                cookie_source: Some("manual"),
                usage_source: Some("auto"),
                manual_cookie: Some("sso=other-account"),
                ..Default::default()
            },
            Expect {
                source: Some(Auto),
                cookie: Some(Some("sso=other-account")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_grok_explicit_web_still_uses_manual_cookie",
            ProviderId::Grok,
            CtxInput {
                cookie_source: Some("manual"),
                usage_source: Some("web"),
                manual_cookie: Some("sso=browser-account"),
                ..Default::default()
            },
            Expect {
                source: Some(Web),
                cookie: Some(Some("sso=browser-account")),
                ..Default::default()
            },
        ),
        (
            // Zed browser billing is opt-in: neither the default manual cookie source
            // nor a stored cookie may turn Auto into Web.
            "fetch_context_zed_default_and_stored_cookie_keep_editor_credential_lane (default)",
            ProviderId::Zed,
            CtxInput::default(),
            Expect {
                source: Some(Auto),
                cookie: Some(None),
                missing: Some(false),
                ..Default::default()
            },
        ),
        (
            "fetch_context_zed_default_and_stored_cookie_keep_editor_credential_lane (stored cookie)",
            ProviderId::Zed,
            CtxInput {
                manual_cookie: Some("zed.session=stored"),
                ..Default::default()
            },
            Expect {
                source: Some(Auto),
                cookie: Some(None),
                ..Default::default()
            },
        ),
        (
            "fetch_context_zed_explicit_web_uses_stored_cookie",
            ProviderId::Zed,
            CtxInput {
                usage_source: Some("web"),
                manual_cookie: Some("zed.session=stored"),
                ..Default::default()
            },
            Expect {
                source: Some(Web),
                cookie: Some(Some("zed.session=stored")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_opencode_empty_manual_remaps_to_web",
            ProviderId::OpenCode,
            CtxInput::default(),
            Expect {
                source: Some(Web),
                ..Default::default()
            },
        ),
        (
            "fetch_context_replicate_empty_manual_fails_closed_without_browser_import",
            ProviderId::Replicate,
            CtxInput::default(),
            Expect {
                source: Some(Web),
                cookie: Some(None),
                missing: Some(true),
                ..Default::default()
            },
        ),
        (
            // Manual with no stored header fails closed instead of using a browser.
            "fetch_context_raycast_cookie_sources_never_import_in_the_shell (empty manual)",
            ProviderId::Raycast,
            CtxInput {
                cookie_source: Some("manual"),
                ..Default::default()
            },
            Expect {
                source: Some(Web),
                cookie: Some(None),
                missing: Some(true),
                ..Default::default()
            },
        ),
        (
            // Off maps to the source the provider refuses, and never carries a header.
            "fetch_context_raycast_cookie_sources_never_import_in_the_shell (off)",
            ProviderId::Raycast,
            CtxInput {
                cookie_source: Some("off"),
                manual_cookie: Some("__raycast_session=stored"),
                ..Default::default()
            },
            Expect {
                source: Some(Cli),
                cookie: Some(None),
                ..Default::default()
            },
        ),
        (
            // Auto leaves browser resolution to the provider (Chrome only).
            "fetch_context_raycast_cookie_sources_never_import_in_the_shell (auto)",
            ProviderId::Raycast,
            CtxInput {
                cookie_source: Some("auto"),
                ..Default::default()
            },
            Expect {
                source: Some(Auto),
                cookie: Some(None),
                missing: Some(false),
                ..Default::default()
            },
        ),
        (
            "fetch_context_raycast_cookie_sources_never_import_in_the_shell (manual)",
            ProviderId::Raycast,
            CtxInput {
                cookie_source: Some("manual"),
                manual_cookie: Some("__raycast_session=stored"),
                ..Default::default()
            },
            Expect {
                source: Some(Web),
                cookie: Some(Some("__raycast_session=stored")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_ollama_empty_manual_fails_closed_without_browser_import",
            ProviderId::Ollama,
            CtxInput::default(),
            Expect {
                source: Some(Web),
                cookie: Some(None),
                missing: Some(true),
                ..Default::default()
            },
        ),
        (
            "fetch_context_ollama_blank_manual_header_counts_as_missing",
            ProviderId::Ollama,
            CtxInput {
                manual_cookie: Some("   "),
                ..Default::default()
            },
            Expect {
                missing: Some(true),
                ..Default::default()
            },
        ),
        (
            "fetch_context_ollama_pasted_header_is_not_reported_missing",
            ProviderId::Ollama,
            CtxInput {
                manual_cookie: Some("__Secure-session=abc"),
                ..Default::default()
            },
            Expect {
                source: Some(Web),
                cookie: Some(Some("__Secure-session=abc")),
                missing: Some(false),
                ..Default::default()
            },
        ),
        (
            "fetch_context_ollama_auto_source_never_reports_manual_cookie_missing",
            ProviderId::Ollama,
            CtxInput {
                cookie_source: Some("auto"),
                ..Default::default()
            },
            Expect {
                missing: Some(false),
                ..Default::default()
            },
        ),
        (
            "fetch_context_codex_manual_cookie_keeps_explicit_supported_source",
            ProviderId::Codex,
            CtxInput {
                usage_source: Some("oauth"),
                manual_cookie: Some("oai-did=abc"),
                ..Default::default()
            },
            Expect {
                source: Some(OAuth),
                ..Default::default()
            },
        ),
        (
            "fetch_context_claude_uses_oauth_without_manual_cookie",
            ProviderId::Claude,
            CtxInput::default(),
            Expect {
                source: Some(OAuth),
                cookie: Some(None),
                ..Default::default()
            },
        ),
        (
            "fetch_context_claude_web_source_defers_cookie_resolution_to_provider",
            ProviderId::Claude,
            CtxInput {
                cookie_source: Some("browser"),
                usage_source: Some("web"),
                ..Default::default()
            },
            Expect {
                source: Some(Web),
                cookie: Some(None),
                ..Default::default()
            },
        ),
        (
            "fetch_context_claude_explicit_cli_source_still_uses_cli",
            ProviderId::Claude,
            CtxInput {
                usage_source: Some("cli"),
                ..Default::default()
            },
            Expect {
                source: Some(Cli),
                cookie: Some(None),
                ..Default::default()
            },
        ),
        (
            "fetch_context_manual_cookie_uses_web_without_browser_import",
            ProviderId::Cursor,
            CtxInput {
                manual_cookie: Some("session=abc123"),
                ..Default::default()
            },
            Expect {
                source: Some(Web),
                cookie: Some(Some("session=abc123")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_api_key_provider_uses_auto_without_cookie_import",
            ProviderId::DeepSeek,
            CtxInput {
                api_key: Some("sk-test"),
                ..Default::default()
            },
            Expect {
                source: Some(Auto),
                cookie: Some(None),
                api_key: Some(Some("sk-test")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_kimi_api_key_preserves_auto_for_web_fallback",
            ProviderId::Kimi,
            CtxInput {
                api_key: Some("sk-kimi-test"),
                ..Default::default()
            },
            Expect {
                source: Some(Auto),
                cookie: Some(None),
                api_key: Some(Some("sk-kimi-test")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_opencodego_api_key_preserves_auto_for_api_overlay",
            ProviderId::OpenCodeGo,
            CtxInput {
                api_key: Some("go-test"),
                ..Default::default()
            },
            Expect {
                source: Some(Auto),
                cookie: Some(None),
                api_key: Some(Some("go-test")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_includes_minimax_region",
            ProviderId::MiniMax,
            CtxInput {
                region: Some("cn"),
                ..Default::default()
            },
            Expect {
                region: Some(Some("cn")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_token_account_uses_web_cookie_header",
            ProviderId::Ollama,
            CtxInput {
                accounts: &[("Work", "abc123")],
                ..Default::default()
            },
            Expect {
                source: Some(Web),
                cookie: Some(Some("__Secure-session=abc123")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_claude_manual_cookie_beats_active_oauth_token_account",
            ProviderId::Claude,
            CtxInput {
                cookie_source: Some("manual"),
                usage_source: Some("auto"),
                manual_cookie: Some("sessionKey=manual-session"),
                accounts: &[("Claude OAuth", "[REDACTED_SECRET]")],
                ..Default::default()
            },
            Expect {
                source: Some(Web),
                cookie: Some(Some("sessionKey=manual-session")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_claude_oauth_token_account_uses_oauth",
            ProviderId::Claude,
            CtxInput {
                accounts: &[("Claude OAuth", "sk-ant-oat01-abc123")],
                ..Default::default()
            },
            Expect {
                source: Some(OAuth),
                cookie: Some(None),
                api_key: Some(Some("sk-ant-oat01-abc123")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_copilot_token_account_uses_oauth_api_key",
            ProviderId::Copilot,
            CtxInput {
                accounts: &[("GitHub", "gho_testtoken")],
                ..Default::default()
            },
            Expect {
                source: Some(OAuth),
                cookie: Some(None),
                api_key: Some(Some("gho_testtoken")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_claude_session_token_account_uses_web_cookie",
            ProviderId::Claude,
            CtxInput {
                accounts: &[("Claude Web", "sessionKey=abc123")],
                ..Default::default()
            },
            Expect {
                source: Some(Web),
                cookie: Some(Some("sessionKey=abc123")),
                api_key: Some(None),
                ..Default::default()
            },
        ),
        (
            "fetch_context_token_account_takes_precedence_over_manual_cookie",
            ProviderId::Cursor,
            CtxInput {
                manual_cookie: Some("manual=old"),
                accounts: &[("Work", "WorkosCursorSessionToken=new")],
                ..Default::default()
            },
            Expect {
                source: Some(Web),
                cookie: Some(Some("WorkosCursorSessionToken=new")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_openrouter_token_account_overrides_stored_api_key",
            ProviderId::OpenRouter,
            CtxInput {
                api_key: Some("sk-or-v1-stored-decoy"),
                accounts: &[("Personal", "sk-or-v1-personal"), ("Work", "sk-or-v1-work")],
                active: Some(1),
                ..Default::default()
            },
            Expect {
                source: Some(OAuth),
                cookie: Some(None),
                api_key: Some(Some("sk-or-v1-work")),
                ..Default::default()
            },
        ),
        (
            "fetch_context_openrouter_falls_back_to_stored_api_key_without_token_accounts",
            ProviderId::OpenRouter,
            CtxInput {
                api_key: Some("sk-or-v1-stored"),
                ..Default::default()
            },
            Expect {
                api_key: Some(Some("sk-or-v1-stored")),
                ..Default::default()
            },
        ),
        (
            "muse_default_cookie_source_reads_no_browser_and_keeps_the_login_source",
            ProviderId::Muse,
            CtxInput::default(),
            Expect {
                source: Some(Auto),
                cookie: Some(None),
                browser_import: Some(false),
                ..Default::default()
            },
        ),
        (
            "muse_cookie_source_off_never_reads_or_forwards_a_cookie",
            ProviderId::Muse,
            CtxInput {
                cookie_source: Some("off"),
                manual_cookie: Some("llama_dev_sess=abc"),
                ..Default::default()
            },
            Expect {
                cookie: Some(None),
                browser_import: Some(false),
                ..Default::default()
            },
        ),
        (
            "muse_automatic_cookie_source_requests_the_browser_import",
            ProviderId::Muse,
            CtxInput {
                cookie_source: Some("auto"),
                manual_cookie: Some("llama_dev_sess=abc"),
                ..Default::default()
            },
            Expect {
                source: Some(Auto),
                cookie: Some(None),
                browser_import: Some(true),
                ..Default::default()
            },
        ),
        (
            "muse_manual_cookie_source_forwards_only_the_pasted_header",
            ProviderId::Muse,
            CtxInput {
                cookie_source: Some("manual"),
                manual_cookie: Some("llama_dev_sess=abc"),
                ..Default::default()
            },
            Expect {
                cookie: Some(Some("llama_dev_sess=abc")),
                browser_import: Some(false),
                ..Default::default()
            },
        ),
        (
            "other_providers_never_request_the_muse_browser_import",
            ProviderId::Cursor,
            CtxInput::default(),
            Expect {
                browser_import: Some(false),
                ..Default::default()
            },
        ),
    ];

    for (name, id, input, expect) in cases {
        let ctx = fetch_ctx(*id, *input);
        if let Some(source) = expect.source {
            assert_eq!(ctx.source_mode, source, "{name}: source_mode");
        }
        if let Some(cookie) = expect.cookie {
            assert_eq!(
                ctx.manual_cookie_header.as_deref(),
                cookie,
                "{name}: manual_cookie_header"
            );
        }
        if let Some(api_key) = expect.api_key {
            assert_eq!(ctx.api_key.as_deref(), api_key, "{name}: api_key");
        }
        if let Some(missing) = expect.missing {
            assert_eq!(ctx.manual_cookie_missing, missing, "{name}: missing");
        }
        if let Some(region) = expect.region {
            assert_eq!(ctx.api_region.as_deref(), region, "{name}: api_region");
        }
        if let Some(browser_import) = expect.browser_import {
            assert_eq!(
                ctx.browser_cookie_import, browser_import,
                "{name}: browser_cookie_import"
            );
        }
    }
}

#[test]
fn fetch_context_carries_the_optional_details_opt_in_for_its_own_provider() {
    let mut settings = Settings::default();
    settings.set_optional_details_enabled(ProviderId::LiteLLM, true);
    let build = |id| {
        super::build_fetch_context(
            id,
            &settings,
            &ManualCookies::default(),
            &ApiKeys::default(),
            &HashMap::new(),
        )
    };

    assert!(build(ProviderId::LiteLLM).optional_details_enabled);
    assert!(!build(ProviderId::Codex).optional_details_enabled);
}

#[test]
fn fetch_context_codex_manual_cookie_never_forces_unsupported_web() {
    // Default cookie source is "manual". Pasting a chatgpt.com cookie used to flip
    // Codex into SourceMode::Web, which CodexProvider rejects with
    // "Source mode 'Web' not supported for this provider" on every refresh.
    let ctx = fetch_ctx(
        ProviderId::Codex,
        CtxInput {
            manual_cookie: Some("oai-did=abc; __Secure-next-auth.session-token=xyz"),
            ..Default::default()
        },
    );

    assert_eq!(ctx.source_mode, SourceMode::Auto);
    assert!(
        instantiate_provider(ProviderId::Codex)
            .available_sources()
            .contains(&ctx.source_mode)
    );
}

#[test]
fn kimi_selected_account_forces_web_and_keeps_saved_region() {
    let mut settings = Settings::default();
    settings.set_usage_source(ProviderId::Kimi, "oauth");
    settings.set_api_region(ProviderId::Kimi, "international");
    let mut accounts = HashMap::new();
    let mut data = ProviderAccountData::new();
    data.add_account(TokenAccount::new("Work", "selected-kimi-session"));
    accounts.insert(ProviderId::Kimi, data);

    let ctx = super::build_fetch_context(
        ProviderId::Kimi,
        &settings,
        &ManualCookies::default(),
        &ApiKeys::default(),
        &accounts,
    );

    assert_eq!(ctx.source_mode, SourceMode::Web);
    assert_eq!(
        ctx.manual_cookie_header.as_deref(),
        Some("kimi-auth=selected-kimi-session")
    );
    assert_eq!(ctx.api_key, None);
    assert_eq!(ctx.api_region.as_deref(), Some("international"));
    assert!(ctx.token_account_isolated);
    assert_eq!(settings.usage_source(ProviderId::Kimi), "oauth");
    assert_eq!(settings.api_region(ProviderId::Kimi), "international");
}

#[test]
fn doubao_selected_account_forces_ark_api_and_ignores_saved_source() {
    let ctx = fetch_ctx(
        ProviderId::Doubao,
        CtxInput {
            usage_source: Some("cli"),
            accounts: &[("Work", "selected-ark-key")],
            ..Default::default()
        },
    );

    assert_eq!(ctx.source_mode, SourceMode::OAuth);
    assert_eq!(ctx.api_key.as_deref(), Some("selected-ark-key"));
    assert!(ctx.token_account_isolated);
}

#[test]
fn opencodego_selected_api_account_overrides_global_key_without_changing_explicit_source() {
    let build = |cookie_source, usage_source| {
        fetch_ctx(
            ProviderId::OpenCodeGo,
            CtxInput {
                cookie_source,
                usage_source: Some(usage_source),
                api_key: Some("global-key"),
                accounts: &[("Work", "selected-account-key")],
                ..Default::default()
            },
        )
    };

    let ctx = build(None, "auto");
    assert_eq!(ctx.source_mode, SourceMode::Auto);
    assert_eq!(ctx.api_key.as_deref(), Some("selected-account-key"));
    assert!(!ctx.auto_prefer_web);
    assert!(ctx.token_account_isolated);

    for cookie_source in ["off", "manual"] {
        let auto_ctx = build(Some(cookie_source), "auto");
        assert_eq!(auto_ctx.source_mode, SourceMode::Auto);
        assert_eq!(auto_ctx.api_key.as_deref(), Some("selected-account-key"));
        assert!(auto_ctx.manual_cookie_header.is_none());
    }

    for (saved_source, expected_source) in [("web", SourceMode::Web), ("cli", SourceMode::Cli)] {
        let explicit_ctx = build(Some("off"), saved_source);
        assert_eq!(explicit_ctx.source_mode, expected_source);
    }
}

#[test]
fn opencodego_selected_cookie_account_uses_web_route() {
    let ctx = fetch_ctx(
        ProviderId::OpenCodeGo,
        CtxInput {
            accounts: &[("Web", "Cookie: session=selected-session")],
            ..Default::default()
        },
    );

    assert_eq!(ctx.source_mode, SourceMode::Web);
    assert_eq!(
        ctx.manual_cookie_header.as_deref(),
        Some("Cookie: session=selected-session")
    );
    assert_eq!(ctx.token_account_kind, Some(TokenAccountKind::Cookie));
    assert!(ctx.token_account_isolated);
}
