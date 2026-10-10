use super::{
    NamedRateWindowSnapshot, ProviderSummary, ProviderUsageSnapshot, provider_cookie_source_lookup,
    provider_region_lookup, validate_external_url, validate_surface_target,
};
use crate::state::AppState;
use crate::surface::SurfaceMode;
use crate::surface_target::SurfaceTarget;
use codexbar::core::{
    FetchContext, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderInventoryItem, instantiate_provider,
};
use codexbar::host::session::launch_block_reason;
use codexbar::settings::{Language, Settings};

#[test]
fn validate_surface_target_accepts_matching_target() {
    let target = validate_surface_target(
        SurfaceMode::Settings,
        SurfaceTarget::Settings {
            tab: "apiKeys".into(),
        },
    )
    .unwrap();

    assert_eq!(
        target,
        SurfaceTarget::Settings {
            tab: "apiKeys".into()
        }
    );
}

#[test]
fn validate_surface_target_rejects_mismatched_target() {
    let error = validate_surface_target(
        SurfaceMode::TrayPanel,
        SurfaceTarget::Settings {
            tab: "apiKeys".into(),
        },
    )
    .unwrap_err();

    assert!(error.contains("not valid for mode 'trayPanel'"));
}

#[test]
fn validate_surface_target_rejects_hidden_mode() {
    let error = validate_surface_target(SurfaceMode::Hidden, SurfaceTarget::Summary).unwrap_err();

    assert!(error.contains("only supports visible surfaces"));
}

#[test]
fn validate_surface_target_rejects_retired_popout_mode() {
    for target in [
        SurfaceTarget::Dashboard,
        SurfaceTarget::Provider {
            provider_id: "codex".into(),
        },
    ] {
        let error = validate_surface_target(SurfaceMode::PopOut, target).unwrap_err();
        assert!(error.contains("popOut surface is retired"));
    }
}

#[test]
fn external_url_validation_allows_only_http_urls() {
    assert_eq!(
        validate_external_url(" https://github.com/Finesssee/Win-CodexBar ").unwrap(),
        "https://github.com/Finesssee/Win-CodexBar"
    );
    assert_eq!(
        validate_external_url("http://codexbar.app").unwrap(),
        "http://codexbar.app"
    );

    for invalid in [
        "",
        "file:///etc/passwd",
        "javascript:alert(1)",
        "https://bad\nhost",
    ] {
        assert!(
            validate_external_url(invalid).is_err(),
            "accepted invalid URL: {invalid:?}"
        );
    }
}

#[test]
fn credential_status_labels_do_not_include_error_details() {
    assert_eq!(
        super::credential_file_status_label(codexbar::secure_file::SecureFileStatus::Missing),
        "missing"
    );
    assert_eq!(
        super::credential_file_status_label(codexbar::secure_file::SecureFileStatus::Plaintext),
        "plaintext"
    );
    assert_eq!(
        super::credential_file_status_label(codexbar::secure_file::SecureFileStatus::Protected(
            "windows-dpapi-user".to_string(),
        )),
        "protected:windows-dpapi-user"
    );
    assert_eq!(
        super::credential_file_status_label(codexbar::secure_file::SecureFileStatus::Unreadable(
            "secret path / token".to_string(),
        )),
        "unreadable"
    );
}

#[test]
fn command_inputs_reject_invalid_provider_ids_before_storage_writes() {
    assert!(super::set_api_key("not-a-provider".into(), "sk-test".into(), None).is_err());
    assert!(super::set_manual_cookie("not-a-provider".into(), "a=b".into()).is_err());
    assert!(super::remove_api_key("bad\nprovider".into()).is_err());
    assert!(super::remove_manual_cookie("".into()).is_err());
}

#[test]
fn command_inputs_reject_multiline_secrets() {
    assert!(super::set_api_key("openrouter".into(), "sk-test\nnext".into(), None).is_err());
    assert!(super::set_manual_cookie("codex".into(), "a=b\nc=d".into()).is_err());
}

#[test]
fn command_inputs_reject_unknown_cookie_source_and_region_values() {
    assert!(super::set_provider_cookie_source("codex".into(), "browser".into()).is_err());
    assert!(super::set_provider_region("zai".into(), "moon".into()).is_err());
}

#[test]
fn apply_provider_order_dedupes_and_appends_unknown_canonical() {
    // Request only "codex" and "claude" — remaining canonical ids should
    // be appended after, preserving canonical order.
    let order =
        codexbar::settings::normalize_provider_order(&["codex".to_string(), "claude".to_string()]);
    assert_eq!(order[0], "codex");
    assert_eq!(order[1], "claude");
    // Every canonical id appears exactly once.
    let mut sorted = order.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), order.len());
    // Every canonical id is present.
    let canonical = codexbar::core::ProviderId::all()
        .iter()
        .map(|p| p.cli_name().to_string())
        .collect::<Vec<_>>();
    for id in &canonical {
        assert!(order.contains(id), "missing canonical id: {id}");
    }
}

#[test]
fn apply_provider_order_ignores_unknown_ids() {
    let order = codexbar::settings::normalize_provider_order(&[
        "not-a-provider".to_string(),
        "codex".to_string(),
    ]);
    assert_eq!(order[0], "codex");
    assert!(!order.iter().any(|id| id == "not-a-provider"));
}

#[test]
fn provider_summaries_reflect_settings_order() {
    // Deprecated providers (KimiK2, CrossModel) are soft-removed from the
    // Settings catalog unless already enabled, so the default Settings
    // surface omits them (upstream #2254).
    let canonical_len = codexbar::core::ProviderId::all()
        .iter()
        .filter(|p| !p.is_deprecated())
        .count();
    let s = Settings::default();
    let summaries: Vec<ProviderSummary> = super::build_provider_summaries(&s);
    assert_eq!(summaries.len(), canonical_len);
    // Index is assigned in emission order.
    for (i, s) in summaries.iter().enumerate() {
        assert_eq!(s.order, i as u32);
    }
}

#[test]
fn provider_catalog_preserves_partial_config_order() {
    let settings = Settings {
        provider_order: codexbar::settings::normalize_provider_order(&[
            "gemini".to_string(),
            "claude".to_string(),
            "codex".to_string(),
        ]),
        ..Settings::default()
    };

    let catalog = super::provider_catalog_for(&settings);

    assert_eq!(
        catalog
            .iter()
            .take(3)
            .map(|provider| provider.id.as_str())
            .collect::<Vec<_>>(),
        vec!["gemini", "claude", "codex"]
    );
}

