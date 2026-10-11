//! `provider_configs` migration, canonicalization and per-provider accessors.

use super::*;

// ── Phase 3: provider_configs migration tests ───────────────────────

/// Loading a legacy `settings.json` (with flat per-provider fields)
/// must populate `provider_configs` and surface every value through the
/// per-provider accessors.
#[test]
fn test_legacy_per_provider_fields_migrate_into_provider_configs() {
    // NOTE: placeholder values only — no real cookies/tokens.
    let legacy_json = r#"{
            "enabled_providers": ["claude", "codex"],
            "refresh_interval_secs": 300,
            "codex_cookie_source": "manual",
            "claude_cookie_source": "browser",
            "cursor_cookie_source": "manual",
            "alibaba_cookie_source": "manual",
            "alibaba_cookie_header": "ali=PLACEHOLDER",
            "alibaba_api_region": "cn",
            "zai_api_region": "cn",
            "minimax_api_region": "cn",
            "minimax_api_token": "TOK_PLACEHOLDER",
            "claude_usage_source": "ccusage",
            "codex_usage_source": "manual",
            "codex_openai_web_extras": false,
            "codex_historical_tracking": true,
            "claude_avoid_keychain_prompts": true,
            "opencode_workspace_id": "ws_placeholder",
            "jetbrains_ide_base_path": "C:/JB"
        }"#;

    let settings: Settings = serde_json::from_str(legacy_json).unwrap();

    // Cookie sources
    assert_eq!(settings.cookie_source(ProviderId::Codex), "manual");
    assert_eq!(settings.cookie_source(ProviderId::Claude), "browser");
    assert_eq!(settings.cookie_source(ProviderId::Cursor), "manual");
    assert_eq!(settings.cookie_source(ProviderId::Alibaba), "manual");
    // Untouched providers fall through to the default "manual" to avoid
    // background browser-cookie reads unless the user opts into Automatic.
    assert_eq!(settings.cookie_source(ProviderId::Amp), "manual");

    // Manual cookie headers + api regions
    assert_eq!(
        settings.manual_cookie_header(ProviderId::Alibaba),
        "ali=PLACEHOLDER"
    );
    assert_eq!(settings.api_region(ProviderId::Alibaba), "cn");
    assert_eq!(settings.api_region(ProviderId::Zai), "cn");
    assert_eq!(settings.api_region(ProviderId::MiniMax), "cn");

    // Usage sources
    assert_eq!(settings.usage_source(ProviderId::Claude), "ccusage");
    assert_eq!(settings.usage_source(ProviderId::Codex), "manual");

    // Codex booleans
    assert!(!settings.openai_web_extras(ProviderId::Codex));
    assert!(settings.historical_tracking(ProviderId::Codex));

    // Claude per-provider boolean
    assert!(settings.avoid_keychain_prompts(ProviderId::Claude));

    // Misc per-provider strings
    assert_eq!(
        settings.workspace_id(ProviderId::OpenCode),
        "ws_placeholder"
    );
    assert_eq!(settings.api_token(ProviderId::MiniMax), "TOK_PLACEHOLDER");
    assert_eq!(settings.ide_base_path(ProviderId::JetBrains), "C:/JB");

    // Legacy field-name aliases agree with typed accessors.
    assert_eq!(settings.codex_cookie_source(), "manual");
    assert_eq!(settings.alibaba_api_region(), "cn");
    assert!(settings.codex_historical_tracking());
    assert!(!settings.codex_openai_web_extras());
    assert!(settings.claude_avoid_keychain_prompts());
}

