//! Pins the JSON the usage bridge sends to the webview: keys, key order,
//! and null versus absent. The fixtures are the wire contract.

use super::openai_usage::{
    OpenAiApiDailyUsageSnapshot, OpenAiApiLineItemSnapshot, OpenAiApiModelUsageSnapshot,
    OpenAiApiUsageSnapshot,
};
use super::pace::{DailyTargetSnapshot, WindowPaceSnapshot};
use super::throughput::{
    ProviderThroughputSnapshot, ThroughputMinuteSnapshot, ThroughputModelSnapshot,
};
use super::*;
use codexbar::core::{IconLane, ProviderStateKind, WayfinderRouteSummary, WayfinderUsageSnapshot};

fn block() -> MonthlyLimitBlockSnapshot {
    MonthlyLimitBlockSnapshot {
        resets_at: Some("2026-12-01T00:00:00+00:00".to_string()),
    }
}

fn full_window(used_percent: f64) -> RateWindowSnapshot {
    RateWindowSnapshot {
        used_percent,
        remaining_percent: 100.0 - used_percent,
        window_minutes: Some(10_080),
        resets_at: Some("2026-11-30T12:00:00+00:00".to_string()),
        reset_description: Some("Resets Sunday".to_string()),
        is_exhausted: true,
        is_informational: true,
        description_is_detail: true,
        reset_regenerates: true,
        hidden_on_card: true,
        reserve_percent: Some(12.0),
        reserve_description: Some("12% in reserve".to_string()),
        reserve_will_last_to_reset: true,
        reserve_eta_seconds: Some(3600.5),
        pace: Some(WindowPaceSnapshot {
            stage: "slightly_behind".to_string(),
            delta_percent: 4.5,
            expected_used_percent: 40.0,
            actual_used_percent: 44.5,
            eta_seconds: Some(7200.0),
            will_last_to_reset: true,
            reset_window: true,
        }),
        daily_target: Some(DailyTargetSnapshot {
            amount: 3.25,
            unit: "credits".to_string(),
        }),
        monthly_limit_block: Some(block()),
    }
}