#[test]
fn settings_snapshot_preserves_partial_config_order_for_enabled_providers() {
    let settings = Settings {
        enabled_providers: ["gemini", "claude", "codex"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        provider_order: codexbar::settings::normalize_provider_order(&[
            "gemini".to_string(),
            "claude".to_string(),
            "codex".to_string(),
        ]),
        ..Settings::default()
    };

    let snapshot = serde_json::to_value(super::SettingsSnapshot::from(settings)).unwrap();

    assert_eq!(
        snapshot["providerOrder"]
            .as_array()
            .unwrap()
            .iter()
            .take(3)
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["gemini", "claude", "codex"],
    );
    assert_eq!(
        snapshot["enabledProviders"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["gemini", "claude", "codex"],
    );
    assert_eq!(snapshot["trayPanelAlwaysOnTop"], false);
}

#[test]
fn provider_cookie_source_lookup_roundtrips_known_providers() {
    let mut s = Settings::default();
    super::provider_cookie_source_set(&mut s, "codex", "cli-config".to_string()).unwrap();
    assert_eq!(
        provider_cookie_source_lookup(&s, "codex").as_deref(),
        Some("cli-config")
    );
    assert!(provider_cookie_source_lookup(&s, "unknown-provider").is_none());
}

#[test]
fn provider_region_lookup_roundtrips_known_providers() {
    let mut s = Settings::default();
    super::provider_region_set(&mut s, "alibaba", "china".to_string()).unwrap();
    assert_eq!(
        provider_region_lookup(&s, "alibaba").as_deref(),
        Some("china")
    );
    // Non-regional providers return None.
    assert!(provider_region_lookup(&s, "claude").is_none());
}

#[test]
fn minimax_region_lookup_normalizes_legacy_china_value() {
    let mut s = Settings::default();
    super::provider_region_set(&mut s, "minimax", "china".to_string()).unwrap();
    assert_eq!(provider_region_lookup(&s, "minimax").as_deref(), Some("cn"));
}

#[test]
fn kimi_region_lookup_defaults_to_china_and_roundtrips_international() {
    let mut settings = Settings::default();
    assert_eq!(
        provider_region_lookup(&settings, "kimi").as_deref(),
        Some("china")
    );
    super::provider_region_set(&mut settings, "kimi", "international".to_string()).unwrap();
    assert_eq!(
        provider_region_lookup(&settings, "kimi").as_deref(),
        Some("international")
    );
}

#[test]
fn minimax_cookie_domain_follows_selected_region() {
    let mut s = Settings::default();
    assert_eq!(
        super::provider_cookie_domain(ProviderId::MiniMax, &s),
        Some("platform.minimax.io")
    );

    s.set_api_region(ProviderId::MiniMax, "cn");
    assert_eq!(
        super::provider_cookie_domain(ProviderId::MiniMax, &s),
        Some("platform.minimaxi.com")
    );
}

#[test]
fn replicate_cookie_source_and_domain_are_exposed() {
    let mut settings = Settings::default();
    super::provider_cookie_source_set(&mut settings, "replicate", "manual".to_string()).unwrap();
    assert_eq!(
        provider_cookie_source_lookup(&settings, "replicate").as_deref(),
        Some("manual")
    );
    assert_eq!(
        super::provider_cookie_domain(ProviderId::Replicate, &settings),
        Some("replicate.com")
    );
}

#[test]
fn raycast_cookie_source_and_domain_are_exposed() {
    let mut settings = Settings::default();
    super::provider_cookie_source_set(&mut settings, "raycast", "manual".to_string()).unwrap();
    assert_eq!(
        provider_cookie_source_lookup(&settings, "raycast").as_deref(),
        Some("manual")
    );
    assert_eq!(
        super::provider_cookie_domain(ProviderId::Raycast, &settings),
        Some("www.raycast.com")
    );
}

#[test]
fn provider_cookie_source_set_rejects_unknown_provider() {
    let mut s = Settings::default();
    let err = super::provider_cookie_source_set(&mut s, "nope", "x".into()).unwrap_err();
    assert!(err.contains("nope"));
}

#[test]
fn provider_dashboard_url_uses_selected_regional_console() {
    let mut settings = Settings::default();
    settings.set_api_region(ProviderId::MiniMax, "cn");
    settings.set_api_region(ProviderId::Kimi, "international");

    assert_eq!(
        super::provider_dashboard_url(ProviderId::MiniMax, &settings).as_deref(),
        Some("https://platform.minimaxi.com/user-center/payment/coding-plan?cycle_type=3")
    );
    assert_eq!(
        super::provider_dashboard_url(ProviderId::Kimi, &settings).as_deref(),
        Some("https://www.kimi.ai/code/console")
    );
}

/// Provider metadata is the only source of the provider dashboard link; the
/// API-key catalog URL is the key-management link shown next to the key field.
/// A provider that only has a catalog URL must get a metadata URL instead of
/// silently borrowing the key page.
#[test]
fn api_key_catalog_providers_have_metadata_dashboard_urls() {
    let settings = Settings::default();
    for provider in codexbar::settings::get_api_key_providers() {
        if provider.dashboard_url.is_some() {
            assert!(
                super::provider_dashboard_url(provider.id, &settings).is_some(),
                "{:?} has an API-key page but no metadata dashboard URL",
                provider.id
            );
        }
    }
}

#[test]
fn provider_region_set_rejects_non_regional_provider() {
    let mut s = Settings::default();
    let err = super::provider_region_set(&mut s, "claude", "global".into()).unwrap_err();
    assert!(err.contains("claude"));
}

#[test]
fn launch_block_reason_helper_returns_none_when_not_blocked() {
    assert!(launch_block_reason(false, false).is_none());
}

#[test]
fn launch_block_reason_helper_prefers_ssh() {
    let msg = launch_block_reason(true, true).unwrap();
    assert!(msg.contains("SSH"));
}

// ── Phase 6b — provider detail pane ────────────────────────────

#[test]
fn build_provider_detail_populates_identity_urls() {
    let (detail, _settings, _id) = super::build_provider_detail("claude").expect("known provider");
    assert_eq!(detail.id, "claude");
    assert_eq!(detail.display_name, "Claude");
    // Claude advertises a status page URL in its metadata.
    assert!(detail.status_page_url.is_some());
    // No snapshot yet — empty usage bars and no error.
    assert!(detail.session.is_none());
    assert!(detail.last_error.is_none());
    assert!(!detail.has_snapshot);
}

#[test]
fn build_provider_detail_rejects_unknown_provider() {
    let err = super::build_provider_detail("not-a-provider").unwrap_err();
    assert!(err.contains("not-a-provider"));
}

#[test]
fn provider_detail_roundtrips_through_serde() {
    let (detail, _settings, _id) = super::build_provider_detail("codex").expect("known provider");
    let json = serde_json::to_string(&detail).expect("serialize");
    // camelCase rename survives the round-trip.
    assert!(json.contains("\"displayName\""));
    assert!(json.contains("\"hasSnapshot\""));
    assert!(json.contains("\"statusPageUrl\""));
}

#[test]
fn provider_detail_carries_openai_daily_usage_only_when_present() {
    let (mut detail, _settings, _id) =
        super::build_provider_detail("openaiapi").expect("known provider");
    assert!(detail.open_ai_api_usage.is_none());
    let json = serde_json::to_string(&detail).expect("serialize");
    assert!(!json.contains("openAiApiUsage"));

    detail.open_ai_api_usage = Some(super::OpenAiApiUsageSnapshot {
        history_days: 30,
        project_id: None,
        daily: Vec::new(),
    });
    let json = serde_json::to_string(&detail).expect("serialize");
    assert!(json.contains("\"openAiApiUsage\":{\"historyDays\":30"));
}

#[test]
fn usage_item_descriptors_keep_raw_ids_and_redact_titles() {
    let metadata = instantiate_provider(ProviderId::Codex).metadata().clone();
    let result = ProviderFetchResult {
        usage: codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(10.0)),
        cost: None,
        wayfinder_usage: None,
        open_ai_api_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: Vec::new(),
        source_label: "OAuth".to_string(),
        has_successful_claude_cli_quota: false,
        pace_authoritative: true,
        account_identity: None,
        last_good_owner: None,
    };
    let mut snapshot =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Codex, &metadata, &result, None);
    snapshot.primary_label = Some("Account owner@example.com".to_string());
    snapshot.extra_rate_windows = vec![NamedRateWindowSnapshot {
        id: "credits".to_string(),
        title: "Credits owner@example.com".to_string(),
        window: snapshot.primary.clone(),
        fallback_lane: false,
        icon_fallback: None,
    }];

    let mut settings = Settings {
        hide_personal_info: true,
        ..Settings::default()
    };
    settings.set_hidden_usage_item_ids(ProviderId::Codex, vec!["metric:extra-missing".to_string()]);

    let items = super::usage_item_descriptors(Some(&snapshot), &settings, ProviderId::Codex);

    assert_eq!(items[0].id, "metric:primary");
    assert_eq!(items[0].title, "Account Hidden");
    assert_eq!(items[1].id, "metric:extra-credits");
    assert_eq!(items[1].title, "Credits Hidden");
    assert_eq!(items[2].id, "metric:extra-missing");
    assert!(!items[2].available);
}

