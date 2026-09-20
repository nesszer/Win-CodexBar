use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::{TimeZone, Utc};

use super::*;
use crate::cli::serve::collection::{ProviderFetchEnvelope, RawCostPayload, SnapshotCollection};
use crate::cli::serve::dashboard;
use crate::cli::serve::dashboard::coordinator::{SnapshotArtifacts, SnapshotArtifactsBuildFn};
use crate::cli::serve::dashboard::snapshot::{DashboardIdentity, SnapshotInput, build_snapshot};
use crate::core::{NamedRateWindow, ProviderFetchResult, RateWindow, UsageSnapshot};
use crate::providers::codex::CodexApi;

fn at(hour: u32) -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 14, hour, 0, 0)
        .single()
        .unwrap()
}

fn provider(id: &str, fetch: Result<ProviderFetchResult, String>) -> ProviderFetchEnvelope {
    ProviderFetchEnvelope {
        id: id.to_string(),
        display_name: format!("Sensitive {id} name"),
        session_label: "Session".to_string(),
        weekly_label: "Weekly".to_string(),
        fetch,
    }
}

fn usage(primary: RateWindow) -> UsageSnapshot {
    let mut usage = UsageSnapshot::new(primary);
    usage.updated_at = at(1);
    usage.account_email = Some("owner@example.test".to_string());
    usage.login_method = Some("Sensitive plan".to_string());
    usage
}

fn codex_usage_from_json(json: serde_json::Value) -> UsageSnapshot {
    CodexApi::new()
        .build_result_from_json_for_test(&json)
        .expect("Codex usage")
        .0
}

fn input(
    providers: Vec<ProviderFetchEnvelope>,
    costs: HashMap<String, RawCostPayload>,
) -> SnapshotCollection {
    let enabled = providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect::<BTreeSet<_>>();
    input_with_enabled(providers, costs, enabled)
}

fn input_with_enabled(
    providers: Vec<ProviderFetchEnvelope>,
    costs: HashMap<String, RawCostPayload>,
    enabled: BTreeSet<String>,
) -> SnapshotCollection {
    let order = enabled.iter().cloned().collect();
    SnapshotCollection {
        providers,
        costs,
        claude_accounts: None,
        generated_at: at(0),
        refresh_seconds: 60,
        order,
        enabled,
    }
}

fn metrics_snapshot(
    providers: Vec<ProviderFetchEnvelope>,
    costs: HashMap<String, RawCostPayload>,
) -> MetricsSnapshot {
    MetricsSnapshot::from_collection(&input(providers, costs))
}

fn artifacts(collection: SnapshotCollection) -> SnapshotArtifacts<MetricsSnapshot> {
    let metrics = MetricsSnapshot::from_collection(&collection);
    let input = SnapshotInput {
        collection,
        identity: DashboardIdentity::Full,
        version: Some("0.56.8".to_string()),
        usage_bars_show_used: None,
    };
    SnapshotArtifacts {
        dashboard: build_snapshot(&input),
        sidecar: Some(metrics),
    }
}

fn samples<'a>(body: &'a str, metric: &'a str) -> impl Iterator<Item = &'a str> {
    body.lines().filter(move |line| {
        line.starts_with(metric)
            && line
                .as_bytes()
                .get(metric.len())
                .is_some_and(|next| matches!(next, b' ' | b'{'))
    })
}

fn metric_name(line: &str) -> Option<&str> {
    if let Some(rest) = line
        .strip_prefix("# HELP ")
        .or_else(|| line.strip_prefix("# TYPE "))
    {
        return rest.split_ascii_whitespace().next();
    }
    if line.starts_with('#') || line.is_empty() {
        return None;
    }
    line.split(['{', ' ', '\t']).next()
}

fn assert_metric_families_are_contiguous(body: &str) {
    let mut closed = BTreeSet::new();
    let mut current = None;
    for line in body.lines() {
        let Some(name) = metric_name(line) else {
            continue;
        };
        if current == Some(name) {
            continue;
        }
        if let Some(previous) = current.replace(name) {
            closed.insert(previous);
        }
        assert!(
            !closed.contains(name),
            "metric family {name} is split into multiple groups"
        );
    }
}

