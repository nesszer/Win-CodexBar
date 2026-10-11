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
    let bearer = TokenAccountSupport::env_override(ProviderId::Grok, "Bearer oauth-token").unwrap();
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

    let override_data = TokenAccountOverride::from_account(ProviderId::OpenRouter, active.clone());
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
    let header = TokenAccountSupport::normalized_cookie_header(ProviderId::Claude, "abc123token");
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