/// Round-trip: build a `Settings` programmatically via the new map +
/// accessors, serialize, parse back, and assert equality of every
/// per-provider field.
#[test]
fn test_provider_configs_roundtrip() {
    let mut settings = Settings::default();
    settings.set_cookie_source(ProviderId::Codex, "manual");
    settings.set_cookie_source(ProviderId::Claude, "browser");
    settings.set_usage_source(ProviderId::Claude, "ccusage");
    settings.set_api_region(ProviderId::Alibaba, "cn");
    settings.set_api_region(ProviderId::Zai, "cn");
    settings
        .provider_config_mut(ProviderId::Amp)
        .manual_cookie_header = Some("amp=PLACEHOLDER".into());
    settings.provider_config_mut(ProviderId::MiniMax).api_token = Some("TOK_PLACEHOLDER".into());
    settings.set_workspace_id(ProviderId::OpenCode, "ws_placeholder");
    settings.set_ide_base_path(ProviderId::JetBrains, "C:/JB");
    settings
        .provider_config_mut(ProviderId::Codex)
        .openai_web_extras = Some(false);
    settings
        .provider_config_mut(ProviderId::Codex)
        .historical_tracking = true;
    settings.set_avoid_keychain_prompts(ProviderId::Claude, true);
    settings.set_auto_resume_after_quota_reset(ProviderId::Codex, true);
    settings
        .set_seat_credit_entitlement(ProviderId::Copilot, Some(300.0))
        .expect("valid seat credit entitlement");

    let (json, loaded) = round_trip(&settings);
    // The legacy flat fields must NOT appear in serialized output.
    assert!(!json.contains("\"codex_cookie_source\""), "json: {json}");
    assert!(!json.contains("\"alibaba_api_region\""), "json: {json}");
    assert!(
        !json.contains("\"claude_avoid_keychain_prompts\""),
        "json: {json}"
    );
    assert!(json.contains("\"provider_configs\""), "json: {json}");

    assert_eq!(loaded.cookie_source(ProviderId::Codex), "manual");
    assert_eq!(loaded.cookie_source(ProviderId::Claude), "browser");
    assert_eq!(loaded.usage_source(ProviderId::Claude), "ccusage");
    assert_eq!(loaded.api_region(ProviderId::Alibaba), "cn");
    assert_eq!(loaded.api_region(ProviderId::Zai), "cn");
    assert_eq!(
        loaded.manual_cookie_header(ProviderId::Amp),
        "amp=PLACEHOLDER"
    );
    assert_eq!(loaded.api_token(ProviderId::MiniMax), "TOK_PLACEHOLDER");
    assert_eq!(loaded.workspace_id(ProviderId::OpenCode), "ws_placeholder");
    assert_eq!(loaded.ide_base_path(ProviderId::JetBrains), "C:/JB");
    assert!(!loaded.openai_web_extras(ProviderId::Codex));
    assert!(loaded.historical_tracking(ProviderId::Codex));
    assert!(loaded.avoid_keychain_prompts(ProviderId::Claude));
    assert!(loaded.auto_resume_after_quota_reset(ProviderId::Codex));
    assert_eq!(
        loaded.seat_credit_entitlement(ProviderId::Copilot),
        Some(300.0)
    );
    assert_eq!(
        loaded.provider_configs.get(&ProviderId::Codex),
        settings.provider_configs.get(&ProviderId::Codex)
    );
}

/// New-format files (no legacy flat fields, only `provider_configs`)
/// must load identically.
#[test]
fn test_new_format_provider_configs_only() {
    let json = r#"{
            "enabled_providers": ["claude"],
            "refresh_interval_secs": 300,
            "provider_configs": {
                "codex": { "cookie_source": "manual", "openai_web_extras": false },
                "alibaba": { "api_region": "cn", "manual_cookie_header": "ali=PLACEHOLDER" }
            }
        }"#;

    let settings: Settings = serde_json::from_str(json).unwrap();
    assert_eq!(settings.cookie_source(ProviderId::Codex), "manual");
    assert!(!settings.openai_web_extras(ProviderId::Codex));
    assert_eq!(settings.api_region(ProviderId::Alibaba), "cn");
    assert_eq!(
        settings.manual_cookie_header(ProviderId::Alibaba),
        "ali=PLACEHOLDER"
    );
    // Untouched providers still get their defaults.
    assert_eq!(settings.cookie_source(ProviderId::Claude), "manual");
    assert_eq!(settings.api_region(ProviderId::Zai), "global");
}

