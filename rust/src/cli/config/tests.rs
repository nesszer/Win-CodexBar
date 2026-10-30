use super::{
    ClaudeCodeCredentialsConsent, ConfigCommand, ConfigFileError, ConfigOutputArgs,
    ConfigSetSourceResult, ConsentAction, CredentialField, apply_consent_action, apply_set_cookie,
    clean_cookie_header, clear_stored_credentials, config_cookie_provider, config_source_selection,
    consent_text, credential_fields, load_credential_store, provider_statuses, read_json_config,
    sanitize_settings_for_dump, source_mode_name,
};
use crate::cli::{Cli, Commands};
use crate::core::{ProviderId, SourceMode};
#[cfg(windows)]
use crate::secure_file;
use crate::settings::{ManualCookies, Settings};
use clap::Parser;
use serde_json::json;

#[test]
fn preferences_subcommands_parse_a_path() {
    use super::{ConfigArgs, ConfigCommand, PreferencesAction};
    use clap::Parser;

    let export =
        ConfigArgs::try_parse_from(["config", "preferences", "export", "--file", "p.json"])
            .expect("export parses");
    assert!(matches!(
        export.command,
        ConfigCommand::Preferences {
            action: PreferencesAction::Export { file: Some(_) }
        }
    ));
    let stdout = ConfigArgs::try_parse_from(["config", "preferences", "export"])
        .expect("export without --file parses");
    assert!(matches!(
        stdout.command,
        ConfigCommand::Preferences {
            action: PreferencesAction::Export { file: None }
        }
    ));
    let import =
        ConfigArgs::try_parse_from(["config", "preferences", "import", "--file", "p.json"])
            .expect("import parses");
    assert!(matches!(
        import.command,
        ConfigCommand::Preferences {
            action: PreferencesAction::Import { .. }
        }
    ));
    assert!(ConfigArgs::try_parse_from(["config", "preferences", "import"]).is_err());
}

fn parse_config(argv: &[&str]) -> Result<ConfigCommand, clap::Error> {
    let cli = Cli::try_parse_from(["codexbar", "config"].iter().chain(argv.iter()))?;
    match cli.command {
        Some(Commands::Config(args)) => Ok(args.command),
        other => panic!("expected the config command, got {other:?}"),
    }
}

#[test]
fn config_providers_accepts_json_output_flags() {
    let ConfigCommand::Providers { output } = parse_config(&["providers"]).unwrap() else {
        panic!("expected providers");
    };
    assert!(!output.is_json());
    for argv in [
        &["providers", "--json"][..],
        &["providers", "--format", "json"][..],
        &["providers", "-f", "json", "--pretty"][..],
    ] {
        let ConfigCommand::Providers { output } = parse_config(argv).unwrap() else {
            panic!("expected providers for {argv:?}");
        };
        assert!(output.is_json(), "{argv:?}");
    }
    assert!(parse_config(&["providers", "--format", "toml"]).is_err());
}

#[test]
fn provider_status_json_matches_the_upstream_shape() {
    let mut settings = Settings::default();
    settings.enabled_providers.clear();
    settings.enable_provider(ProviderId::Cursor);

    let statuses = provider_statuses(&settings);

    assert_eq!(
        statuses.len(),
        ProviderId::all()
            .iter()
            .filter(|p| !p.is_deprecated())
            .count()
    );
    let cursor = statuses
        .iter()
        .find(|status| status.provider == "cursor")
        .expect("cursor row");
    assert!(cursor.enabled);
    let codex = statuses
        .iter()
        .find(|status| status.provider == "codex")
        .expect("codex row");
    assert!(!codex.enabled);

    let value = serde_json::to_value(codex).unwrap();
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["defaultEnabled", "displayName", "enabled", "provider"]
    );
    assert_eq!(value["displayName"], ProviderId::Codex.display_name());
    assert_eq!(value["enabled"], false);
    assert_eq!(value["defaultEnabled"], codex.default_enabled);
}