#[test]
fn usage_item_descriptors_include_detail_sections() {
    let metadata = instantiate_provider(ProviderId::Codex).metadata().clone();
    let result = ProviderFetchResult {
        usage: codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(10.0)),
        cost: None,
        wayfinder_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: vec![
            codexbar::core::ProviderDisplayDetail::new("d1", "Rate limits", "5")
                .expect("valid detail"),
            codexbar::core::ProviderDisplayDetail::new("d2", "Rate limits", "6")
                .expect("valid detail"),
            codexbar::core::ProviderDisplayDetail::new("d3", "owner@example.com quota", "1")
                .expect("valid detail"),
        ],
        source_label: "OAuth".to_string(),
        has_successful_claude_cli_quota: false,
        pace_authoritative: true,
        account_identity: None,
        open_ai_api_usage: None,
        last_good_owner: None,
    };
    let snapshot =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Codex, &metadata, &result, None);

    let mut settings = Settings {
        hide_personal_info: true,
        ..Settings::default()
    };
    settings.set_hidden_usage_item_ids(
        ProviderId::Codex,
        vec!["detailSection:owner@example.com quota".to_string()],
    );

    let items = super::usage_item_descriptors(Some(&snapshot), &settings, ProviderId::Codex);

    // First three items are the distinct detail sections, redacted titles.
    assert_eq!(items[0].id, "detailSection:Rate limits");
    assert_eq!(items[0].title, "Rate limits");
    assert!(items[0].available);
    // Duplicate title is deduped to one descriptor; the email title is redacted.
    assert!(
        !items[1..]
            .iter()
            .any(|item| item.title.contains("owner@example.com"))
    );
    // The hidden detail section is a placeholder, redacted in the stored ID.
    let hidden = items
        .iter()
        .find(|item| item.id.starts_with("detailSection:") && !item.available)
        .expect("hidden detail placeholder");
    assert!(hidden.id.contains("owner@example.com quota"));
    assert_eq!(hidden.title, "Hidden quota");
}

#[test]
fn pace_stage_serializes_to_snake_case_string() {
    use codexbar::core::PaceStage;
    assert_eq!(
        super::bridge::pace::stage_str(PaceStage::OnTrack),
        "on_track"
    );
    assert_eq!(
        super::bridge::pace::stage_str(PaceStage::SlightlyAhead),
        "slightly_ahead"
    );
    assert_eq!(
        super::bridge::pace::stage_str(PaceStage::FarAhead),
        "far_ahead"
    );
    assert_eq!(
        super::bridge::pace::stage_str(PaceStage::SlightlyBehind),
        "slightly_behind"
    );
    assert_eq!(super::bridge::pace::stage_str(PaceStage::Behind), "behind");
    assert_eq!(
        super::bridge::pace::stage_str(PaceStage::FarBehind),
        "far_behind"
    );
}

#[test]
fn local_opencodego_estimates_keep_quota_windows_but_drop_derived_pace() {
    let now = chrono::Utc::now();
    let usage = codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::with_details(
        12.0,
        Some(300),
        Some(now + chrono::Duration::hours(2)),
        None,
    ))
    .with_secondary(codexbar::core::RateWindow::with_details(
        23.0,
        Some(10080),
        Some(now + chrono::Duration::days(3)),
        None,
    ))
    .with_tertiary(codexbar::core::RateWindow::with_details(
        34.0,
        Some(43200),
        Some(now + chrono::Duration::days(10)),
        None,
    ));
    let result = ProviderFetchResult::new(
        usage,
        codexbar::providers::opencodego::LOCAL_ESTIMATE_SOURCE_LABEL,
    )
    .with_non_authoritative_pace();
    let metadata = instantiate_provider(ProviderId::OpenCodeGo)
        .metadata()
        .clone();
    let snapshot =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::OpenCodeGo, &metadata, &result, None);

    assert_eq!(snapshot.source_label, "local estimate");
    assert_eq!(snapshot.primary.used_percent, 12.0);
    assert_eq!(snapshot.secondary.as_ref().unwrap().used_percent, 23.0);
    assert_eq!(snapshot.tertiary.as_ref().unwrap().used_percent, 34.0);
    assert!(snapshot.primary.resets_at.is_some());
    assert!(snapshot.secondary.as_ref().unwrap().resets_at.is_some());
    assert!(snapshot.tertiary.as_ref().unwrap().resets_at.is_some());
    assert!(snapshot.pace.is_none());
    assert!(
        snapshot
            .secondary
            .as_ref()
            .unwrap()
            .reserve_percent
            .is_none()
    );
}