#[test]
fn retired_provider_config_is_ignored_until_explicit_save() {
    let original = r#"{
            "enabled_providers": ["codex", "crof"],
            "refresh_interval_secs": 300,
            "provider_metrics": { "codex": "weekly", "crof": "session" },
            "float_bar_provider_ids": ["codex", "crof"],
            "stacked_tray_top_provider": "crof",
            "stacked_tray_bottom_provider": "crof",
            "provider_configs": {
                "crof": { "api_token": "retired-fixture-key" },
                "codex": { "cookie_source": "manual", "openai_web_extras": false },
                "alibaba": { "api_region": "cn", "manual_cookie_header": "ali=PLACEHOLDER" }
            }
        }"#;
    let original_bytes = original.as_bytes().to_vec();

    let settings: Settings =
        serde_json::from_str(original).expect("load settings with retired key");

    assert_eq!(original.as_bytes(), original_bytes);
    assert_eq!(settings.cookie_source(ProviderId::Codex), "manual");
    assert!(!settings.openai_web_extras(ProviderId::Codex));
    assert_eq!(
        settings.enabled_providers,
        HashSet::from(["codex".to_string()])
    );
    assert_eq!(settings.provider_metrics.len(), 1);
    assert_eq!(settings.float_bar_provider_ids, ["codex"]);
    assert_eq!(settings.stacked_tray_top_provider, None);
    assert_eq!(settings.stacked_tray_bottom_provider, None);
    assert_eq!(settings.api_region(ProviderId::Alibaba), "cn");
    assert_eq!(
        settings.manual_cookie_header(ProviderId::Alibaba),
        "ali=PLACEHOLDER"
    );

    let saved = serde_json::to_string(&settings).expect("serialize sanitized settings");
    let saved_value: serde_json::Value = serde_json::from_str(&saved).unwrap();
    let saved_configs = saved_value["provider_configs"].as_object().unwrap();
    assert!(!saved_configs.contains_key("crof"));
    assert!(saved_configs.contains_key("codex"));
    assert!(saved_configs.contains_key("alibaba"));
    assert!(
        !saved.contains("\"crof\""),
        "saved settings retained Crof: {saved}"
    );
}

#[test]
fn provider_aliases_are_canonicalized_at_the_load_boundary() {
    let settings: Settings = serde_json::from_str(
        r#"{
            "enabled_providers": ["openai", "ClAuDe", "not-a-provider"],
            "provider_metrics": {
                "openai": "weekly",
                "CoDeX": "session",
                "not-a-provider": "weekly"
            },
            "float_bar_provider_ids": ["OPENAI", "codex", "ClAuDe", "unknown"],
            "stacked_tray_top_provider": "OPENAI",
            "stacked_tray_bottom_provider": "ClAuDe"
        }"#,
    )
    .expect("load settings containing provider aliases");

    assert_eq!(
        settings.enabled_providers,
        HashSet::from(["claude".to_string(), "codex".to_string()])
    );
    assert_eq!(
        settings.provider_metrics.get("codex"),
        Some(&MetricPreference::Session)
    );
    assert_eq!(settings.provider_metrics.len(), 1);
    assert_eq!(settings.float_bar_provider_ids, ["codex", "claude"]);
    assert_eq!(settings.stacked_tray_top_provider.as_deref(), Some("codex"));
    assert_eq!(
        settings.stacked_tray_bottom_provider.as_deref(),
        Some("claude")
    );
}

#[test]
fn stacked_preferences_preserve_known_disabled_providers() {
    let settings: Settings = serde_json::from_str(
        r#"{
            "enabled_providers": ["claude"],
            "stacked_tray_top_provider": "OPENAI",
            "stacked_tray_bottom_provider": "not-a-provider"
        }"#,
    )
    .expect("load stacked preferences independently of enablement");

    assert_eq!(settings.stacked_tray_top_provider.as_deref(), Some("codex"));
    assert_eq!(settings.stacked_tray_bottom_provider, None);
    assert_eq!(
        settings.enabled_providers,
        HashSet::from(["claude".to_string()])
    );
}

/// Default `Settings` should serialize WITHOUT a `provider_configs`
/// field (empty map skipped).
#[test]
fn test_default_settings_skip_empty_provider_configs() {
    let settings = Settings::default();
    let json = serde_json::to_string(&settings).unwrap();
    assert!(
        !json.contains("\"provider_configs\""),
        "empty map should be skipped, json: {json}"
    );
}

/// Per-provider defaults are applied even when the entry is absent.
#[test]
fn test_per_provider_defaults_applied() {
    let settings = Settings::default();
    assert_eq!(settings.cookie_source(ProviderId::Codex), "manual");
    assert_eq!(settings.usage_source(ProviderId::Codex), "auto");
    assert_eq!(settings.api_region(ProviderId::Alibaba), "singapore");
    assert_eq!(settings.api_region(ProviderId::Zai), "global");
    assert_eq!(settings.api_region(ProviderId::MiniMax), "global");
    assert!(settings.openai_web_extras(ProviderId::Codex));
    assert!(!settings.historical_tracking(ProviderId::Codex));
    assert!(!settings.avoid_keychain_prompts(ProviderId::Claude));
    assert!(!settings.auto_resume_after_quota_reset(ProviderId::Codex));
    assert!(!settings.auto_resume_after_quota_reset(ProviderId::Claude));
}