#[test]
fn deprecated_providers_are_hidden_until_enabled_then_listed() {
    let mut settings = Settings::default();
    settings.enabled_providers.clear();
    let statuses = provider_statuses(&settings);
    for retired in [ProviderId::KimiK2, ProviderId::CrossModel] {
        assert!(retired.is_deprecated());
        assert!(statuses.iter().all(|s| s.provider != retired.cli_name()));
    }

    settings.enable_provider(ProviderId::KimiK2);
    let statuses = provider_statuses(&settings);
    let kimi = statuses
        .iter()
        .find(|s| s.provider == "kimik2")
        .expect("enabled deprecated provider is listed");
    assert!(kimi.enabled);
    assert!(statuses.iter().all(|s| s.provider != "crossmodel"));
}

#[test]
fn provider_status_text_lines_keep_the_existing_format() {
    let mut settings = Settings::default();
    settings.enabled_providers.clear();
    settings.enable_provider(ProviderId::Codex);
    let statuses = provider_statuses(&settings);
    let codex = statuses.iter().find(|s| s.provider == "codex").unwrap();
    let claude = statuses.iter().find(|s| s.provider == "claude").unwrap();

    let marker = |default: bool| if default { " default" } else { "" };
    assert_eq!(
        codex.text_line(),
        format!("codex: enabled{} (Codex)", marker(codex.default_enabled))
    );
    assert_eq!(
        claude.text_line(),
        format!(
            "claude: disabled{} (Claude)",
            marker(claude.default_enabled)
        )
    );
}

#[test]
fn provider_subcommands_accept_the_positional_or_provider_option() {
    for argv in [
        &["enable", "cursor"][..],
        &["enable", "-p", "cursor"][..],
        &["enable", "--provider", "cursor", "--json"][..],
    ] {
        let ConfigCommand::Enable { target, .. } = parse_config(argv).unwrap() else {
            panic!("expected enable for {argv:?}");
        };
        assert_eq!(target.name(), "cursor", "{argv:?}");
    }
    let ConfigCommand::Disable { target, output } =
        parse_config(&["disable", "--provider", "cursor", "--format", "json"]).unwrap()
    else {
        panic!("expected disable");
    };
    assert_eq!(target.name(), "cursor");
    assert!(output.is_json());
    let ConfigCommand::SetApiKey { target, stdin, .. } =
        parse_config(&["set-api-key", "-p", "openrouter", "--stdin"]).unwrap()
    else {
        panic!("expected set-api-key");
    };
    assert_eq!(target.name(), "openrouter");
    assert!(stdin);

    assert!(parse_config(&["enable"]).is_err(), "provider is required");
    assert!(
        parse_config(&["enable", "cursor", "--provider", "codex"]).is_err(),
        "positional and --provider conflict"
    );
}

#[test]
fn set_source_parses_provider_and_source() {
    for argv in [
        &["set-source", "codex", "--source", "cli"][..],
        &[
            "set-source",
            "--provider",
            "codex",
            "--source",
            "cli",
            "--json",
        ][..],
    ] {
        let ConfigCommand::SetSource { target, source, .. } = parse_config(argv).unwrap() else {
            panic!("expected set-source for {argv:?}");
        };
        assert_eq!(target.name(), "codex");
        assert_eq!(source, "cli");
    }
    assert!(
        parse_config(&["set-source", "codex"]).is_err(),
        "--source is required"
    );
    assert!(
        parse_config(&["set-source", "--source", "cli"]).is_err(),
        "provider is required"
    );
}

#[test]
fn set_source_validates_provider_and_supported_source() {
    let (id, mode) = config_source_selection("Codex", "CLI").unwrap();
    assert_eq!(id, ProviderId::Codex);
    assert_eq!(mode, SourceMode::Cli);
    assert_eq!(
        config_source_selection("codex", "auto").unwrap().1,
        SourceMode::Auto
    );

    let unknown_provider = config_source_selection("nosuch", "cli").unwrap_err();
    assert!(unknown_provider.to_string().contains("Unknown provider"));

    let unknown_source = config_source_selection("codex", "api").unwrap_err();
    assert!(
        unknown_source
            .to_string()
            .contains("Use --source auto|web|cli|oauth")
    );

    let unsupported = config_source_selection("codex", "web").unwrap_err();
    assert_eq!(
        unsupported.to_string(),
        "Source web is not supported for codex. Supported sources: auto, cli, oauth."
    );
}