#[test]
fn renders_enabled_provider_health_and_codex_metrics_without_private_text() {
    let mut codex_usage = usage(RateWindow::with_details(25.0, Some(300), Some(at(3)), None));
    codex_usage.secondary = Some(RateWindow::with_details(
        40.0,
        Some(10_080),
        Some(at(4)),
        None,
    ));
    let mut costs = HashMap::new();
    costs.insert(
        "codex".to_string(),
        RawCostPayload {
            today_usd: Some(1.25),
            last_30_days_usd: Some(12.5),
        },
    );
    costs.insert(
        "anthropic".to_string(),
        RawCostPayload {
            today_usd: Some(0.75),
            last_30_days_usd: None,
        },
    );
    let snapshot = metrics_snapshot(
        vec![
            provider(
                "openai",
                Ok(ProviderFetchResult::new(codex_usage, "sensitive-source")),
            ),
            provider("claude", Err("private provider failure".to_string())),
        ],
        costs,
    );

    let body = render_at(&snapshot, at(5)).unwrap();
    assert!(body.contains("codexbar_up 1\n"));
    assert!(body.contains("codexbar_provider_up{provider=\"codex\"} 1\n"));
    assert!(body.contains("codexbar_provider_up{provider=\"claude\"} 0\n"));
    assert!(body.contains("codexbar_quota_session_used_ratio{provider=\"codex\"} 0.25\n"));
    assert!(body.contains(
        "codexbar_quota_session_reset_timestamp_seconds{provider=\"codex\"} 1789354800\n"
    ));
    assert!(body.contains("codexbar_quota_weekly_remaining_ratio{provider=\"codex\"} 0.6\n"));
    assert!(body.contains("codexbar_cost_last_30_days_usd{provider=\"codex\"} 12.5\n"));
    assert!(!body.contains("codexbar_cost_today_usd{provider=\"claude\"}"));
    assert!(!body.contains("codexbar_build_info"));
    assert!(!body.contains("version=\""));
    assert!(!body.contains("window=\""));
    assert!(body.ends_with('\n'));

    for private in [
        "owner@example.test",
        "Sensitive plan",
        "Sensitive codex name",
        "sensitive-source",
        "private provider failure",
    ] {
        assert!(!body.contains(private), "private text leaked: {private}");
    }
}

#[test]
fn exports_only_fixed_quota_slots_and_omits_extras() {
    let mut provider_usage = usage(RateWindow::no_active_session());
    provider_usage.secondary = Some(RateWindow::with_details(35.0, Some(10_080), None, None));
    provider_usage.tertiary = Some(RateWindow::with_details(45.0, Some(43_200), None, None));
    provider_usage = provider_usage.with_code_review(RateWindow::new(55.0));
    provider_usage.extra_rate_windows.extend([
        NamedRateWindow::new("reset-credits", "Reset credits", RateWindow::new(0.0)),
        NamedRateWindow::new("customer-defined", "Tenant Secret", RateWindow::new(99.0)),
        NamedRateWindow::new("codex-spark", "Codex Spark", RateWindow::new(20.0)),
    ]);
    let snapshot = metrics_snapshot(
        vec![provider(
            "codex",
            Ok(ProviderFetchResult::new(provider_usage, "cli")),
        )],
        HashMap::new(),
    );

    let body = render_at(&snapshot, at(2)).unwrap();
    assert_eq!(
        samples(&body, "codexbar_quota_session_used_ratio").count(),
        0
    );
    assert!(body.contains("codexbar_quota_weekly_used_ratio{provider=\"codex\"} 0.35\n"));
    assert!(body.contains("codexbar_quota_code_review_used_ratio{provider=\"codex\"} 0.55\n"));
    assert!(body.contains("codexbar_quota_monthly_used_ratio{provider=\"codex\"} 0.45\n"));
    for excluded in [
        "window=\"",
        "reset-credits",
        "customer-defined",
        "Tenant Secret",
        "codex-spark",
    ] {
        assert!(
            !body.contains(excluded),
            "unexpected exported value: {excluded}"
        );
    }
}