#[test]
fn codex_spark_usage_visibility_defaults_to_visible_and_roundtrips() {
    let mut settings = Settings::default();
    assert!(settings.codex_spark_usage_visible());

    settings.set_codex_spark_usage_visible(false);
    let serialized = serde_json::to_string(&settings).unwrap();
    let loaded: Settings = serde_json::from_str(&serialized).unwrap();

    assert!(!loaded.codex_spark_usage_visible());
}

#[test]
fn migrate_legacy_visibility_flags_materializes_hidden_usage_item_ids() {
    let mut settings = Settings::default();
    settings.set_spark_usage_visible(ProviderId::Codex, false);
    settings.claude_daily_routines_usage_visible = false;

    settings.migrate_legacy_usage_item_flags();

    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Codex),
        CODEX_SPARK_USAGE_ITEM_IDS
            .iter()
            .map(|id| (*id).to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Claude),
        vec![CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID.to_string()]
    );
}

#[test]
fn generic_claude_visibility_writes_only_the_usage_item_list() {
    let mut settings = Settings::default();

    settings.set_hidden_usage_item_ids(
        ProviderId::Claude,
        vec![CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID.to_string()],
    );

    assert!(settings.claude_daily_routines_usage_visible);
    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Claude),
        vec![CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID.to_string()]
    );

    settings.set_hidden_usage_item_ids(ProviderId::Claude, Vec::new());

    assert!(settings.claude_daily_routines_usage_visible);
    assert!(
        settings
            .hidden_usage_item_ids(ProviderId::Claude)
            .is_empty()
    );
}

#[test]
fn explicit_hidden_usage_item_ids_roundtrip_and_restore_defaults() {
    let mut settings = Settings::default();
    settings.set_hidden_usage_item_ids(
        ProviderId::Codex,
        vec![
            "metric:secondary".to_string(),
            "metric:secondary".to_string(),
            "not-a-metric".to_string(),
        ],
    );

    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Codex),
        vec!["metric:secondary".to_string()]
    );
    assert!(settings.codex_spark_usage_visible());

    let serialized = serde_json::to_string(&settings).unwrap();
    let loaded: Settings = serde_json::from_str(&serialized).unwrap();
    assert_eq!(
        loaded.hidden_usage_item_ids(ProviderId::Codex),
        vec!["metric:secondary".to_string()]
    );

    settings.set_hidden_usage_item_ids(ProviderId::Codex, Vec::new());
    settings.set_hidden_usage_item_ids(ProviderId::Claude, Vec::new());
    assert!(settings.hidden_usage_item_ids(ProviderId::Codex).is_empty());
    assert!(settings.codex_spark_usage_visible());
}

#[test]
fn legacy_visibility_setters_preserve_other_explicit_hidden_items() {
    let mut settings = Settings::default();
    settings.set_hidden_usage_item_ids(ProviderId::Claude, vec!["metric:secondary".to_string()]);

    settings.toggle_hidden_items(
        ProviderId::Claude,
        &[CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID],
        false,
    );
    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Claude),
        vec![
            "metric:extra-claude-routines".to_string(),
            "metric:secondary".to_string(),
        ]
    );

    settings.toggle_hidden_items(
        ProviderId::Claude,
        &[CLAUDE_DAILY_ROUTINES_USAGE_ITEM_ID],
        true,
    );
    assert_eq!(
        settings.hidden_usage_item_ids(ProviderId::Claude),
        vec!["metric:secondary".to_string()]
    );
}

/// Cookie-denial settings must survive the same path-based persistence used by
/// `Settings::save` and `Settings::load`, including the secure-file wrapper
/// shared by desktop and CLI settings.
#[test]
fn cookie_denial_round_trips_through_settings_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let mut settings = Settings::default();
    settings.set_cookie_source(ProviderId::Codex, "off");
    settings
        .provider_config_mut(ProviderId::Codex)
        .openai_web_extras = Some(false);

    settings.save_to_path(&path).unwrap();
    let loaded = Settings::load_from_path(Some(&path));

    assert_eq!(loaded.cookie_source(ProviderId::Codex), "off");
    assert!(!loaded.openai_web_extras(ProviderId::Codex));
}