#[test]
fn set_source_persists_without_changing_enablement() {
    let mut settings = Settings::default();
    settings.enabled_providers.clear();
    let (id, mode) = config_source_selection("codex", "oauth").unwrap();
    settings.set_usage_source(id, source_mode_name(mode));

    let json = serde_json::to_string(&settings).unwrap();
    let loaded: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(loaded.usage_source(ProviderId::Codex), "oauth");
    assert!(!loaded.is_provider_enabled(ProviderId::Codex));

    let mut loaded = loaded;
    loaded.set_usage_source(id, source_mode_name(SourceMode::Auto));
    assert_eq!(loaded.usage_source(ProviderId::Codex), "auto");
}

#[test]
fn set_source_json_matches_the_upstream_shape() {
    let value = serde_json::to_value(ConfigSetSourceResult {
        provider: "codex".to_string(),
        display_name: "Codex".to_string(),
        enabled: true,
        source: "cli".to_string(),
        config_path: Some("settings.json".to_string()),
    })
    .unwrap();
    assert_eq!(
        value,
        json!({
            "provider": "codex",
            "displayName": "Codex",
            "enabled": true,
            "source": "cli",
            "configPath": "settings.json"
        })
    );
}

#[test]
fn default_output_args_are_text() {
    assert!(!ConfigOutputArgs::default().is_json());
}

#[test]
fn claude_code_credentials_parses_each_action() {
    for (word, expected) in [
        ("allow", ConsentAction::Allow),
        ("deny", ConsentAction::Deny),
        ("status", ConsentAction::Status),
    ] {
        let ConfigCommand::ClaudeCodeCredentials { action, output } =
            parse_config(&["claude-code-credentials", word, "--json"]).unwrap()
        else {
            panic!("expected claude-code-credentials for {word}");
        };
        assert_eq!(action, expected);
        assert!(output.is_json());
    }
    assert!(parse_config(&["claude-code-credentials"]).is_err());
    assert!(parse_config(&["claude-code-credentials", "maybe"]).is_err());
}

#[test]
fn consent_actions_toggle_only_the_claude_code_flag() {
    let mut settings = Settings::default();
    assert!(!settings.claude_allow_reading_claude_code_credentials);
    let before = serde_json::to_value(&settings).unwrap();

    assert!(!apply_consent_action(&mut settings, ConsentAction::Status));
    assert!(!apply_consent_action(&mut settings, ConsentAction::Deny));
    assert!(apply_consent_action(&mut settings, ConsentAction::Allow));
    assert!(settings.claude_allow_reading_claude_code_credentials);
    assert!(!apply_consent_action(&mut settings, ConsentAction::Allow));
    assert!(!apply_consent_action(&mut settings, ConsentAction::Status));
    assert!(settings.claude_allow_reading_claude_code_credentials);

    let mut after = serde_json::to_value(&settings).unwrap();
    after["claude_allow_reading_claude_code_credentials"] = json!(false);
    assert_eq!(after, before, "no other setting changes");

    assert!(apply_consent_action(&mut settings, ConsentAction::Deny));
    assert!(!settings.claude_allow_reading_claude_code_credentials);
}

#[test]
fn consent_output_reports_the_choice() {
    let value = serde_json::to_value(ClaudeCodeCredentialsConsent {
        allowed: true,
        config_path: Some("settings.json".to_string()),
    })
    .unwrap();
    assert_eq!(
        value,
        json!({ "allowed": true, "configPath": "settings.json" })
    );
    assert!(consent_text(true).contains("allowed"));
    assert!(consent_text(false).contains("not allowed"));
}

#[cfg(windows)]
#[test]
fn validates_dpapi_protected_manual_cookies_without_plaintext() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let path = dir.path().join("manual_cookies.json");
    let expected = r#"{"cookies":{}}"#;

    secure_file::write_string(&path, expected).expect("write protected cookies");

    let raw = std::fs::read_to_string(&path).expect("read protected wrapper");
    assert!(raw.contains("\"format\": \"codexbar.secure-file\""));
    assert!(!raw.contains(expected));
    assert!(
        read_json_config::<ManualCookies>(&path).is_ok(),
        "valid protected cookies should validate"
    );
}

