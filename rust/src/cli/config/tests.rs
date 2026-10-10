
use super::{
    ClaudeCodeCredentialsConsent, ConfigCommand, ConfigFileError, ConfigOutputArgs, ConsentAction,
    apply_consent_action, consent_text, provider_statuses, read_json_config,
    sanitize_settings_for_dump,
};
use crate::cli::{Cli, Commands};
use crate::core::ProviderId;
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
