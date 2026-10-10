use super::*;

pub(super) fn build_usage_spend_summary(
    cached: &[ProviderUsageSnapshot],
    period: CostReportingPeriod,
    settings: &codexbar::settings::Settings,
    force_refresh: bool,
) -> UsageSpendSummary {
    let now = Utc::now();
    let period_scan_days = period.scan_days(now);
    let include_opencodex = settings.open_codex_usage_logs_enabled;
    let hide_native = settings.hide_native_codex_cost_when_open_codex_present;
    let pi_selected = settings.enabled_providers.iter().any(|id| id == "pi")
        || cached.iter().any(|snapshot| snapshot.provider_id == "pi");
    let include_pi_in_native = !pi_selected;

    // Upstream 0.55.0 #3105: independent provider baselines load in parallel.
    // Keep each provider's 7d/30d scans serial so they can safely share that
    // provider's incremental cache, while Codex and Claude run concurrently.
    let codex_scan_options = if force_refresh {
        codexbar::core::CostScanOptions::app_driven()
    } else {
        codexbar::core::CostScanOptions::default()
    };
    let mut codex_scan_options = codex_scan_options;
    codex_scan_options.include_pi_sessions = include_pi_in_native;
    // Any period other than 7d/30d costs one extra scan per provider.
    let (
        (codex_7_summary, codex_30_summary, codex_period_summary),
        (claude_7_summary, claude_30_summary, claude_period_summary),
        (pi_7_summary, pi_30_summary, pi_period_summary),
    ) = std::thread::scope(|scope| {
        let codex = scope.spawn(move || {
            let seven = CostScanner::new(7)
                .with_options(codex_scan_options)
                .scan_codex();
            let thirty = CostScanner::new(30)
                .with_options(codex_scan_options)
                .scan_codex();
            let selected = selected_period_scan(period, &seven, &thirty, || {
                CostScanner::for_period(period)
                    .with_options(codex_scan_options)
                    .scan_codex()
            });
            (seven, thirty, selected)
        });
        let claude = scope.spawn(|| {
            let seven = CostScanner::new(7)
                .scan_claude_with_cancel_and_pi_sessions(None, include_pi_in_native);
            let thirty = CostScanner::new(30)
                .scan_claude_with_cancel_and_pi_sessions(None, include_pi_in_native);
            let selected = selected_period_scan(period, &seven, &thirty, || {
                CostScanner::for_period(period)
                    .scan_claude_with_cancel_and_pi_sessions(None, include_pi_in_native)
            });
            (seven, thirty, selected)
        });
        let pi = scope.spawn(|| {
            let seven = CostScanner::new(7).scan_pi();
            let thirty = CostScanner::new(30).scan_pi();
            let selected = selected_period_scan(period, &seven, &thirty, || {
                CostScanner::for_period(period).scan_pi()
            });
            (seven, thirty, selected)
        });
        (
            codex.join().expect("Codex spend scan worker panicked"),
            claude.join().expect("Claude spend scan worker panicked"),
            pi.join().expect("Pi spend scan worker panicked"),
        )
    });

    let codex_stale = !codex_30_summary.history_coverage_established;
    let codex_stale_updated_at = codex_stale
        .then(|| {
            codexbar::core::JsonlScanner::load_cache_status(codexbar::core::ProviderId::Codex, None)
                .previous_report
                .and_then(|report| report.updated_at)
        })
        .flatten();

    let codex_7_contract = build_local_spend_contract_from_summary(
        "codex",
        7,
        include_opencodex,
        hide_native,
        settings.hide_personal_info,
        codex_7_summary.clone(),
    );
    let codex_30_contract = build_local_spend_contract_from_summary(
        "codex",
        30,
        include_opencodex,
        hide_native,
        settings.hide_personal_info,
        codex_30_summary.clone(),
    );
    let codex_period_contract = build_contract_from_period_summary(
        "codex",
        period,
        include_opencodex,
        hide_native,
        settings.hide_personal_info,
        codex_period_summary,
    );
    let pi_period_contract = build_contract_from_period_summary(
        "pi",
        period,
        false,
        false,
        settings.hide_personal_info,
        pi_period_summary,
    );
    let pi_7_contract = build_local_spend_contract_from_summary(
        "pi",
        7,
        false,
        false,
        settings.hide_personal_info,
        pi_7_summary.clone(),
    );
    let pi_30_contract = build_local_spend_contract_from_summary(
        "pi",
        30,
        false,
        false,
        settings.hide_personal_info,
        pi_30_summary.clone(),
    );

    let mut provider_ids: BTreeSet<String> = settings.enabled_providers.iter().cloned().collect();
    provider_ids.extend(cached.iter().map(|snapshot| snapshot.provider_id.clone()));
    if include_opencodex {
        // OpenCodex is an enrichment source, never a standalone provider row.
        // Publish routed subscriptions even when no live provider snapshot exists.
        for id in ["codex", "opencodego", "kimi", "deepseek", "nous"] {
            let contract = match id {
                "codex" => None,
                _ => Some(build_local_spend_contract(id, 30, true)),
            };
            if contract
                .as_ref()
                .is_some_and(|contract| !contract.imports.is_empty())
            {
                provider_ids.insert(id.to_string());
            }
        }
    }

    let cached_by_id: HashMap<&str, &ProviderUsageSnapshot> = cached
        .iter()
        .map(|snapshot| (snapshot.provider_id.as_str(), snapshot))
        .collect();

    let mut rows = Vec::new();
    for provider_id in provider_ids {
        let cached_snapshot = cached_by_id.get(provider_id.as_str()).copied();
        let display_name = cached_snapshot
            .map(|snapshot| snapshot.display_name.trim())
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .or_else(|| {
                codexbar::core::ProviderId::from_cli_name(&provider_id).map(|id| {
                    codexbar::core::instantiate_provider(id)
                        .metadata()
                        .display_name
                        .to_string()
                })
            })
            .unwrap_or_else(|| provider_id.clone());

        let mut local_cost_estimates = None;
        let mut token_lower_bounds = (false, false);
        let spend = match provider_id.as_str() {
            "codex" => SpendValues {
                seven_day: codex_7_contract.known_cost_usd,
                thirty_day: codex_30_contract.known_cost_usd,
                seven_day_tokens: codex_7_contract.token_total,
                thirty_day_tokens: codex_30_contract.token_total,
                period_cost: codex_period_contract.known_cost_usd,
                period_tokens: codex_period_contract.token_total,
                source: if include_opencodex && !codex_30_contract.imports.is_empty() {
                    "local logs + OpenCodex".to_string()
                } else {
                    "local logs".to_string()
                },
                refreshing: codex_stale,
                stale_updated_at: codex_stale_updated_at.clone(),
            },
            "claude" => SpendValues {
                seven_day: Some(claude_7_summary.total_cost_usd),
                thirty_day: Some(claude_30_summary.total_cost_usd),
                seven_day_tokens: Some(claude_7_summary.total_tokens_for_provider("claude")),
                thirty_day_tokens: Some(claude_30_summary.total_tokens_for_provider("claude")),
                period_cost: Some(claude_period_summary.total_cost_usd),
                period_tokens: Some(claude_period_summary.total_tokens_for_provider("claude")),
                source: "local logs".to_string(),
                refreshing: false,
                stale_updated_at: None,
            },
            "pi" => SpendValues {
                seven_day: pi_7_contract.known_cost_usd,
                thirty_day: pi_30_contract.known_cost_usd,
                seven_day_tokens: pi_7_contract.token_total,
                thirty_day_tokens: pi_30_contract.token_total,
                period_cost: pi_period_contract.known_cost_usd,
                period_tokens: pi_period_contract.token_total,
                source: "local Pi/OMP history".to_string(),
                refreshing: !pi_30_summary.history_coverage_established,
                stale_updated_at: None,
            },
            "opencodego" | "kimi" | "deepseek" | "nous" if include_opencodex => {
                let seven = build_local_spend_contract(&provider_id, 7, true);
                let thirty = build_local_spend_contract(&provider_id, 30, true);
                if !thirty.imports.is_empty() {
                    let selected =
                        build_local_spend_contract_for_period(&provider_id, period, true);
                    SpendValues {
                        seven_day: seven.known_cost_usd,
                        thirty_day: thirty.known_cost_usd,
                        seven_day_tokens: seven.token_total,
                        thirty_day_tokens: thirty.token_total,
                        period_cost: selected.known_cost_usd,
                        period_tokens: selected.token_total,
                        source: if provider_id == "opencodego" {
                            "local logs + OpenCodex".to_string()
                        } else {
                            "OpenCodex".to_string()
                        },
                        refreshing: false,
                        stale_updated_at: None,
                    }
                } else {
                    cached_spend(cached_snapshot, period, now)
                }
            }
            "opencodego" => {
                // Cost stays the provider's; recorded local tokens fill the token columns.
                let mut spend = cached_spend(cached_snapshot, period, now);
                let seven = CostScanner::new(7).scan_opencodego_usage_tokens_with_cancel(None);
                let thirty = CostScanner::new(30).scan_opencodego_usage_tokens_with_cancel(None);
                spend.period_tokens = selected_period_scan(period, &seven, &thirty, || {
                    CostScanner::for_period(period).scan_opencodego_usage_tokens_with_cancel(None)
                });
                spend.seven_day_tokens = seven;
                spend.thirty_day_tokens = thirty;
                spend
            }
            "cursor" => {
                let seven = codexbar::providers::cursor::local_csv::summarize(7);
                let thirty = codexbar::providers::cursor::local_csv::summarize(30);
                if thirty.row_count > 0 {
                    let selected =
                        codexbar::providers::cursor::local_csv::summarize(period_scan_days);
                    SpendValues {
                        seven_day: (seven.row_count > 0).then_some(seven.total_cost_usd),
                        thirty_day: Some(thirty.total_cost_usd),
                        seven_day_tokens: (seven.row_count > 0).then_some(seven.total_tokens),
                        thirty_day_tokens: Some(thirty.total_tokens),
                        period_cost: (selected.row_count > 0).then_some(selected.total_cost_usd),
                        period_tokens: (selected.row_count > 0).then_some(selected.total_tokens),
                        source: "local Cursor tokscale cache".to_string(),
                        refreshing: false,
                        stale_updated_at: None,
                    }
                } else {
                    cached_spend(cached_snapshot, period, now)
                }
            }
            "grok" => {
                let seven = codexbar::providers::grok::local_sessions::summarize(7);
                let thirty = codexbar::providers::grok::local_sessions::summarize(30);
                let selected =
                    codexbar::providers::grok::local_sessions::summarize(period_scan_days);
                let mut spend = cached_spend(cached_snapshot, period, now);
                spend.seven_day_tokens = (seven.session_count > 0).then_some(seven.total_tokens);
                spend.thirty_day_tokens = (thirty.session_count > 0).then_some(thirty.total_tokens);
                spend.period_tokens = (selected.session_count > 0).then_some(selected.total_tokens);
                if thirty.session_count > 0 {
                    spend.source = "local Grok sessions".to_string();
                }
                spend
            }
            "antigravity" => {
                use codexbar::providers::antigravity::local_sessions;
                let seven = local_sessions::summarize(7);
                let thirty = local_sessions::summarize(30);
                let selected = local_sessions::summarize(period_scan_days);
                let mut spend = cached_spend(cached_snapshot, period, now);
                // Upstream 0.64: the app refreshes unknown-model pricing in the
                // background; a later read (provider refresh or Refresh) reprices.
                if let Some(refresh) = local_sessions::background_pricing_refresh(&thirty) {
                    tauri::async_runtime::spawn(refresh);
                }
                {
                    use codexbar::providers::antigravity::local_sessions::LocalHistoryCoverage;
                    spend.seven_day_tokens =
                        matches!(seven.coverage, LocalHistoryCoverage::Complete)
                            .then_some(seven.total_tokens);
                    spend.thirty_day_tokens =
                        matches!(thirty.coverage, LocalHistoryCoverage::Complete)
                            .then_some(thirty.total_tokens);
                    spend.period_tokens =
                        matches!(selected.coverage, LocalHistoryCoverage::Complete)
                            .then_some(selected.total_tokens);
                    if matches!(thirty.coverage, LocalHistoryCoverage::Complete) {
                        spend.source = "local Antigravity history".to_string();
                    }
                }
                let spend = antigravity_spend_values(spend, &seven, &thirty);
                token_lower_bounds = (seven.lower_bound, thirty.lower_bound);
                local_cost_estimates = Some((seven.cost_estimate, thirty.cost_estimate));
                spend
            }
            _ => cached_spend(cached_snapshot, period, now),
        };

        let currency = cached_snapshot
            .and_then(|snapshot| snapshot.cost.as_ref())
            .map(|cost| cost.currency_code.clone())
            .unwrap_or_else(|| "USD".to_string());
        let daily = cached_snapshot
            .and_then(|snapshot| snapshot.cost.as_ref())
            .map(|cost| {
                cost.daily
                    .iter()
                    .map(|point| UsageSpendDailyPoint {
                        day: point.day.clone(),
                        amount: point.amount,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let (seven_day_estimate, thirty_day_estimate) = local_cost_estimates
            .map(|(seven, thirty)| (Some(seven), Some(thirty)))
            .unwrap_or((None, None));
        rows.push(UsageSpendRow {
            provider_id: provider_id.clone(),
            display_name,
            seven_day: spend.seven_day,
            thirty_day: spend.thirty_day,
            seven_day_estimate,
            thirty_day_estimate,
            seven_day_tokens: spend.seven_day_tokens,
            thirty_day_tokens: spend.thirty_day_tokens,
            seven_day_tokens_lower_bound: token_lower_bounds.0,
            thirty_day_tokens_lower_bound: token_lower_bounds.1,
            period_cost: spend.period_cost,
            period_tokens: spend.period_tokens,
            currency,
            source: spend.source,
            included_in_overview: include_in_shared_overview(
                &provider_id,
                settings.enabled_providers.contains(&provider_id),
                cached_snapshot.is_some(),
            ),
            daily,
            refreshing: spend.refreshing,
            stale_updated_at: spend.stale_updated_at,
        });
    }

    let contract = codex_period_contract;
    let reporting_day = last_included_reporting_day(&contract);
    let dashboard_timezone = codexbar::core::local_timezone_name();
    UsageSpendSummary {
        rows,
        contract,
        reporting_period: period.raw(),
        reporting_day,
        dashboard_timezone,
    }
}

/// Reuse the fixed scan when it already covers the selected period.
pub(super) fn selected_period_scan<T: Clone>(
    period: CostReportingPeriod,
    seven: &T,
    thirty: &T,
    scan: impl FnOnce() -> T,
) -> T {
    match period {
        CostReportingPeriod::Rolling(7) => seven.clone(),
        CostReportingPeriod::Rolling(30) => thirty.clone(),
        _ => scan(),
    }
}

/// Pi is an alternate local-history view over rows that may already be
/// projected into Codex or Claude. Keep it out of the shared denominator so
/// enabling Pi cannot double-count the same physical usage.
pub(super) fn include_in_shared_overview(provider_id: &str, enabled: bool, cached: bool) -> bool {
    provider_id != "pi" && (enabled || cached)
}

fn last_included_reporting_day(contract: &SpendContract) -> String {
    contract
        .daily
        .iter()
        .filter_map(|point| chrono::NaiveDate::parse_from_str(&point.day, "%Y-%m-%d").ok())
        .max()
        .unwrap_or_else(|| chrono::Local::now().date_naive())
        .format("%Y-%m-%d")
        .to_string()
}

pub(super) fn antigravity_spend_values(
    mut spend: SpendValues,
    seven: &codexbar::spend_contract::LocalTokenHistorySummary,
    thirty: &codexbar::spend_contract::LocalTokenHistorySummary,
) -> SpendValues {
    use codexbar::spend_contract::LocalHistoryCoverage;

    spend.seven_day = seven.total_usd();
    spend.thirty_day = thirty.total_usd();
    // Exact for a complete scan, a floor for a lower bound, unknown otherwise.
    spend.seven_day_tokens = seven.published_tokens();
    spend.thirty_day_tokens = thirty.published_tokens();
    if spend.thirty_day.is_some() {
        spend.source = "local Antigravity history · API list-price estimate".to_string();
    } else if thirty.cost_estimate.known_subtotal_usd.is_some() {
        spend.source = "local Antigravity history · known API list-price subtotal".to_string();
    } else if thirty.coverage == LocalHistoryCoverage::Complete {
        spend.source = "local Antigravity history · unpriced".to_string();
    }
    spend
}

/// Provider-reported daily costs bucket by UTC day, so the selected period is
/// resolved in UTC here. `None` when no day falls inside the window.
pub(super) fn period_cost_from_daily(
    daily: &[crate::commands::bridge::CostDailyPointBridge],
    period: CostReportingPeriod,
    now: chrono::DateTime<Utc>,
) -> Option<f64> {
    let earliest = daily
        .iter()
        .filter_map(|point| chrono::NaiveDate::parse_from_str(&point.day, "%Y-%m-%d").ok())
        .min();
    let bounds = period.bounds(now, CostTimeZone::UTC, earliest);
    let mut total = 0.0;
    let mut saw = false;
    for point in daily {
        if bounds.contains_day_key(&point.day) {
            total += point.amount;
            saw = true;
        }
    }
    saw.then_some(total)
}

pub(super) fn cached_spend(
    snapshot: Option<&ProviderUsageSnapshot>,
    reporting_period: CostReportingPeriod,
    now: chrono::DateTime<Utc>,
) -> SpendValues {
    let Some(snapshot) = snapshot else {
        return SpendValues {
            seven_day: None,
            thirty_day: None,
            seven_day_tokens: None,
            thirty_day_tokens: None,
            period_cost: None,
            period_tokens: None,
            source: "unavailable".to_string(),
            refreshing: false,
            stale_updated_at: None,
        };
    };
    let Some(cost) = snapshot.cost.as_ref() else {
        return SpendValues {
            seven_day: None,
            thirty_day: None,
            seven_day_tokens: None,
            thirty_day_tokens: None,
            period_cost: None,
            period_tokens: None,
            source: if snapshot.error.is_some() {
                "unavailable".to_string()
            } else {
                snapshot.source_label.clone()
            },
            refreshing: false,
            stale_updated_at: None,
        };
    };
    let period = cost.period.trim();
    let period_lower = period.to_ascii_lowercase();
    let (seven_day, thirty_day) = if cost.daily.is_empty() {
        (
            None,
            (period_lower.contains("30 day") || period_lower.contains("30-day"))
                .then_some(cost.used),
        )
    } else {
        let today = chrono::Utc::now().date_naive();
        let seven_cutoff = today - chrono::Duration::days(6);
        let thirty_cutoff = today - chrono::Duration::days(29);
        let mut seven = 0.0;
        let mut thirty = 0.0;
        let mut saw_seven = false;
        let mut saw_thirty = false;
        for point in &cost.daily {
            let Ok(day) = chrono::NaiveDate::parse_from_str(&point.day, "%Y-%m-%d") else {
                continue;
            };
            // Providers with long daily history (Bedrock keeps 14 months) must
            // not widen the fixed 30-day column.
            if day > today || day < thirty_cutoff {
                continue;
            }
            thirty += point.amount;
            saw_thirty = true;
            if day >= seven_cutoff {
                seven += point.amount;
                saw_seven = true;
            }
        }
        (saw_seven.then_some(seven), saw_thirty.then_some(thirty))
    };
    let period_cost = if cost.daily.is_empty() {
        // A provider-reported 30-day total only answers a 30-day selection.
        (reporting_period == CostReportingPeriod::Rolling(30))
            .then_some(thirty_day)
            .flatten()
    } else {
        period_cost_from_daily(&cost.daily, reporting_period, now)
    };
    SpendValues {
        seven_day,
        thirty_day,
        seven_day_tokens: None,
        thirty_day_tokens: None,
        period_cost,
        period_tokens: None,
        source: if period.is_empty() {
            snapshot.source_label.clone()
        } else {
            format!("period ({period})")
        },
        refreshing: false,
        stale_updated_at: None,
    }
}