#[test]
fn canonical_provider_labels_keep_metric_families_bounded_and_contiguous() {
    let mut codex_usage = usage(RateWindow::new(25.0));
    codex_usage.extra_rate_windows.push(NamedRateWindow::new(
        "dynamic-tenant-id",
        "Sensitive tenant label",
        RateWindow::new(90.0),
    ));
    let body = render_at(
        &metrics_snapshot(
            vec![
                provider("openai", Ok(ProviderFetchResult::new(codex_usage, "cli"))),
                provider(
                    "anthropic",
                    Ok(ProviderFetchResult::new(
                        usage(RateWindow::new(50.0)),
                        "cli",
                    )),
                ),
                provider(
                    "co\"dex\\lan\nnode",
                    Ok(ProviderFetchResult::new(
                        usage(RateWindow::new(50.0)),
                        "cli",
                    )),
                ),
            ],
            HashMap::new(),
        ),
        at(2),
    )
    .unwrap();

    assert_metric_families_are_contiguous(&body);
    assert_eq!(samples(&body, "codexbar_provider_up").count(), 2);
    assert!(body.contains("codexbar_provider_up{provider=\"codex\"} 1\n"));
    assert!(body.contains("codexbar_provider_up{provider=\"claude\"} 1\n"));
    assert!(!body.contains("dynamic-tenant-id"));
    assert!(!body.contains("Sensitive tenant label"));
    assert!(!body.contains("co\\\"dex"));
}

#[test]
fn does_not_infer_unknown_informational_non_finite_or_out_of_range_values() {
    let mut primary = RateWindow::with_details(25.0, Some(300), None, None);
    primary.used_percent = f64::NAN;
    let mut model = RateWindow::new(25.0);
    model.used_percent = 125.0;
    let mut tertiary = RateWindow::with_details(25.0, Some(43_200), None, None);
    tertiary.used_percent = -25.0;
    let mut provider_usage = usage(primary);
    provider_usage.secondary = Some(RateWindow::informational("unknown allowance"));
    provider_usage = provider_usage.with_code_review(model);
    provider_usage.tertiary = Some(tertiary);
    let mut costs = HashMap::new();
    costs.insert(
        "codex".to_string(),
        RawCostPayload {
            today_usd: Some(f64::NAN),
            last_30_days_usd: Some(f64::INFINITY),
        },
    );

    let body = render_at(
        &metrics_snapshot(
            vec![provider(
                "codex",
                Ok(ProviderFetchResult::new(provider_usage, "cli")),
            )],
            costs,
        ),
        at(2),
    )
    .unwrap();

    assert_eq!(
        samples(&body, "codexbar_quota_session_used_ratio").count(),
        0
    );
    assert_eq!(
        samples(&body, "codexbar_quota_weekly_used_ratio").count(),
        0
    );
    assert_eq!(
        samples(&body, "codexbar_quota_code_review_used_ratio").count(),
        0
    );
    assert_eq!(
        samples(&body, "codexbar_quota_monthly_used_ratio").count(),
        0
    );
    assert_eq!(samples(&body, "codexbar_cost_today_usd").count(), 0);
    assert_eq!(samples(&body, "codexbar_cost_last_30_days_usd").count(), 0);
    assert!(!body.contains("unknown allowance"));
}

#[test]
fn parsed_missing_usage_is_not_exported_as_zero() {
    let usage = codex_usage_from_json(serde_json::json!({
        "rate_limit": {
            "primary_window": {
                "limit_window_seconds": 18_000,
                "reset_at": 1_789_354_800
            }
        }
    }));
    let body = render_at(
        &metrics_snapshot(
            vec![provider(
                "codex",
                Ok(ProviderFetchResult::new(usage, "oauth")),
            )],
            HashMap::new(),
        ),
        at(2),
    )
    .unwrap();

    assert!(body.contains("codexbar_provider_up{provider=\"codex\"} 1\n"));
    assert_eq!(
        samples(&body, "codexbar_quota_session_used_ratio").count(),
        0
    );
    assert_eq!(
        samples(&body, "codexbar_quota_session_reset_timestamp_seconds").count(),
        0
    );
}