#[test]
fn provider_inventory_maps_to_the_bridge_without_token_ids() {
    let expiry = chrono::DateTime::<chrono::Utc>::from_timestamp(1_900_000_000, 0).unwrap();
    let result = ProviderFetchResult::new(
        codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(12.0)),
        "web",
    )
    .with_inventory_item(ProviderInventoryItem {
        id: "reset-credits".to_string(),
        title: "Limit Reset Credits".to_string(),
        available_count: 2,
        next_expires_at: Some(expiry),
    })
    .with_display_detail(
        ProviderDisplayDetail::new("credits", "Used this cycle", "12")
            .and_then(|row| row.with_secondary_value("Monthly refill: 100"))
            .and_then(|row| row.with_section_title("Credit usage"))
            .and_then(|row| row.with_progress(12.0, 100.0)),
    );
    let metadata = instantiate_provider(ProviderId::Grok).metadata().clone();
    let snapshot =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Grok, &metadata, &result, None);

    assert_eq!(snapshot.inventory.len(), 1);
    assert_eq!(snapshot.inventory[0].available_count, 2);
    assert_eq!(
        snapshot.inventory[0].next_expires_at.as_deref(),
        Some("2030-03-17T17:46:40+00:00")
    );
    assert_eq!(snapshot.display_details.len(), 1);
    assert_eq!(snapshot.display_details[0].value, "12");
    assert_eq!(
        snapshot.display_details[0].section_title.as_deref(),
        Some("Credit usage")
    );
    let snapshot_json = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(
        snapshot_json["displayDetails"][0]["sectionTitle"],
        "Credit usage"
    );
    assert_eq!(
        snapshot.display_details[0].secondary_value.as_deref(),
        Some("Monthly refill: 100")
    );
    let serialized = serde_json::to_string(&snapshot).unwrap();
    assert!(serialized.contains("reset-credits"));
    assert!(!serialized.contains("coupon-token-secret"));
}

#[test]
fn provider_cache_is_fresh_inside_stale_window() {
    assert!(super::is_provider_cache_fresh(
        Some(std::time::Instant::now()),
        std::time::Duration::from_secs(30),
    ));
}

#[test]
fn provider_cache_is_stale_when_missing_timestamp() {
    assert!(!super::is_provider_cache_fresh(
        None,
        std::time::Duration::from_secs(30),
    ));
}

#[test]
fn provider_cache_is_stale_after_window() {
    assert!(!super::is_provider_cache_fresh(
        Some(std::time::Instant::now() - std::time::Duration::from_secs(31)),
        std::time::Duration::from_secs(30),
    ));
}

#[test]
fn provider_fetch_timeout_allows_slower_authenticated_providers() {
    let ctx = FetchContext {
        web_timeout: 30,
        ..FetchContext::default()
    };
    assert_eq!(
        super::provider_fetch_timeout(ProviderId::Claude, &ctx),
        std::time::Duration::from_secs(75)
    );
    assert_eq!(
        super::provider_fetch_timeout(ProviderId::Codex, &ctx),
        std::time::Duration::from_secs(75)
    );
    assert_eq!(
        super::provider_fetch_timeout(ProviderId::Copilot, &ctx),
        std::time::Duration::from_secs(75)
    );
    assert_eq!(
        super::provider_fetch_timeout(ProviderId::DeepSeek, &ctx),
        std::time::Duration::from_secs(35)
    );

    let optional_litellm_ctx = FetchContext {
        web_timeout: 30,
        optional_details_enabled: true,
        ..FetchContext::default()
    };
    assert_eq!(
        super::provider_fetch_timeout(ProviderId::LiteLLM, &optional_litellm_ctx),
        std::time::Duration::from_secs(40)
    );
}

#[test]
fn provider_fetch_timeout_respects_context_web_timeout_with_cap() {
    let ctx = FetchContext {
        web_timeout: 60,
        ..FetchContext::default()
    };
    assert_eq!(
        super::provider_fetch_timeout(ProviderId::T3Chat, &ctx),
        std::time::Duration::from_secs(65)
    );

    let ctx = FetchContext {
        web_timeout: 120,
        ..FetchContext::default()
    };
    assert_eq!(
        super::provider_fetch_timeout(ProviderId::AzureOpenAI, &ctx),
        std::time::Duration::from_secs(65)
    );
}

#[test]
fn provider_cache_upsert_replaces_existing_provider() {
    let metadata = instantiate_provider(ProviderId::Codex).metadata().clone();
    let result = ProviderFetchResult {
        usage: codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(10.0)),
        cost: None,
        wayfinder_usage: None,
        open_ai_api_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: Vec::new(),
        source_label: "CLI".to_string(),
        has_successful_claude_cli_quota: false,
        pace_authoritative: true,
        account_identity: None,
        last_good_owner: None,
    };
    let mut first =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Codex, &metadata, &result, None);
    let mut second = first.clone();
    first.error = Some("old".to_string());
    second.error = Some("new".to_string());

    let mut cache = vec![first];
    super::upsert_provider_cache(&mut cache, second);

    assert_eq!(cache.len(), 1);
    assert_eq!(cache[0].provider_id, "codex");
    assert_eq!(cache[0].error.as_deref(), Some("new"));
}

#[test]
fn provider_cache_prunes_disabled_providers() {
    let metadata = instantiate_provider(ProviderId::Codex).metadata().clone();
    let result = ProviderFetchResult {
        usage: codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(10.0)),
        cost: None,
        wayfinder_usage: None,
        open_ai_api_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: Vec::new(),
        source_label: "CLI".to_string(),
        has_successful_claude_cli_quota: false,
        pace_authoritative: true,
        account_identity: None,
        last_good_owner: None,
    };
    let codex =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Codex, &metadata, &result, None);
    let claude_meta = instantiate_provider(ProviderId::Claude).metadata().clone();
    let claude =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &claude_meta, &result, None);

    let mut cache = vec![codex, claude];
    super::prune_provider_cache_to_enabled(&mut cache, &[ProviderId::Codex]);

    assert_eq!(cache.len(), 1);
    assert_eq!(cache[0].provider_id, "codex");
}

#[test]
fn superseded_refresh_generation_is_not_current() {
    let mut state = AppState::new();
    state.provider_refresh_generation = 3;
    assert!(super::is_current_provider_refresh_generation(&state, 3));
    assert!(!super::is_current_provider_refresh_generation(&state, 2));
}