#[test]
fn rejects_malformed_secure_wrapper_without_echoing_payload() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let path = dir.path().join("manual_cookies.json");
    let payload = "AA==";
    let malformed = format!(
        r#"{{"format":"codexbar.secure-file","version":1,"protection":"unsupported","payload":"{payload}"}}"#
    );
    std::fs::write(&path, malformed).expect("write malformed wrapper");

    let error =
        read_json_config::<ManualCookies>(&path).expect_err("reject malformed protected wrapper");
    let ConfigFileError::Read(error) = error else {
        panic!("malformed protected wrapper must be a read error");
    };
    let message = error.to_string();
    assert!(message.contains("unsupported secure file protection"));
    assert!(!message.contains(payload));
}

#[test]
fn sanitize_settings_for_dump_redacts_secret_fields() {
    let raw = json!({
        "http_proxy_password": "proxy-secret",
        "provider_configs": {
            "claude": {
                "manual_cookie_header": "sessionKey=abc",
                "api_token": "tok-123",
                "management_api_token": "management-secret"
            }
        },
        "api_keys": {
            "keys": {
                "zai": {
                    "api_key": "cb_test_api_key_456",
                    "label": "Work",
                    "saved_at": "2026-01-01"
                }
            }
        },
        "manual_cookies": {
            "cookies": {
                "claude": {
                    "cookie_header": "sessionKey=secret-cookie",
                    "saved_at": "2026-01-01"
                }
            }
        },
        "token_accounts": {
            "factory": {
                "accounts": [{
                    "id": "11111111-1111-1111-1111-111111111111",
                    "label": "Team",
                    "token": "raw-token-value",
                    "added_at": 1
                }]
            }
        },
        "secret_key": "top-secret"
    });

    let redacted = sanitize_settings_for_dump(raw, false);
    let text = serde_json::to_string(&redacted).expect("serialize");

    assert!(text.contains("[REDACTED]"));
    assert!(!text.contains("proxy-secret"));
    assert!(!text.contains("sessionKey=abc"));
    assert!(!text.contains("tok-123"));
    assert!(!text.contains("management-secret"));
    assert!(!text.contains("cb_test_api_key_456"));
    assert!(!text.contains("secret-cookie"));
    assert!(!text.contains("raw-token-value"));
    assert!(!text.contains("top-secret"));
    // Non-secret identity fields stay.
    assert!(text.contains("Work"));
    assert!(text.contains("Team"));
    assert!(text.contains("11111111-1111-1111-1111-111111111111"));
}

#[test]
fn sanitize_settings_for_dump_show_secrets_keeps_raw() {
    let raw = json!({
        "api_key": "keep-me",
        "nested": { "token": "also-keep", "label": "Team" }
    });

    let out = sanitize_settings_for_dump(raw.clone(), true);
    assert_eq!(out, raw);
    assert_eq!(out["api_key"], "keep-me");
    assert_eq!(out["nested"]["token"], "also-keep");
}

#[test]
fn config_login_parses_the_positional_or_provider_option() {
    let ConfigCommand::Login { target, output } =
        parse_config(&["login", "--provider", "copilot", "--json"]).unwrap()
    else {
        panic!("expected login");
    };
    assert_eq!(target.name(), "copilot");
    assert!(output.is_json());

    let ConfigCommand::Login { target, output } =
        parse_config(&["login", "-p", "copilot", "--format", "json", "--pretty"]).unwrap()
    else {
        panic!("expected login");
    };
    assert_eq!(target.name(), "copilot");
    assert!(output.is_json());

    let ConfigCommand::Login { target, output } = parse_config(&["login", "copilot"]).unwrap()
    else {
        panic!("expected login");
    };
    assert_eq!(target.name(), "copilot");
    assert!(!output.is_json());

    assert!(parse_config(&["login"]).is_err(), "provider is required");
}

#[tokio::test]
async fn config_login_rejects_other_providers_before_any_io() {
    let error = super::login("cursor", ConfigOutputArgs::default())
        .await
        .expect_err("cursor has no CLI login");
    assert_eq!(
        error.to_string(),
        "Login is supported for copilot, grok and kiro. Use --provider copilot, grok or kiro."
    );
    assert_eq!(error.exit_code(), crate::cli::exit_codes::USAGE_ERROR);
}