#[test]
fn parsed_positional_fallback_without_cadence_is_not_exported() {
    let usage = codex_usage_from_json(serde_json::json!({
        "rate_limits": [
            { "used_percent": 10 },
            { "used_percent": 20 },
            { "used_percent": 30 },
            { "used_percent": 40 }
        ]
    }));
    let body = render_at(
        &metrics_snapshot(
            vec![provider(
                "codex",
                Ok(ProviderFetchResult::new(usage, "oauth")),
            )],
            HashMap::new(),
        ),
        at(2),
    )
    .unwrap();

    for metric in [
        "codexbar_quota_session_used_ratio",
        "codexbar_quota_weekly_used_ratio",
        "codexbar_quota_monthly_used_ratio",
        "codexbar_quota_code_review_used_ratio",
    ] {
        assert_eq!(samples(&body, metric).count(), 0, "unexpected {metric}");
    }
}

#[test]
fn parsed_verified_cadences_and_code_review_are_exported() {
    let named = codex_usage_from_json(serde_json::json!({
        "rate_limit": {
            "primary_window": {
                "used_percent": 25,
                "limit_window_seconds": 18_000
            },
            "secondary_window": {
                "used_percent": 40,
                "limit_window_seconds": 604_800
            },
            "code_review_window": { "used_percent": 55 }
        }
    }));
    let named_body = render_at(
        &metrics_snapshot(
            vec![provider(
                "codex",
                Ok(ProviderFetchResult::new(named, "oauth")),
            )],
            HashMap::new(),
        ),
        at(2),
    )
    .unwrap();
    assert!(named_body.contains("codexbar_quota_session_used_ratio{provider=\"codex\"} 0.25\n"));
    assert!(named_body.contains("codexbar_quota_weekly_used_ratio{provider=\"codex\"} 0.4\n"));
    assert!(
        named_body.contains("codexbar_quota_code_review_used_ratio{provider=\"codex\"} 0.55\n")
    );

    let array = codex_usage_from_json(serde_json::json!({
        "rate_limits": [
            { "used_percent": 45, "limit_window_seconds": 2_592_000 }
        ]
    }));
    let array_body = render_at(
        &metrics_snapshot(
            vec![provider(
                "codex",
                Ok(ProviderFetchResult::new(array, "oauth")),
            )],
            HashMap::new(),
        ),
        at(2),
    )
    .unwrap();
    assert!(array_body.contains("codexbar_quota_monthly_used_ratio{provider=\"codex\"} 0.45\n"));
}

#[test]
fn duplicate_series_invariant_failure_returns_http_500() {
    let mut snapshot = metrics_snapshot(
        vec![provider(
            "codex",
            Ok(ProviderFetchResult::new(
                usage(RateWindow::new(25.0)),
                "cli",
            )),
        )],
        HashMap::new(),
    );
    snapshot.providers.push(snapshot.providers[0].clone());

    let response = metrics_response(Some(&snapshot));

    assert!(response.starts_with("HTTP/1.1 500 Internal Server Error\r\n"));
    assert!(response.contains("codexbar_up 0\n"));
}

#[test]
fn freshness_metrics_are_deterministic_clamped_and_use_strict_staleness() {
    let snapshot = metrics_snapshot(
        vec![provider(
            "codex",
            Ok(ProviderFetchResult::new(
                usage(RateWindow::new(25.0)),
                "cli",
            )),
        )],
        HashMap::new(),
    );
    let at_threshold = render_at(
        &snapshot,
        snapshot.generated_at + chrono::Duration::seconds(180),
    )
    .unwrap();
    assert!(at_threshold.contains("codexbar_snapshot_age_seconds 180\n"));
    assert!(at_threshold.contains("codexbar_snapshot_stale 0\n"));

    let stale = render_at(
        &snapshot,
        snapshot.generated_at + chrono::Duration::seconds(181),
    )
    .unwrap();
    assert!(stale.contains("codexbar_snapshot_stale 1\n"));

    let before_provider_update = render_at(&snapshot, snapshot.generated_at).unwrap();
    assert!(
        before_provider_update
            .contains("codexbar_provider_data_age_seconds{provider=\"codex\"} 0\n")
    );
}