#[test]

fn claude_transient_auth_failure_preserves_first_last_good_snapshot() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let result = ProviderFetchResult {
        usage: codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(42.0)),
        cost: None,
        wayfinder_usage: None,
        open_ai_api_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: Vec::new(),
        source_label: "OAuth".to_string(),
        has_successful_claude_cli_quota: false,
        pace_authoritative: true,
        account_identity: None,
        last_good_owner: None,
    };
    let good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);
    let snapshot = ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        "Unauthorized".to_string(),
        codexbar::core::ProviderStateKind::NeedsAuthentication,
    );
    let error = ProviderError::AuthRequired;
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good.clone());

    let preserved = super::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        snapshot,
        &error,
    );

    assert_eq!(preserved.error, None);
    assert_eq!(preserved.primary.used_percent, 42.0);
}

#[test]
fn codex_transient_transport_failure_helper_uses_typed_policy() {
    let metadata = instantiate_provider(ProviderId::Codex).metadata().clone();
    let result = ProviderFetchResult {
        usage: codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(42.0)),
        cost: None,
        wayfinder_usage: None,
        open_ai_api_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: Vec::new(),
        source_label: "OAuth".to_string(),
        has_successful_claude_cli_quota: false,
        pace_authoritative: true,
        account_identity: None,
        last_good_owner: None,
    };
    let good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Codex, &metadata, &result, None);
    let snapshot = ProviderUsageSnapshot::from_error(
        ProviderId::Codex,
        &metadata,
        "Timeout".to_string(),
        codexbar::core::ProviderStateKind::Unknown,
    );
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good);

    let preserved = super::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Codex,
        snapshot,
        &ProviderError::Timeout,
    );

    assert_eq!(preserved.error, None);
    assert_eq!(preserved.primary.used_percent, 42.0);
}

#[test]
fn claude_repeated_auth_failure_surfaces_error() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let result = ProviderFetchResult {
        usage: codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(42.0)),
        cost: None,
        wayfinder_usage: None,
        open_ai_api_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: Vec::new(),
        source_label: "OAuth".to_string(),
        has_successful_claude_cli_quota: false,
        pace_authoritative: true,
        account_identity: None,
        last_good_owner: None,
    };
    let good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);
    let first_error = ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        "Unauthorized".to_string(),
        codexbar::core::ProviderStateKind::NeedsAuthentication,
    );
    let second_error = first_error.clone();
    let failure = ProviderError::AuthRequired;
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good);

    let _ = super::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        first_error,
        &failure,
    );
    let surfaced = super::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        second_error,
        &failure,
    );

    assert!(surfaced.error.is_some());
}

#[test]
fn claude_cloudflare_challenge_retains_prior_usage_while_surfaceing_guidance() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let result = ProviderFetchResult {
        usage: codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(42.0)),
        cost: None,
        wayfinder_usage: None,
        open_ai_api_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: Vec::new(),
        source_label: "OAuth".to_string(),
        has_successful_claude_cli_quota: false,
        pace_authoritative: true,
        account_identity: None,
        last_good_owner: None,
    };
    let good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);
    let challenge = codexbar::providers::claude::CLOUDFLARE_CHALLENGE_MESSAGE;
    let error = ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        challenge.to_string(),
        codexbar::core::ProviderStateKind::Unknown,
    );
    let failure = ProviderError::Other(challenge.to_string());
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good);

    let surfaced = super::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        error,
        &failure,
    );

    assert_eq!(surfaced.error, None);
    assert_eq!(surfaced.primary.used_percent, 42.0);
    assert_eq!(
        super::providers::preserve_last_good_transient_failure(
            &mut state,
            ProviderId::Claude,
            ProviderUsageSnapshot::from_error(
                ProviderId::Claude,
                &metadata,
                challenge.to_string(),
                codexbar::core::ProviderStateKind::Unknown,
            ),
            &failure,
        )
        .error
        .as_deref(),
        Some(challenge)
    );
}

#[test]
fn claude_cloudflare_challenge_keeps_prior_usage_when_guidance_surfaces() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let result = ProviderFetchResult {
        usage: codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(42.0)),
        cost: None,
        wayfinder_usage: None,
        open_ai_api_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: Vec::new(),
        source_label: "Web".to_string(),
        has_successful_claude_cli_quota: false,
        pace_authoritative: true,
        account_identity: None,
        last_good_owner: None,
    };
    let mut good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);
    good.updated_at = "2026-09-01T00:00:00Z".to_string();
    let error = ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        codexbar::providers::claude::CLOUDFLARE_CHALLENGE_MESSAGE.to_string(),
        codexbar::core::ProviderStateKind::Unknown,
    );
    let failure =
        ProviderError::Other(codexbar::providers::claude::CLOUDFLARE_CHALLENGE_MESSAGE.to_string());
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good.clone());

    let first = super::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        error.clone(),
        &failure,
    );
    let second = super::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        error,
        &failure,
    );

    assert_eq!(first.error, None);
    assert_eq!(first.primary.used_percent, 42.0);
    assert_eq!(
        second.error.as_deref(),
        Some(codexbar::providers::claude::CLOUDFLARE_CHALLENGE_MESSAGE,)
    );
    assert_eq!(second.primary.used_percent, 42.0);
    assert_eq!(second.updated_at, good.updated_at);
}

#[test]
fn claude_cli_parse_failure_keeps_last_good_every_time() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let result = ProviderFetchResult {
        usage: codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(17.0)),
        cost: None,
        wayfinder_usage: None,
        open_ai_api_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: Vec::new(),
        source_label: "CLI".to_string(),
        has_successful_claude_cli_quota: true,
        pace_authoritative: true,
        account_identity: None,
        last_good_owner: None,
    };
    let good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);
    let err = ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        "Parse error: Empty output from Claude CLI".to_string(),
        codexbar::core::ProviderStateKind::Unknown,
    );
    let failure = ProviderError::Parse("Empty output from Claude CLI".to_string());
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good.clone());

    let first = super::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        err.clone(),
        &failure,
    );
    let second = super::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        err,
        &failure,
    );

    assert_eq!(first.error, None);
    assert_eq!(first.primary.used_percent, 17.0);
    assert!(!first.has_successful_claude_cli_quota);
    // Parse failures keep last-good on every refresh (upstream #2247), unlike one-shot auth.
    assert_eq!(second.error, None);
    assert_eq!(second.primary.used_percent, 17.0);
}