#[test]
fn enable_failure_says_whether_codexbar_saved_the_account() {
    let grok = super::enable_failed(ProviderId::Grok, true, "settings write failed");
    assert_eq!(
        grok.to_string(),
        "Signed in and saved the account, but could not enable Grok: settings write failed"
    );
    assert_eq!(grok.exit_code(), 1);

    let kiro = super::enable_failed(ProviderId::Kiro, false, "settings write failed");
    assert_eq!(
        kiro.to_string(),
        "Signed in, but could not enable Kiro: settings write failed"
    );
}

mod copilot_login_output {
    use std::cell::Cell;
    use std::future::Future;

    use super::super::{
        ConfigLoginError, CopilotApi, CopilotDeviceFlow, TokenAccountStore, run_copilot_login,
    };
    use crate::core::ProviderId;
    use crate::providers::copilot::endpoints::GitHubEndpoints;
    use crate::providers::copilot::login::SavedCopilotAccount;
    use crate::providers::copilot::test_server::{FakeGitHub, Reply, ok};

    const DEVICE_JSON: &str = r#"{"device_code":"device-example","user_code":"ABCD-1234","verification_uri":"https://github.com/login/device","expires_in":900,"interval":0}"#;
    const TOKEN_JSON: &str =
        r#"{"access_token":"gho_example_token","token_type":"bearer","scope":"read:user"}"#;
    const USER_JSON: &str = r#"{"login":"octocat","id":42}"#;
    const USAGE_JSON: &str = r#"{"copilot_plan":"individual","quota_snapshots":{}}"#;

    fn fake(token_replies: Vec<Reply>) -> FakeGitHub {
        FakeGitHub::start(vec![
            ("/login/device/code", vec![ok(DEVICE_JSON)]),
            ("/login/oauth/access_token", token_replies),
            ("/user", vec![ok(USER_JSON)]),
            ("/copilot_internal/user", vec![ok(USAGE_JSON)]),
        ])
    }

    /// One login run. `_dir` must stay alive while the store is read back.
    struct Run {
        result: Result<SavedCopilotAccount, ConfigLoginError>,
        out: String,
        store: TokenAccountStore,
        enable_calls: u32,
        _dir: tempfile::TempDir,
    }

    async fn run(fake: &FakeGitHub, json: bool) -> Run {
        run_with_cancel(fake, json, std::future::pending()).await
    }

    async fn run_with_cancel(
        fake: &FakeGitHub,
        json: bool,
        cancel: impl Future<Output = ()>,
    ) -> Run {
        run_with_options(fake, json, cancel, false).await
    }

    async fn run_with_options(
        fake: &FakeGitHub,
        json: bool,
        cancel: impl Future<Output = ()>,
        enable_fails: bool,
    ) -> Run {
        let endpoints = GitHubEndpoints::with_origin(&fake.origin).expect("loopback origin");
        let flow = CopilotDeviceFlow::with_endpoints(&endpoints);
        let api = CopilotApi::with_endpoints(endpoints);
        let dir = tempfile::tempdir().expect("tempdir");
        let store = TokenAccountStore::with_path(dir.path().join("token-accounts.json"));
        let mut out = Vec::new();
        let enable_calls = Cell::new(0);
        let result = run_copilot_login(&flow, &api, &store, json, &mut out, cancel, || {
            enable_calls.set(enable_calls.get() + 1);
            if enable_fails {
                anyhow::bail!("settings write failed");
            }
            Ok(())
        })
        .await;
        Run {
            result,
            out: String::from_utf8(out).expect("utf8"),
            store,
            enable_calls: enable_calls.get(),
            _dir: dir,
        }
    }

