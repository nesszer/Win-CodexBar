use super::*;

#[test]
fn build_provider_detail_populates_identity_urls() {
    let (detail, _settings, _id) =
        crate::commands::build_provider_detail("claude").expect("known provider");
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
    let err = crate::commands::build_provider_detail("not-a-provider").unwrap_err();
    assert!(err.contains("not-a-provider"));
}

#[test]
fn provider_detail_roundtrips_through_serde() {
    let (detail, _settings, _id) =
        crate::commands::build_provider_detail("codex").expect("known provider");
    let json = serde_json::to_string(&detail).expect("serialize");
    // camelCase rename survives the round-trip.
    assert!(json.contains("\"displayName\""));
    assert!(json.contains("\"hasSnapshot\""));
    assert!(json.contains("\"statusPageUrl\""));
}

#[test]
fn provider_detail_carries_openai_daily_usage_only_when_present() {
    let (mut detail, _settings, _id) =
        crate::commands::build_provider_detail("openaiapi").expect("known provider");
    assert!(detail.open_ai_api_usage.is_none());
    let json = serde_json::to_string(&detail).expect("serialize");
    assert!(!json.contains("openAiApiUsage"));

    detail.open_ai_api_usage = Some(crate::commands::OpenAiApiUsageSnapshot {
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
    let result = ProviderFetchResult::new(
        codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(10.0)),
        "OAuth",
    );
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

    let items =
        crate::commands::usage_item_descriptors(Some(&snapshot), &settings, ProviderId::Codex);

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
    let mut result = ProviderFetchResult::new(
        codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(10.0)),
        "OAuth",
    );
    result.display_details = vec![
        codexbar::core::ProviderDisplayDetail::new("d1", "Rate limits", "5").expect("valid detail"),
        codexbar::core::ProviderDisplayDetail::new("d2", "Rate limits", "6").expect("valid detail"),
        codexbar::core::ProviderDisplayDetail::new("d3", "owner@example.com quota", "1")
            .expect("valid detail"),
    ];
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

    let items =
        crate::commands::usage_item_descriptors(Some(&snapshot), &settings, ProviderId::Codex);

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
        crate::commands::bridge::pace::stage_str(PaceStage::OnTrack),
        "on_track"
    );
    assert_eq!(
        crate::commands::bridge::pace::stage_str(PaceStage::SlightlyAhead),
        "slightly_ahead"
    );
    assert_eq!(
        crate::commands::bridge::pace::stage_str(PaceStage::FarAhead),
        "far_ahead"
    );
    assert_eq!(
        crate::commands::bridge::pace::stage_str(PaceStage::SlightlyBehind),
        "slightly_behind"
    );
    assert_eq!(
        crate::commands::bridge::pace::stage_str(PaceStage::Behind),
        "behind"
    );
    assert_eq!(
        crate::commands::bridge::pace::stage_str(PaceStage::FarBehind),
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