#[test]
fn claude_hard_credentials_missing_does_not_preserve_stale() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let result = ProviderFetchResult {
        usage: codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(17.0)),
        cost: None,
        wayfinder_usage: None,
        open_ai_api_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: Vec::new(),
        source_label: "OAuth".to_string(),
        has_successful_claude_cli_quota: false,
        pace_authoritative: true,
        account_identity: None,
        last_good_owner: None,
    };
    let good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);
    let err = ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        "OAuth error: Claude OAuth credentials not found. Run `claude` to authenticate."
            .to_string(),
        codexbar::core::ProviderStateKind::NeedsAuthentication,
    );
    let failure = ProviderError::OAuth(
        "Claude OAuth credentials not found. Run `claude` to authenticate.".to_string(),
    );
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good);

    let out = super::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        err,
        &failure,
    );
    assert!(out.error.is_some());
    assert_eq!(
        out.error_state,
        codexbar::core::ProviderStateKind::NeedsAuthentication,
        "hard auth failure must carry its classification on the snapshot"
    );
}

#[test]
fn claude_error_message_removes_upstream_swift_cancellation() {
    let message = super::friendly_provider_error(
        ProviderId::Claude,
        "The operation couldn't be completed. (Swift.CancellationError error 1.)",
    );

    assert!(!message.contains("Swift"));
    assert!(message.contains("Claude usage fetch was cancelled"));
    assert!(message.contains("Refresh Claude"));
}

#[test]
fn claude_error_message_explains_missing_sign_in() {
    let message = super::friendly_provider_error(
        ProviderId::Claude,
        "OAuth error: Claude OAuth credentials not found. Run `claude` to authenticate.",
    );

    assert_eq!(
        message,
        "Claude sign-in was not found. Run `claude` once to authenticate, then refresh Claude in Win-CodexBar."
    );
}

#[test]
fn claude_cloudflare_error_preserves_distinct_recovery_guidance() {
    let challenge = codexbar::providers::claude::CLOUDFLARE_CHALLENGE_MESSAGE;
    let message = super::friendly_provider_error(ProviderId::Claude, challenge);

    assert_eq!(message, challenge);
    assert!(message.contains("OAuth"));
    assert!(message.contains("different network"));
}

#[test]
fn non_claude_error_message_is_preserved() {
    let message = super::friendly_provider_error(
        ProviderId::Codex,
        "OAuth error: Claude OAuth credentials not found. Run `claude` to authenticate.",
    );

    assert_eq!(
        message,
        "OAuth error: Claude OAuth credentials not found. Run `claude` to authenticate."
    );
}

#[test]
fn chart_data_serde_roundtrip_preserves_fields() {
    use super::{
        DailyCostPoint, DailyTokenPoint, DailyUsageBreakdown, ProviderChartData,
        QuotaWindowHistoryBridge, QuotaWindowHistoryPoint, ServiceUsagePoint,
    };

    let original = ProviderChartData {
        provider_id: "codex".into(),
        cost_history: vec![
            DailyCostPoint {
                date: "2025-01-01".into(),
                value: Some(1.25),
                incomplete_request_count: None,
            },
            DailyCostPoint {
                date: "2025-01-02".into(),
                value: Some(0.0),
                incomplete_request_count: None,
            },
        ],
        credits_history: vec![DailyCostPoint {
            date: "2025-01-01".into(),
            value: Some(42.0),
            incomplete_request_count: None,
        }],
        usage_breakdown: vec![DailyUsageBreakdown {
            day: "2025-01-01".into(),
            services: vec![
                ServiceUsagePoint {
                    service: "gpt-4o".into(),
                    credits_used: 10.0,
                },
                ServiceUsagePoint {
                    service: "gpt-4o-mini".into(),
                    credits_used: 3.5,
                },
            ],
            total_credits_used: 13.5,
        }],
        local_usage: None,
        tokens_history: vec![DailyTokenPoint {
            date: "2025-01-01".into(),
            tokens: 123_456,
        }],
        tokens_incomplete: true,
        quota_window_history: Some(QuotaWindowHistoryBridge {
            provider_id: "codex".into(),
            account_scope: Some("person@example.com".into()),
            windows: vec![QuotaWindowHistoryPoint {
                offset: 0,
                start: "2025-01-01T00:00:00Z".into(),
                end: "2025-01-08T00:00:00Z".into(),
                total_tokens: Some(123_456),
                total_cost_usd: None,
                tokens_are_complete: true,
                cost_is_complete: false,
                entry_count: 2,
                boundaries_are_estimated: true,
            }],
            history_coverage_established: false,
        }),
    };

    let json = serde_json::to_string(&original).expect("serialize");
    assert!(
        json.contains("\"providerId\":\"codex\""),
        "camelCase providerId: {json}"
    );
    assert!(json.contains("\"costHistory\""));
    assert!(json.contains("\"creditsHistory\""));
    assert!(json.contains("\"usageBreakdown\""));
    assert!(json.contains("\"localUsage\":null"));
    assert!(json.contains("\"creditsUsed\":10.0"));
    assert!(json.contains("\"totalCreditsUsed\":13.5"));
    assert!(json.contains("\"tokensHistory\""));
    assert!(json.contains("\"tokens\":123456"));
    assert!(json.contains("\"tokensIncomplete\":true"));

    let back: ProviderChartData = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.provider_id, "codex");
    assert_eq!(back.cost_history.len(), 2);
    assert_eq!(back.cost_history[0].date, "2025-01-01");
    assert_eq!(back.credits_history[0].value, Some(42.0));
    assert_eq!(back.usage_breakdown[0].services.len(), 2);
    assert_eq!(back.usage_breakdown[0].total_credits_used, 13.5);
    assert_eq!(back.tokens_history[0].tokens, 123_456);
    assert!(back.tokens_incomplete);
    let history = back.quota_window_history.expect("quota history");
    assert_eq!(history.account_scope.as_deref(), Some("person@example.com"));
    assert_eq!(history.windows[0].offset, 0);
    assert!(history.windows[0].boundaries_are_estimated);
    assert!(!history.windows[0].cost_is_complete);
    assert!(!history.history_coverage_established);

    let mut legacy = serde_json::to_value(&original).expect("serialize legacy fixture");
    legacy
        .as_object_mut()
        .expect("chart object")
        .remove("quotaWindowHistory");
    let legacy_back: ProviderChartData =
        serde_json::from_value(legacy).expect("legacy chart payload remains readable");
    assert!(legacy_back.quota_window_history.is_none());
}

#[test]
fn chart_data_for_unknown_provider_is_empty() {
    let data =
        super::build_provider_chart_data("this-provider-definitely-does-not-exist".into(), None);
    assert_eq!(data.provider_id, "this-provider-definitely-does-not-exist");
    assert!(data.credits_history.is_empty());
    assert!(data.usage_breakdown.is_empty());
}

