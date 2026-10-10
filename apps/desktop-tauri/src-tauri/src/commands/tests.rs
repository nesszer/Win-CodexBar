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

mod cookie_region_options;
mod provider_cache;
mod provider_detail;

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
    let result = ProviderFetchResult::new(usage, "OAuth");

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
    let result = ProviderFetchResult::new(usage, "OAuth");

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
