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
    "CommandCode|Session tokens|Store multiple Command Code subscription sessions.|Cookie: __Secure-commandcode_prod_.session_token=... or better-auth value|cookie|true|Some(\"__Secure-commandcode_prod_.session_token\")",
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
    "IBMBob|API keys|Store multiple IBM Bob API keys.|Paste API key…|env:BOBSHELL_API_KEY|false|None",
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
fn command_code_bare_token_uses_production_cookie_name() {
    // Upstream #4212: a pasted bare token gets the live production cookie name.
    assert_eq!(
        TokenAccountSupport::normalized_cookie_header(ProviderId::CommandCode, "bare-value"),
        "__Secure-commandcode_prod_.session_token=bare-value"
    );
    // Explicit cookie headers are preserved untouched.
    assert_eq!(
        TokenAccountSupport::normalized_cookie_header(
            ProviderId::CommandCode,
            "__Secure-better-auth.session_token=legacy"
        ),
        "__Secure-better-auth.session_token=legacy"
    );
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
fn ibmbob_token_accounts_inject_bobshell_api_key_env() {
    let support = TokenAccountSupport::for_provider(ProviderId::IBMBob).unwrap();
    assert_eq!(support.title, "API keys");
    assert_eq!(support.subtitle, "Store multiple IBM Bob API keys.");
    assert_eq!(support.placeholder, "Paste API key…");
    assert!(!support.requires_manual_cookie_source);
    let env = TokenAccountSupport::env_override(ProviderId::IBMBob, "bob_fixture").unwrap();
    assert_eq!(
        env.get("BOBSHELL_API_KEY").map(String::as_str),
        Some("bob_fixture")
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

/// Three synthetic accounts; the middle one is active. Returns their ids in list order.
fn edit_fixture() -> (ProviderAccountData, Uuid, Uuid, Uuid) {
    let mut data = ProviderAccountData::new();
    let mut first = TokenAccount::new("First", "fixture-first");
    first.external_identifier = Some("fixture-ext".to_string());
    first.claude_config_directory = Some("/fixture/claude".to_string());
    first.added_at = 1_700_000_000;
    let second = TokenAccount::new("Second", "fixture-second");
    let third = TokenAccount::new("Third", "fixture-third");
    let ids = (first.id, second.id, third.id);
    data.add_account(first);
    data.add_account(second);
    data.add_account(third);
    data.set_active(1);
    (data, ids.0, ids.1, ids.2)
}

#[test]
fn update_account_replaces_label_and_credential_and_keeps_identity() {
    let (mut data, first_id, _, _) = edit_fixture();
    let before = data.accounts[0].clone();

    data.update_account(first_id, Some(" Renamed "), Some(" fixture-replacement "))
        .expect("known id updates");

    let edited = &data.accounts[0];
    assert_eq!(edited.id, first_id);
    assert_eq!(edited.label, "Renamed");
    assert_eq!(edited.token, "fixture-replacement");
    assert_eq!(edited.added_at, before.added_at);
    assert_eq!(edited.external_identifier, before.external_identifier);
    assert_eq!(
        edited.claude_config_directory,
        before.claude_config_directory
    );
    assert_eq!(
        data.active_index, 1,
        "editing never moves the active selection"
    );
}

#[test]
fn update_account_blank_or_missing_credential_keeps_saved_token() {
    let (mut data, first_id, _, _) = edit_fixture();

    data.update_account(first_id, Some("Only label"), None)
        .unwrap();
    assert_eq!(data.accounts[0].token, "fixture-first");
    assert_eq!(data.accounts[0].label, "Only label");

    data.update_account(first_id, Some("Blank token"), Some(" \n "))
        .unwrap();
    assert_eq!(data.accounts[0].token, "fixture-first");
    assert_eq!(data.accounts[0].label, "Blank token");
}

#[test]
fn update_account_blank_label_keeps_saved_label() {
    let (mut data, first_id, _, _) = edit_fixture();

    data.update_account(first_id, Some("   "), Some("fixture-new"))
        .unwrap();

    assert_eq!(data.accounts[0].label, "First");
    assert_eq!(data.accounts[0].token, "fixture-new");
}

#[test]
fn update_account_rejects_unknown_id_without_changes() {
    let (mut data, _, _, _) = edit_fixture();
    let before = data.accounts.clone();

    let err = data
        .update_account(Uuid::new_v4(), Some("Ghost"), Some("fixture-ghost"))
        .unwrap_err();

    assert_eq!(err, TokenAccountEditError::NotFound);
    assert_eq!(data.accounts.len(), before.len());
    assert_eq!(data.accounts[0].label, before[0].label);
    assert_eq!(data.accounts[0].token, before[0].token);
}

#[test]
fn move_account_keeps_the_same_account_active() {
    let (mut data, first_id, second_id, third_id) = edit_fixture();
    assert_eq!(data.active_account().unwrap().id, second_id);

    data.move_account(second_id, 0).unwrap();
    assert_eq!(
        data.accounts.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![second_id, first_id, third_id]
    );
    assert_eq!(data.active_account().unwrap().id, second_id);

    data.move_account(second_id, 2).unwrap();
    assert_eq!(
        data.accounts.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![first_id, third_id, second_id]
    );
    assert_eq!(data.active_account().unwrap().id, second_id);
    assert_eq!(data.active_index, 2);

    data.move_account(first_id, 1).unwrap();
    assert_eq!(data.active_account().unwrap().id, second_id);
    assert_eq!(data.active_index, 2);
}

#[test]
fn move_account_moves_other_accounts_past_the_active_one() {
    let (mut data, first_id, second_id, third_id) = edit_fixture();

    data.move_account(third_id, 0).unwrap();

    assert_eq!(
        data.accounts.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![third_id, first_id, second_id]
    );
    assert_eq!(data.active_account().unwrap().id, second_id);
    assert_eq!(data.active_index, 2);
}

#[test]
fn move_account_rejects_unknown_id_and_out_of_range_destination() {
    let (mut data, first_id, second_id, third_id) = edit_fixture();
    let order = vec![first_id, second_id, third_id];

    assert_eq!(
        data.move_account(Uuid::new_v4(), 0),
        Err(TokenAccountEditError::NotFound)
    );
    assert_eq!(
        data.move_account(first_id, 3),
        Err(TokenAccountEditError::OutOfRange)
    );
    assert_eq!(
        data.accounts.iter().map(|a| a.id).collect::<Vec<_>>(),
        order
    );
    assert_eq!(data.active_account().unwrap().id, second_id);
}

#[test]
fn move_account_to_its_own_position_is_a_no_op() {
    let (mut data, first_id, second_id, third_id) = edit_fixture();

    data.move_account(first_id, 0).unwrap();

    assert_eq!(
        data.accounts.iter().map(|a| a.id).collect::<Vec<_>>(),
        vec![first_id, second_id, third_id]
    );
    assert_eq!(data.active_index, 1);
}