fn full_snapshot() -> ProviderUsageSnapshot {
    ProviderUsageSnapshot {
        tertiary_label: Some("monthly".to_string()),
        provider_id: "codex".to_string(),
        display_name: "Codex".to_string(),
        primary: full_window(25.0),
        primary_label: Some("Session".to_string()),
        secondary: Some(full_window(50.0)),
        secondary_label: Some("Weekly".to_string()),
        secondary_label_key: Some("weeklyLabel".to_string()),
        model_specific: Some(full_window(10.0)),
        tertiary: Some(full_window(75.0)),
        extra_rate_windows: vec![NamedRateWindowSnapshot {
            id: "extra-1".to_string(),
            title: "Extra".to_string(),
            window: full_window(5.0),
            fallback_lane: true,
            icon_fallback: Some(IconLane::Secondary),
        }],
        inventory: vec![ProviderInventoryItemSnapshot {
            id: "reset-credit".to_string(),
            title: "Reset credits".to_string(),
            available_count: 2,
            next_expires_at: Some("2026-12-15T00:00:00+00:00".to_string()),
        }],
        display_details: vec![ProviderDisplayDetailSnapshot {
            id: "detail-1".to_string(),
            section_title: Some("Details".to_string()),
            title: "Requests".to_string(),
            value: "120".to_string(),
            secondary_value: Some("of 500".to_string()),
            progress: Some(ProviderDisplayProgressSnapshot {
                used: 120.0,
                total: 500.0,
            }),
        }],
        cost: Some(CostSnapshotBridge {
            used: 12.5,
            limit: Some(100.0),
            remaining: Some(87.5),
            currency_code: "EUR".to_string(),
            currency_symbol: Some("€".to_string()),
            period: "month".to_string(),
            resets_at: Some("2026-12-01T00:00:00+00:00".to_string()),
            formatted_used: "€12.50".to_string(),
            formatted_limit: Some("€100.00".to_string()),
            balance: Some(42.0),
            balance_updated_at: Some("2026-11-27T08:00:00+00:00".to_string()),
            account_id: Some("acct_1".to_string()),
            formatted_balance: Some("€42.00".to_string()),
            daily: vec![CostDailyPointBridge {
                day: "2026-11-26".to_string(),
                amount: 1.5,
            }],
            always_visible: true,
            folded_into_primary: true,
            pay_as_you_go: true,
            title_key: Some("costTitle".to_string()),
        }),
        plan_name: Some("Pro".to_string()),
        account_email: Some("user@example.com".to_string()),
        account_label: Some("Work".to_string()),
        subscription: Some(SubscriptionMetadataSnapshot {
            starts_at: Some("2026-01-01T00:00:00+00:00".to_string()),
            expires_at: Some("2027-01-01T00:00:00+00:00".to_string()),
            renews_at: Some("2027-01-01T00:00:00+00:00".to_string()),
            renews_at_is_date_only: true,
            expires_at_is_date_only: true,
        }),
        source_label: "oauth".to_string(),
        has_successful_claude_cli_quota: true,
        updated_at: "2026-11-27T08:30:00+00:00".to_string(),
        error: Some("Rate limited".to_string()),
        error_state: ProviderStateKind::ExpiredSession,
        pace: Some(PaceSnapshot {
            stage: "far_ahead".to_string(),
            delta_percent: -20.0,
            will_last_to_reset: true,
            eta_seconds: Some(5400.0),
            expected_used_percent: 60.0,
            actual_used_percent: 40.0,
            monthly_limit_block: Some(block()),
        }),
        pace_authoritative: false,
        quota_estimated: true,
        account_organization: Some("Acme".to_string()),
        tray_status_label: Some("25% used".to_string()),
        fetch_duration_ms: Some(1234),
        wayfinder_usage: Some(WayfinderUsageSnapshot {
            gateway_status: "online".to_string(),
            offline: true,
            dry_run: true,
            missing_keys: vec!["OPENAI_API_KEY".to_string()],
            model_count: 3,
            models: vec!["gpt-5".to_string()],
            requests: 100,
            estimated_requests: 120,
            tokens: 50_000,
            realized: 1.5,
            baseline: 3.0,
            saved: 1.5,
            saved_percent: 50.0,
            period_days: 30,
            unit: "usd".to_string(),
            priced: true,
            routes: vec![WayfinderRouteSummary {
                name: "fast".to_string(),
                requests: 60,
                tokens: 30_000,
                realized: 0.5,
                baseline: 1.0,
                saved: 0.5,
            }],
            avg_decision_ms: Some(2.5),
        }),
        open_ai_api_usage: Some(OpenAiApiUsageSnapshot {
            history_days: 30,
            project_id: Some("proj_abc".to_string()),
            daily: vec![OpenAiApiDailyUsageSnapshot {
                start_time: 1_700_000_000,
                end_time: 1_700_086_400,
                cost_usd: 14.75,
                requests: 10,
                input_tokens: 1300,
                cached_input_tokens: 250,
                output_tokens: 700,
                total_tokens: 2000,
                line_items: vec![OpenAiApiLineItemSnapshot {
                    name: "Text tokens".to_string(),
                    cost_usd: 12.5,
                }],
                models: vec![OpenAiApiModelUsageSnapshot {
                    name: "gpt-5.2".to_string(),
                    requests: 7,
                    input_tokens: 1000,
                    cached_input_tokens: 250,
                    output_tokens: 500,
                    total_tokens: 1500,
                }],
            }],
        }),
        throughput: Some(ProviderThroughputSnapshot {
            minutes: vec![ThroughputMinuteSnapshot {
                start: 1_700_000_000,
                total_tokens: 900,
                requests: 3,
                is_pending: true,
            }],
            frontier: Some(1_700_000_060),
            latest_tokens_per_minute: Some(800),
            peak_tokens_per_minute: 1200,
            peak_at: Some(1_700_000_120),
            five_minute_average_tokens_per_minute: 650.5,
            models: vec![ThroughputModelSnapshot {
                name: "gpt-5.2".to_string(),
                latest_tpm: 700,
                peak_tpm: 1100,
                total_tokens: 4000,
                peak_limit_counted_tokens: 1000,
                limit_counted_tokens_by_minute: vec![100, 200],
            }],
            window_minutes: 60,
            fetched_at: 1_700_003_600,
            project_id: Some("proj_abc".to_string()),
        }),
        session_equivalent_forecast: Some(SessionEquivalentForecastSnapshot {
            estimated_windows_to_exhaust_weekly: 6.5,
            windows_until_reset: 4,
            available_windows_until_reset: 3.5,
            sample_count: 12,
            weekly_resets_at: "2026-11-30T12:00:00+00:00".to_string(),
            weekly_used_percent: 50.0,
        }),
        quota_burndown: Some(QuotaBurndownSnapshot {
            series: "weekly".to_string(),
            window_minutes: 10_080,
            start: "2026-11-23T12:00:00+00:00".to_string(),
            reset: "2026-11-30T12:00:00+00:00".to_string(),
            samples: vec![QuotaBurndownPointSnapshot {
                captured_at: "2026-11-25T12:00:00+00:00".to_string(),
                remaining_percent: 70.0,
            }],
            ideal: [
                QuotaBurndownPointSnapshot {
                    captured_at: "2026-11-23T12:00:00+00:00".to_string(),
                    remaining_percent: 100.0,
                },
                QuotaBurndownPointSnapshot {
                    captured_at: "2026-11-30T12:00:00+00:00".to_string(),
                    remaining_percent: 0.0,
                },
            ],
        }),
    }
}

fn assert_wire(value: &impl Serialize, fixture: &str) {
    let actual = serde_json::to_string_pretty(value).expect("snapshot serializes") + "\n";
    assert_eq!(actual, fixture);
}

#[test]
fn default_usage_snapshot_wire_shape() {
    assert_wire(
        &ProviderUsageSnapshot::default(),
        include_str!("fixtures/usage_snapshot_default.json"),
    );
}

#[test]
fn full_usage_snapshot_wire_shape() {
    assert_wire(
        &full_snapshot(),
        include_str!("fixtures/usage_snapshot_full.json"),
    );
}

#[test]
fn default_usage_presentation_wire_shape() {
    let presentation = ProviderUsagePresentationSnapshot {
        snapshot: ProviderUsageSnapshot::default(),
        selected_metric: RateWindowSnapshot::default(),
        hidden_usage_item_ids: Vec::new(),
    };
    assert_wire(
        &presentation,
        include_str!("fixtures/usage_presentation_default.json"),
    );
}

#[test]
fn full_usage_presentation_wire_shape() {
    let presentation = ProviderUsagePresentationSnapshot {
        snapshot: full_snapshot(),
        selected_metric: full_window(60.0),
        hidden_usage_item_ids: vec!["extra-1".to_string()],
    };
    assert_wire(
        &presentation,
        include_str!("fixtures/usage_presentation_full.json"),
    );
}