    #[tokio::test]
    async fn text_output_shows_url_and_code_then_connected_and_never_the_token() {
        let fake = fake(vec![ok(TOKEN_JSON)]);
        let Run {
            result,
            out,
            store,
            enable_calls,
            _dir,
        } = run(&fake, false).await;

        let saved = result.expect("login succeeds");
        assert!(saved.enabled, "a saved login enables the provider");
        assert_eq!(enable_calls, 1);
        assert_eq!(
            out,
            "https://github.com/login/device\nABCD-1234\nConnected\n"
        );
        assert!(!out.contains("gho_example_token"));

        let data = store.load_provider(ProviderId::Copilot).expect("load");
        assert_eq!(data.accounts[0].token, "gho_example_token");
    }

    #[tokio::test]
    async fn failed_enable_reports_that_the_account_was_saved() {
        let fake = fake(vec![ok(TOKEN_JSON)]);
        let Run {
            result,
            store,
            enable_calls,
            _dir,
            ..
        } = run_with_options(&fake, false, std::future::pending(), true).await;

        let error = result.expect_err("enable failure fails the login");
        assert_eq!(enable_calls, 1);
        assert_eq!(error.exit_code(), 1);
        assert_eq!(
            error.to_string(),
            "Signed in and saved the account, but could not enable Copilot: settings write failed"
        );
        let data = store.load_provider(ProviderId::Copilot).expect("load");
        assert_eq!(data.accounts.len(), 1, "the account stays saved");
    }

    #[tokio::test]
    async fn json_output_is_upstream_shaped_events_without_the_token() {
        let fake = fake(vec![
            ok(r#"{"error":"authorization_pending"}"#),
            ok(TOKEN_JSON),
        ]);
        let Run {
            result,
            out,
            enable_calls,
            _dir,
            ..
        } = run(&fake, true).await;

        result.expect("login succeeds");
        assert_eq!(enable_calls, 1);
        assert!(!out.contains("gho_example_token"));
        let events: Vec<serde_json::Value> = out
            .lines()
            .map(|line| serde_json::from_str(line).expect("one JSON object per line"))
            .collect();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["status"], "authorization_pending");
        assert_eq!(
            events[0]["verificationURI"],
            "https://github.com/login/device"
        );
        assert_eq!(events[0]["userCode"], "ABCD-1234");
        assert_eq!(events[0]["expiresIn"], 900);
        assert!(events[0].get("error").is_none());
        assert_eq!(events[1], serde_json::json!({ "status": "authorized" }));
    }