#[test]
fn japanese_provider_snapshot_localizes_weekly_label() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let usage = codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(10.0))
        .with_secondary(codexbar::core::RateWindow::new(20.0));
    let result = ProviderFetchResult {
        usage,
        cost: None,
        wayfinder_usage: None,
        open_ai_api_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: Vec::new(),
        source_label: "OAuth".to_string(),
        has_successful_claude_cli_quota: false,
        pace_authoritative: true,
        account_identity: None,
        last_good_owner: None,
    };

    let snapshot =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);

    // Secondary label stays raw; localization happens at render time.
    assert_eq!(snapshot.secondary_label, Some("Weekly".to_string()));
}

#[test]
fn japanese_provider_snapshot_localizes_pace_reserve_description() {
    use chrono::{Duration, Utc};

    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let now = Utc::now();
    // 7-day window, half elapsed, 40% used → 10% ahead of pace, will last to reset.
    let secondary = codexbar::core::RateWindow::with_details(
        40.0,
        Some(7 * 24 * 60),
        Some(now + Duration::minutes(7 * 24 * 60 / 2)),
        None,
    );
    let usage = codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(10.0))
        .with_secondary(secondary);
    let result = ProviderFetchResult {
        usage,
        cost: None,
        wayfinder_usage: None,
        open_ai_api_usage: None,
        inventory: Vec::new(),
        reset_credits: None,
        display_details: Vec::new(),
        source_label: "OAuth".to_string(),
        has_successful_claude_cli_quota: false,
        pace_authoritative: true,
        account_identity: None,
        last_good_owner: None,
    };

    let snapshot =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);

    // Reserve data stays raw; localization happens at render time.
    let secondary = snapshot.secondary.as_ref().expect("secondary window");
    assert!(secondary.reserve_percent.is_some());
    assert!(secondary.reserve_will_last_to_reset);
    assert!(secondary.reserve_description.is_none());
}

#[test]
fn chart_data_requires_account_email_for_codex() {
    let (credits_history, usage_breakdown) =
        super::chart::load_openai_dashboard_chart_data_for_test("codex", None);
    assert!(credits_history.is_empty());
    assert!(usage_breakdown.is_empty());
}

#[test]
fn cookie_options_for_cookie_supporting_provider() {
    let opts = super::cookie_source_options_for("codex", Language::English);
    let values: Vec<_> = opts.iter().map(|o| o.value.as_str()).collect();
    assert_eq!(values, vec!["auto", "manual", "off"]);
    assert!(opts.iter().any(|o| o.label == "Automatic"));
    assert!(opts.iter().any(|o| o.label == "Manual"));
    assert!(opts.iter().any(|o| o.label == "Disabled"));
}

#[test]
fn replicate_cookie_options_allow_automatic_and_manual_sessions() {
    let opts = super::cookie_source_options_for("replicate", Language::English);
    let values: Vec<_> = opts.iter().map(|option| option.value.as_str()).collect();
    assert_eq!(values, vec!["auto", "manual"]);
}

#[test]
fn raycast_cookie_options_include_off_and_a_pinned_manual_session() {
    let opts = super::cookie_source_options_for("raycast", Language::English);
    let values: Vec<_> = opts.iter().map(|option| option.value.as_str()).collect();
    assert_eq!(values, vec!["auto", "manual", "off"]);
}

#[test]
fn cookie_options_empty_for_providers_without_picker() {
    assert!(super::cookie_source_options_for("anthropic", Language::English).is_empty());
    assert!(super::cookie_source_options_for("unknown", Language::English).is_empty());
}

#[test]
fn region_options_for_regional_provider() {
    let opts = super::region_options_for("alibaba", Language::English);
    let values: Vec<_> = opts.iter().map(|o| o.value.as_str()).collect();
    assert_eq!(values, vec!["singapore", "us", "germany", "hongkong", "cn"]);
}

#[test]
fn alibaba_token_plan_region_options() {
    let opts = super::region_options_for("alibabatokenplan", Language::English);
    let values: Vec<_> = opts.iter().map(|o| o.value.as_str()).collect();
    let labels: Vec<_> = opts.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(values, vec!["cn", "intl", "cn-personal", "intl-personal"]);
    assert_eq!(
        labels,
        vec![
            "China Team",
            "International Team",
            "China Personal/Solo",
            "International Personal/Solo"
        ]
    );
}

#[test]
fn minimax_region_options_match_upstream_hosts() {
    let opts = super::region_options_for("minimax", Language::English);
    let values: Vec<_> = opts.iter().map(|o| o.value.as_str()).collect();
    let labels: Vec<_> = opts.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(values, vec!["global", "cn"]);
    assert_eq!(
        labels,
        vec![
            "Global (platform.minimax.io)",
            "China mainland (platform.minimaxi.com)"
        ]
    );
}

#[test]
fn kimi_region_options_match_regional_hosts() {
    let opts = super::region_options_for("kimi", Language::English);
    let values: Vec<_> = opts.iter().map(|option| option.value.as_str()).collect();
    assert_eq!(values, vec!["china", "international"]);
}

#[test]
fn cookie_and_region_options_are_localized_for_every_provider() {
    // Every description and region label must come from the locale catalog: none may stay
    // English in another language, except the labels in ALLOWED_SAME_AS_ENGLISH, which the
    // catalogs legitimately keep identical to the English text (proper names).
    const ALLOWED_SAME_AS_ENGLISH: &[(Language, &str, &str)] = &[
        (Language::Spanish, "kimi", "china"),
        (Language::Spanish, "minimax", "global"),
        (Language::Spanish, "zai", "global"),
        (Language::PortugueseBrazil, "kimi", "china"),
        (Language::PortugueseBrazil, "minimax", "global"),
        (Language::PortugueseBrazil, "zai", "global"),
    ];
    for &lang in Language::all() {
        if lang == Language::English {
            continue;
        }
        for provider in codexbar::core::ProviderId::all() {
            let id = provider.cli_name();
            let en = super::cookie_source_options_for(id, Language::English);
            let other = super::cookie_source_options_for(id, lang);
            assert_eq!(en.len(), other.len(), "{lang:?} {id}");
            for (en, other) in en.iter().zip(&other) {
                if let Some(text) = &en.description {
                    assert_ne!(
                        Some(text),
                        other.description.as_ref(),
                        "{lang:?} {id} {} description",
                        en.value
                    );
                }
            }
            let en = super::region_options_for(id, Language::English);
            let other = super::region_options_for(id, lang);
            assert_eq!(en.len(), other.len(), "{lang:?} {id} regions");
            for (en, other) in en.iter().zip(&other) {
                if ALLOWED_SAME_AS_ENGLISH.contains(&(lang, id, en.value.as_str())) {
                    continue;
                }
                assert_ne!(en.label, other.label, "{lang:?} {id} region {}", en.value);
            }
        }
    }
}