#[test]
fn partial_claude_failure_keeps_codex_metrics_usable() {
    let input = input_with_enabled(
        vec![
            provider(
                "codex",
                Ok(ProviderFetchResult::new(
                    usage(RateWindow::with_details(50.0, Some(300), None, None)),
                    "cli",
                )),
            ),
            provider("claude", Err("private failure".to_string())),
        ],
        HashMap::new(),
        ["codex", "claude", "cursor"]
            .into_iter()
            .map(str::to_string)
            .collect(),
    );
    let body = render_at(&MetricsSnapshot::from_collection(&input), at(2)).unwrap();

    assert!(body.contains("codexbar_provider_up{provider=\"codex\"} 1\n"));
    assert!(body.contains("codexbar_provider_up{provider=\"claude\"} 0\n"));
    assert!(body.contains("codexbar_provider_up{provider=\"cursor\"} 0\n"));
    assert!(body.contains("codexbar_quota_session_used_ratio{provider=\"codex\"} 0.5\n"));
    assert!(!body.contains("codexbar_quota_session_used_ratio{provider=\"claude\"}"));
    assert!(!body.contains("private failure"));

    let disabled = render_at(&metrics_snapshot(Vec::new(), HashMap::new()), at(2)).unwrap();
    assert_eq!(samples(&disabled, "codexbar_provider_up").count(), 0);
    assert!(disabled.contains("codexbar_up 1\n"));
}

#[tokio::test]
async fn scrape_reads_cache_and_single_flights_the_shared_snapshot_builder() {
    // The metrics route owns no provider client. Blocking the shared dashboard
    // builder proves scrapes return from cache and do not start a second path.
    let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
    let started_tx = Arc::new(Mutex::new(Some(started_tx)));
    let (release_tx, release_rx) = tokio::sync::oneshot::channel::<()>();
    let release_rx = Arc::new(Mutex::new(Some(release_rx)));
    let build_count = Arc::new(AtomicUsize::new(0));
    let payload = artifacts(input(
        vec![provider(
            "codex",
            Ok(ProviderFetchResult::new(
                usage(RateWindow::with_details(25.0, Some(300), None, None)),
                "cli",
            )),
        )],
        HashMap::new(),
    ));
    let builder_count = build_count.clone();
    let build: SnapshotArtifactsBuildFn<MetricsSnapshot> = Arc::new(move || {
        let started_tx = started_tx.clone();
        let release_rx = release_rx.clone();
        let build_count = builder_count.clone();
        let payload = payload.clone();
        Box::pin(async move {
            build_count.fetch_add(1, Ordering::SeqCst);
            if let Some(started_tx) = started_tx.lock().expect("poisoned").take() {
                started_tx
                    .send(())
                    .expect("background collection start receiver must remain alive");
            }
            let release_rx = release_rx.lock().expect("poisoned").take().unwrap();
            release_rx
                .await
                .expect("background collection release sender must remain alive");
            Ok(payload)
        })
    });
    let state = dashboard::DashboardState::stub_with_artifacts(
        build,
        3600,
        Some(DashboardIdentity::Redacted),
    );

    let first_snapshot = state.latest_metrics_snapshot();
    let first = metrics_response(first_snapshot.as_deref());
    assert!(first.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(first.contains("codexbar_up 0\n"));
    tokio::time::timeout(Duration::from_secs(5), started_rx)
        .await
        .expect("background collection did not start")
        .expect("background collection start sender dropped");

    let second_snapshot = state.latest_metrics_snapshot();
    let second = metrics_response(second_snapshot.as_deref());
    assert!(second.contains("codexbar_up 0\n"));
    assert_eq!(build_count.load(Ordering::SeqCst), 1);
    release_tx
        .send(())
        .expect("background collection must still be waiting");
    state.coordinator.get().await.unwrap();

    let ready_snapshot = state.latest_metrics_snapshot();
    let ready = metrics_response(ready_snapshot.as_deref());
    assert!(ready.contains("codexbar_up 1\n"));
    assert!(ready.contains("codexbar_quota_session_used_ratio"));
    assert_eq!(build_count.load(Ordering::SeqCst), 1);
}