    #[tokio::test]
    async fn denied_json_reports_a_failed_event_and_returns_the_error() {
        let fake = fake(vec![ok(r#"{"error":"access_denied"}"#)]);
        let Run {
            result,
            out,
            store,
            enable_calls,
            _dir,
        } = run(&fake, true).await;

        let error = result.expect_err("denied");
        assert_eq!(error.exit_code(), 5);
        assert_eq!(
            enable_calls, 0,
            "a denied login does not enable the provider"
        );
        assert_eq!(
            error.to_string(),
            "GitHub device login failed: Access denied by user"
        );
        let failed: serde_json::Value =
            serde_json::from_str(out.lines().last().expect("event")).expect("failed event is JSON");
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["reason"], "denied");
        assert_eq!(
            failed["error"],
            "GitHub device login failed: Access denied by user"
        );
        assert!(!out.contains("gho_example_token"));
        assert!(
            store
                .load_provider(ProviderId::Copilot)
                .expect("load")
                .accounts
                .is_empty()
        );
    }

    #[tokio::test]
    async fn expired_text_login_prints_the_code_and_returns_an_error() {
        let fake = fake(vec![ok(r#"{"error":"expired_token"}"#)]);
        let Run {
            result,
            out,
            enable_calls,
            _dir,
            ..
        } = run(&fake, false).await;

        let error = result.expect_err("expired");
        assert_eq!(error.exit_code(), 6);
        assert_eq!(enable_calls, 0);
        assert_eq!(
            error.to_string(),
            "GitHub device login failed: Device code expired"
        );
        assert_eq!(out, "https://github.com/login/device\nABCD-1234\n");
    }

    #[tokio::test]
    async fn cancelled_before_any_request_saves_nothing_and_reports_cancelled() {
        let fake = fake(vec![ok(TOKEN_JSON)]);
        let Run {
            result,
            out,
            store,
            enable_calls,
            _dir,
        } = run_with_cancel(&fake, true, std::future::ready(())).await;

        let error = result.expect_err("cancelled");
        assert_eq!(error.exit_code(), 7);
        assert_eq!(error.to_string(), "Login cancelled.");
        assert_eq!(out, "{\"status\":\"cancelled\"}\n");
        assert_eq!(enable_calls, 0);
        assert!(fake.requests().is_empty(), "no GitHub request after Ctrl-C");
        assert!(
            store
                .load_provider(ProviderId::Copilot)
                .expect("load")
                .accounts
                .is_empty()
        );
    }

    #[tokio::test]
    async fn network_failure_is_a_general_failure_with_an_error_reason() {
        let fake = FakeGitHub::start(vec![(
            "/login/device/code",
            vec![Reply {
                status: 500,
                body: "{}",
            }],
        )]);
        let Run {
            result,
            out,
            store,
            _dir,
            ..
        } = run(&fake, true).await;

        assert_eq!(result.expect_err("server error").exit_code(), 1);
        let failed: serde_json::Value =
            serde_json::from_str(out.lines().last().expect("event")).expect("failed event is JSON");
        assert_eq!(failed["status"], "failed");
        assert_eq!(failed["reason"], "error");
        assert!(
            store
                .load_provider(ProviderId::Copilot)
                .expect("load")
                .accounts
                .is_empty()
        );
    }
}

#[test]
fn set_cookie_and_clear_credential_parse_upstream_flags() {
    let ConfigCommand::SetCookie {
        target,
        cookie,
        stdin,
        no_enable,
        output,
    } = parse_config(&[
        "set-cookie",
        "--provider",
        "cursor",
        "--cookie",
        "a=b",
        "--no-enable",
        "--json",
    ])
    .unwrap()
    else {
        panic!("expected set-cookie");
    };
    assert_eq!(target.name(), "cursor");
    assert_eq!(cookie.as_deref(), Some("a=b"));
    assert!(!stdin);
    assert!(no_enable);
    assert!(output.is_json());

    let ConfigCommand::ClearCredential {
        target,
        api_key,
        cookie,
        ..
    } = parse_config(&["clear-credential", "-p", "openrouter", "--api-key"]).unwrap()
    else {
        panic!("expected clear-credential");
    };
    assert_eq!(target.name(), "openrouter");
    assert!(api_key);
    assert!(!cookie);
}

#[test]
fn set_cookie_rejects_providers_without_a_cookie_lane() {
    assert_eq!(
        config_cookie_provider("cursor").unwrap(),
        ProviderId::Cursor
    );
    let error = config_cookie_provider("openrouter")
        .unwrap_err()
        .to_string();
    assert_eq!(error, "openrouter does not support config cookies.");
    assert!(config_cookie_provider("not-a-provider").is_err());
}

#[test]
fn set_cookie_trims_and_rejects_empty_or_multiline_headers() {
    assert_eq!(
        clean_cookie_header(Some("  session=abc; other=1 \n")).unwrap(),
        "session=abc; other=1"
    );
    assert!(clean_cookie_header(None).is_err());
    assert!(clean_cookie_header(Some("   ")).is_err());
    let error = clean_cookie_header(Some("a=1\nb=2"))
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Cookie header must be a single line.");
    let error = clean_cookie_header(Some("a=1\rb=2"))
        .unwrap_err()
        .to_string();
    assert_eq!(error, "Cookie header must be a single line.");
}

#[test]
fn credential_store_load_stops_on_an_unreadable_file_without_quoting_it() {
    let dir = tempfile::tempdir().expect("create temporary directory");
    let path = dir.path().join("manual_cookies.json");

    let missing: ManualCookies =
        load_credential_store(Some(path.clone()), "manual_cookies.json").unwrap();
    assert!(missing.cookies.is_empty());

    std::fs::write(&path, r#"{"cookies":{"cursor":"session=secret-value"}}"#).unwrap();
    let error = load_credential_store::<ManualCookies>(Some(path.clone()), "manual_cookies.json")
        .unwrap_err()
        .to_string();
    assert_eq!(
        error,
        "manual_cookies.json could not be parsed; nothing was changed."
    );
    assert!(!error.contains("secret-value"));

    crate::secure_file::write_string(
        &path,
        r#"{"cookies":{"cursor":{"cookie_header":"a=1","saved_at":"2026-01-01 00:00"}}}"#,
    )
    .unwrap();
    let loaded: ManualCookies = load_credential_store(Some(path), "manual_cookies.json").unwrap();
    assert_eq!(loaded.get("cursor"), Some("a=1"));
}

#[test]
fn set_cookie_selects_manual_and_enables_unless_told_not_to() {
    let mut settings = Settings::default();
    settings.set_cookie_source(ProviderId::Cursor, "auto");
    settings.disable_provider(ProviderId::Cursor);
    let mut cookies = ManualCookies::default();
    apply_set_cookie(
        &mut settings,
        &mut cookies,
        ProviderId::Cursor,
        "session=abc",
        false,
    );
    assert_eq!(cookies.get("cursor"), Some("session=abc"));
    assert_eq!(settings.cookie_source(ProviderId::Cursor), "manual");
    assert!(!settings.is_provider_enabled(ProviderId::Cursor));

    apply_set_cookie(
        &mut settings,
        &mut cookies,
        ProviderId::Cursor,
        "session=new",
        true,
    );
    assert_eq!(cookies.get("cursor"), Some("session=new"));
    assert!(settings.is_provider_enabled(ProviderId::Cursor));
}

#[test]
fn clear_credential_defaults_to_both_fields_and_flags_narrow_it() {
    assert_eq!(
        credential_fields(false, false),
        vec![CredentialField::ApiKey, CredentialField::CookieHeader]
    );
    assert_eq!(
        credential_fields(true, false),
        vec![CredentialField::ApiKey]
    );
    assert_eq!(
        credential_fields(false, true),
        vec![CredentialField::CookieHeader]
    );
    assert_eq!(
        credential_fields(true, true),
        vec![CredentialField::ApiKey, CredentialField::CookieHeader]
    );
}

#[test]
fn clear_credential_removes_only_stored_requested_fields_for_that_provider() {
    let mut keys = crate::settings::ApiKeys::default();
    keys.set("cursor", "key-cursor", None);
    keys.set("openrouter", "key-openrouter", None);
    let mut cookies = ManualCookies::default();
    cookies.set("cursor", "session=cursor");
    cookies.set("codex", "session=codex");

    let cleared = clear_stored_credentials(
        &mut keys,
        &mut cookies,
        ProviderId::Cursor,
        &[CredentialField::CookieHeader],
    );
    assert_eq!(cleared, vec![CredentialField::CookieHeader]);
    assert_eq!(cookies.get("cursor"), None);
    assert_eq!(keys.get("cursor"), Some("key-cursor"));
    assert_eq!(cookies.get("codex"), Some("session=codex"));

    let cleared = clear_stored_credentials(
        &mut keys,
        &mut cookies,
        ProviderId::Cursor,
        &CredentialField::ALL,
    );
    assert_eq!(cleared, vec![CredentialField::ApiKey]);
    assert_eq!(keys.get("cursor"), None);
    assert_eq!(keys.get("openrouter"), Some("key-openrouter"));

    let cleared = clear_stored_credentials(
        &mut keys,
        &mut cookies,
        ProviderId::Cursor,
        &CredentialField::ALL,
    );
    assert!(cleared.is_empty());
}

#[test]
fn credential_results_serialize_with_upstream_field_names_and_no_secrets() {
    let value = serde_json::to_value(super::ConfigClearCredentialResult {
        provider: "cursor".into(),
        cleared: vec![CredentialField::ApiKey, CredentialField::CookieHeader],
        config_path: Some("C:/cfg/settings.json".into()),
    })
    .unwrap();
    assert_eq!(
        value,
        json!({
            "provider": "cursor",
            "cleared": ["apiKey", "cookieHeader"],
            "configPath": "C:/cfg/settings.json"
        })
    );
    let value = serde_json::to_value(super::ConfigSetCookieResult {
        provider: "cursor".into(),
        enabled: true,
        config_path: None,
    })
    .unwrap();
    assert_eq!(
        value,
        json!({"provider": "cursor", "enabled": true, "configPath": null})
    );
}