#[test]
fn english_region_labels_match_the_provider_display_names() {
    use codexbar::providers::{AlibabaRegion, AlibabaTokenPlanRegion, KimiRegion, MiniMaxRegion};
    let labels = |id| {
        super::region_options_for(id, Language::English)
            .into_iter()
            .map(|option| option.label)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        labels("alibaba"),
        AlibabaRegion::ALL
            .iter()
            .map(|r| r.display_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        labels("alibabatokenplan"),
        AlibabaTokenPlanRegion::ALL
            .iter()
            .map(|r| r.display_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        labels("kimi"),
        KimiRegion::ALL
            .iter()
            .map(|r| r.display_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        labels("minimax"),
        [MiniMaxRegion::Global, MiniMaxRegion::ChinaMainland].map(|r| r.display_name())
    );
}

#[test]
fn region_options_empty_for_non_regional_provider() {
    assert!(super::region_options_for("claude", Language::English).is_empty());
    assert!(super::region_options_for("codex", Language::English).is_empty());
}

#[test]
fn cookie_source_option_roundtrips_serde() {
    let opt = super::CookieSourceOption {
        value: "auto".to_string(),
        label: "Automatic".to_string(),
        description: Some("Imports browser cookies.".to_string()),
    };
    let json = serde_json::to_string(&opt).unwrap();
    let back: super::CookieSourceOption = serde_json::from_str(&json).unwrap();
    assert_eq!(opt, back);
}

#[test]
fn region_option_roundtrips_serde() {
    let opt = super::RegionOption {
        value: "intl".to_string(),
        label: "International".to_string(),
    };
    let json = serde_json::to_string(&opt).unwrap();
    let back: super::RegionOption = serde_json::from_str(&json).unwrap();
    assert_eq!(opt, back);
}

// ── Phase 6d — credential detection UIs ────────────────────────

#[test]
fn open_path_rejects_empty_path() {
    let err = super::open_path(String::new()).unwrap_err();
    assert!(err.to_lowercase().contains("empty"));
}

#[test]
fn open_path_rejects_relative_path() {
    let err = super::open_path("relative/path".into()).unwrap_err();
    assert!(err.contains("absolute"));
}

#[test]
fn open_path_rejects_missing_path() {
    let missing = std::env::temp_dir()
        .join(format!("codexbar-phase6d-missing-{}", std::process::id()))
        .join("does-not-exist");
    let err = super::open_path(missing.to_string_lossy().into_owned()).unwrap_err();
    assert!(err.contains("not found"));
}

#[test]
fn external_url_validator_accepts_http_and_https() {
    assert_eq!(
        super::validate_external_url(" https://github.com/Finesssee/Win-CodexBar "),
        Ok("https://github.com/Finesssee/Win-CodexBar")
    );
    assert_eq!(
        super::validate_external_url("http://localhost:1420"),
        Ok("http://localhost:1420")
    );
}

#[test]
fn external_url_validator_rejects_non_web_and_control_urls() {
    assert!(super::validate_external_url("file:///C:/Windows/win.ini").is_err());
    assert!(super::validate_external_url("javascript:alert(1)").is_err());
    assert!(super::validate_external_url("https://example.com/\nmalicious").is_err());
}

// ── Phase 13 — E2E IPC harness ─────────────────────────────────
//
// Build the full bootstrap payload and prove that every shared
// `ProviderId` variant ends up in the provider catalog with a
// non-empty id + display name. If a new provider is added to the
// enum but never wired through the desktop catalog, this test will
// fail with `missing provider in bootstrap catalog: <id>`.

#[test]
fn bootstrap_payload_exposes_every_provider_variant() {
    // Built from default settings instead of `Settings::load()` so a retired
    // provider enabled in the developer's real settings.json cannot change
    // the catalog size.
    let payload = super::bootstrap_state_for(Settings::default());

    let catalog_ids: std::collections::HashSet<String> = payload
        .providers
        .iter()
        .map(|entry| entry.id.clone())
        .collect();

    for entry in &payload.providers {
        assert!(!entry.id.is_empty(), "provider entry has empty id");
        assert!(
            !entry.display_name.is_empty(),
            "provider {} has empty display_name",
            entry.id
        );
    }

    // Deprecated providers (KimiK2, CrossModel) are soft-removed from the
    // desktop catalog unless already enabled (upstream #2254); they are
    // intentionally absent from the default bootstrap payload.
    let active: Vec<ProviderId> = ProviderId::all()
        .iter()
        .copied()
        .filter(|p| !p.is_deprecated())
        .collect();

    for provider in &active {
        let expected = provider.cli_name().to_string();
        assert!(
            catalog_ids.contains(&expected),
            "missing provider in bootstrap catalog: {expected}"
        );
    }

    assert_eq!(
        catalog_ids.len(),
        active.len(),
        "bootstrap catalog size drifted from the active (non-deprecated) providers"
    );

    // Sanity — payload must also round-trip through JSON cleanly so
    // the TypeScript bridge never sees a partially-populated record.
    let encoded = serde_json::to_string(&payload).expect("serialize bootstrap");
    assert!(encoded.contains("contractVersion"));
    assert!(encoded.contains("\"providers\""));
    assert!(encoded.contains("\"settings\""));
}

// Issue #684: the catalog size depends only on the settings passed in. A
// deprecated provider that is still enabled (the state that made the old
// test read 79 entries on a developer machine) is listed exactly once, and
// building the payload twice from the same settings gives the same catalog.
#[test]
fn bootstrap_catalog_depends_only_on_supplied_settings() {
    let active_count = ProviderId::all()
        .iter()
        .filter(|provider| !provider.is_deprecated())
        .count();

    let mut settings = Settings::default();
    settings
        .enabled_providers
        .insert(ProviderId::KimiK2.cli_name().to_string());

    let first = super::bootstrap_state_for(settings.clone());
    let second = super::bootstrap_state_for(settings);

    let ids = |payload: &super::BootstrapState| -> Vec<String> {
        payload
            .providers
            .iter()
            .map(|entry| entry.id.clone())
            .collect()
    };
    assert_eq!(ids(&first), ids(&second));
    assert_eq!(first.providers.len(), active_count + 1);
    assert_eq!(
        ids(&first)
            .iter()
            .filter(|id| id.as_str() == ProviderId::KimiK2.cli_name())
            .count(),
        1
    );
    assert!(
        !ids(&first)
            .iter()
            .any(|id| id.as_str() == ProviderId::CrossModel.cli_name()),
        "a deprecated provider that is not enabled stays hidden"
    );
}
