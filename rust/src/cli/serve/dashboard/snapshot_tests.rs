#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::core::NamedRateWindow;
    use crate::core::{CostSnapshot, RateWindow};

    fn fetch_result(used: f64, email: Option<&str>, plan: Option<&str>) -> ProviderFetchResult {
        let mut usage = UsageSnapshot::new(RateWindow::new(used));
        usage.account_email = email.map(str::to_string);
        usage.login_method = plan.map(str::to_string);
        ProviderFetchResult::new(usage, "oauth")
    }

    fn provider_envelope(fetch: Result<ProviderFetchResult, String>) -> ProviderFetchEnvelope {
        ProviderFetchEnvelope {
            id: "claude".to_string(),
            display_name: "Claude".to_string(),
            session_label: "Session".to_string(),
            weekly_label: "Weekly".to_string(),
            fetch,
        }
    }

    fn input(providers: Vec<ProviderFetchEnvelope>, identity: DashboardIdentity) -> SnapshotInput {
        SnapshotInput {
            providers,
            costs: HashMap::new(),
            claude_accounts: None,
            identity,
            generated_at: DateTime::parse_from_rfc3339("2026-08-08T01:02:03Z")
                .unwrap()
                .with_timezone(&Utc),
            refresh_seconds: 60,
            version: Some("0.48.0-test".to_string()),
            order: vec!["claude".to_string(), "codex".to_string()],
            enabled: BTreeSet::from(["claude".to_string()]),
        }
    }

    #[test]
    fn snapshot_envelope_shape() {
        let payload = build_snapshot(&input(
            vec![provider_envelope(Ok(fetch_result(
                42.0,
                Some("me@example.com"),
                None,
            )))],
            DashboardIdentity::Redacted,
        ));
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["generatedAt"], "2026-08-08T01:02:03Z");
        assert_eq!(json["staleAfterSeconds"], 180);
        assert_eq!(json["host"]["codexBarVersion"], "0.48.0-test");
        assert_eq!(json["host"]["refreshIntervalSeconds"], 60);
        let row = &json["providers"][0];
        assert_eq!(row["id"], "claude");
        assert_eq!(row["name"], "Claude");
        assert_eq!(row["enabled"], true);
        assert_eq!(row["source"], "oauth");
        assert!(
            row["status"].is_null(),
            "no status pipeline in v1 (parity #2723)"
        );
        assert_eq!(row["identity"]["accountEmail"], "redacted@example.com");
        assert_eq!(row["windows"][0]["kind"], "session");
        assert_eq!(row["windows"][0]["usedPercent"], 42.0);
        assert_eq!(row["windows"][0]["remainingPercent"], 58.0);
        assert!(
            row["credits"].is_null(),
            "no credits pipeline (documented divergence)"
        );
        assert!(row["cost"].is_null());
        assert!(row["error"].is_null());
        assert_eq!(row["display"]["accentColor"], "#6E6E6E");
        assert_eq!(row["display"]["sortKey"], 0);
        assert!(
            row.get("accounts").is_none(),
            "accounts absent without input"
        );
        assert!(row.get("accountsError").is_none());
    }

    #[test]
    fn identity_full_exposes_email() {
        let payload = build_snapshot(&input(
            vec![provider_envelope(Ok(fetch_result(
                1.0,
                Some("me@example.com"),
                Some("Claude Max"),
            )))],
            DashboardIdentity::Full,
        ));
        let row = &serde_json::to_value(&payload).unwrap()["providers"][0];
        assert_eq!(row["identity"]["accountEmail"], "me@example.com");
        assert_eq!(row["identity"]["plan"], "Claude Max");
    }

    #[test]
    fn redaction_handles_missing_at_and_empty() {
        assert_eq!(
            dashboard_email(Some("nobody"), DashboardIdentity::Redacted).as_deref(),
            Some("redacted")
        );
        assert_eq!(
            dashboard_email(Some("  "), DashboardIdentity::Redacted),
            None
        );
        assert_eq!(dashboard_email(None, DashboardIdentity::Full), None);
    }

    #[test]
    fn error_row_uses_provider_error_payload() {
        let payload = build_snapshot(&input(
            vec![provider_envelope(Err("network down".to_string()))],
            DashboardIdentity::Redacted,
        ));
        let row = &serde_json::to_value(&payload).unwrap()["providers"][0];
        assert_eq!(row["error"]["code"], 1);
        assert_eq!(row["error"]["message"], "network down");
        assert_eq!(row["error"]["kind"], "provider");
        assert_eq!(row["source"], "unknown");
        assert_eq!(row["updatedAt"], "2026-08-08T01:02:03Z");
        assert_eq!(row["windows"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn sort_key_falls_back_to_position() {
        let mut other = provider_envelope(Ok(fetch_result(3.0, None, None)));
        other.id = "unknownprovider".to_string();
        let payload = build_snapshot(&input(vec![other], DashboardIdentity::Redacted));
        let row = &serde_json::to_value(&payload).unwrap()["providers"][0];
        assert_eq!(row["display"]["sortKey"], 10_000);
        assert_eq!(row["enabled"], true, "unknown ids stay enabled");
    }

    #[test]
    fn window_kinds_cover_secondary_tertiary_model_extras() {
        let mut usage = UsageSnapshot::new(RateWindow::new(10.0));
        usage.secondary = Some(RateWindow::new(20.0));
        usage.model_specific = Some(RateWindow::new(30.0));
        usage.tertiary = Some(RateWindow::new(40.0));
        usage
            .extra_rate_windows
            .push(crate::core::NamedRateWindow::new(
                "reset-credits",
                "Reset credits",
                RateWindow::new(0.0),
            ));
        let payload = build_snapshot(&input(
            vec![provider_envelope(Ok(ProviderFetchResult::new(
                usage, "cli",
            )))],
            DashboardIdentity::Redacted,
        ));
        let windows = &serde_json::to_value(&payload).unwrap()["providers"][0]["windows"];
        let kinds: Vec<&str> = windows
            .as_array()
            .unwrap()
            .iter()
            .map(|w| w["kind"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            ["session", "weekly", "model", "tertiary", "reset-credits"]
        );
    }

    fn antigravity_envelope(windows: Vec<NamedRateWindow>) -> ProviderFetchEnvelope {
        let primary = windows
            .first()
            .map(|window| window.window.clone())
            .unwrap_or_else(|| RateWindow::new(0.0));
        let primary_label = windows.first().map(|window| window.title.clone());
        let mut usage = UsageSnapshot::new(primary);
        usage.primary_label = primary_label;
        usage.extra_rate_windows = windows.into_iter().skip(1).collect();
        ProviderFetchEnvelope {
            id: "antigravity".to_string(),
            display_name: "Antigravity".to_string(),
            session_label: "Session".to_string(),
            weekly_label: "Weekly".to_string(),
            fetch: Ok(ProviderFetchResult::new(usage, "local")),
        }
    }

    #[test]
    fn antigravity_dashboard_marks_only_known_idle_family() {
        let windows = vec![
            NamedRateWindow::new("model-gemini-pro", "Gemini Pro", RateWindow::new(20.0)),
            NamedRateWindow::new("model-gemini-flash", "Gemini Flash", RateWindow::new(0.0)),
            NamedRateWindow::new("model-claude", "Claude Sonnet", RateWindow::new(0.0)),
            NamedRateWindow::new("model-gpt", "GPT", RateWindow::new(0.0)),
        ];
        let json = serde_json::to_value(build_snapshot(&input(
            vec![antigravity_envelope(windows)],
            DashboardIdentity::Redacted,
        )))
        .unwrap();
        let windows = json["providers"][0]["windows"].as_array().unwrap();
        assert_eq!(
            windows.len(),
            4,
            "representative session rows must not duplicate buckets"
        );
        let by_label: HashMap<_, _> = windows
            .iter()
            .map(|window| (window["label"].as_str().unwrap(), window))
            .collect();
        assert!(by_label["Gemini Pro"].get("idle").is_none());
        assert!(by_label["Gemini Flash"].get("idle").is_none());
        assert_eq!(by_label["Claude Sonnet"]["idle"], true);
        assert_eq!(by_label["GPT"]["idle"], true);
    }

    #[test]
    fn antigravity_dashboard_keeps_selected_core_buckets_with_extras() {
        let mut usage = UsageSnapshot::new(RateWindow::with_details(10.0, Some(300), None, None))
            .with_primary_label("Gemini 5h")
            .with_secondary(RateWindow::with_details(90.0, Some(10_080), None, None))
            .with_secondary_label("Gemini Weekly");
        usage.extra_rate_windows.push(NamedRateWindow::new(
            "antigravity-quota-summary-3p-weekly",
            "Claude/GPT weekly",
            RateWindow::with_details(20.0, Some(10_080), None, None),
        ));

        let json = serde_json::to_value(build_snapshot(&input(
            vec![ProviderFetchEnvelope {
                id: "antigravity".to_string(),
                display_name: "Antigravity".to_string(),
                session_label: "Claude".to_string(),
                weekly_label: "Gemini Pro".to_string(),
                fetch: Ok(ProviderFetchResult::new(usage, "local")),
            }],
            DashboardIdentity::Redacted,
        )))
        .unwrap();
        let windows = json["providers"][0]["windows"].as_array().unwrap();
        let labels: Vec<&str> = windows
            .iter()
            .map(|window| window["label"].as_str().unwrap())
            .collect();
        assert_eq!(
            labels,
            vec!["Gemini 5h", "Gemini Weekly", "Claude/GPT weekly"]
        );
    }

    #[test]
    fn antigravity_dashboard_keeps_equal_reading_core_and_extra_buckets() {
        let primary = RateWindow::with_details(20.0, Some(300), None, None);
        let mut usage = UsageSnapshot::new(primary.clone()).with_primary_label("Gemini 5h");
        usage.extra_rate_windows.push(NamedRateWindow::new(
            "antigravity-quota-summary-3p-session",
            "Claude/GPT 5h",
            primary,
        ));
        let result = ProviderFetchResult::new(usage, "local");

        let json = serde_json::to_value(build_snapshot(&input(
            vec![ProviderFetchEnvelope {
                id: "antigravity".to_string(),
                display_name: "Antigravity".to_string(),
                session_label: "Session".to_string(),
                weekly_label: "Weekly".to_string(),
                fetch: Ok(result),
            }],
            DashboardIdentity::Redacted,
        )))
        .unwrap();
        let windows = json["providers"][0]["windows"].as_array().unwrap();
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0]["label"], "Gemini 5h");
        assert_eq!(windows[1]["label"], "Claude/GPT 5h");
    }

    #[test]
    fn dashboard_uses_provider_resolved_labels_without_extra_buckets() {
        let usage = UsageSnapshot::new(RateWindow::new(20.0))
            .with_primary_label("Gemini 5h")
            .with_secondary(RateWindow::new(30.0))
            .with_secondary_label("Gemini Weekly");
        let json = serde_json::to_value(build_snapshot(&input(
            vec![ProviderFetchEnvelope {
                id: "antigravity".to_string(),
                display_name: "Antigravity".to_string(),
                session_label: "Session".to_string(),
                weekly_label: "Weekly".to_string(),
                fetch: Ok(ProviderFetchResult::new(usage, "local")),
            }],
            DashboardIdentity::Redacted,
        )))
        .unwrap();
        let windows = json["providers"][0]["windows"].as_array().unwrap();
        let labels: Vec<&str> = windows
            .iter()
            .map(|window| window["label"].as_str().unwrap())
            .collect();
        assert_eq!(labels, vec!["Gemini 5h", "Gemini Weekly"]);
    }

    #[test]
    fn antigravity_dashboard_keeps_unknown_zero_family_visible() {
        let windows = vec![
            NamedRateWindow::new("model-gemini", "Gemini Pro", RateWindow::new(20.0)),
            NamedRateWindow::new("model-claude", "Claude Sonnet", RateWindow::new(0.0))
                .with_usage_known(false),
            NamedRateWindow::new("model-gpt", "GPT", RateWindow::new(0.0)),
        ];
        let json = serde_json::to_value(build_snapshot(&input(
            vec![antigravity_envelope(windows)],
            DashboardIdentity::Redacted,
        )))
        .unwrap();
        let windows = json["providers"][0]["windows"].as_array().unwrap();
        assert!(windows.iter().all(|window| window.get("idle").is_none()));
    }

    #[test]
    fn antigravity_dashboard_keeps_all_families_after_global_reset() {
        let windows = vec![
            NamedRateWindow::new("model-gemini", "Gemini Pro", RateWindow::new(0.0)),
            NamedRateWindow::new("model-claude", "Claude Sonnet", RateWindow::new(0.0)),
        ];
        let json = serde_json::to_value(build_snapshot(&input(
            vec![antigravity_envelope(windows)],
            DashboardIdentity::Redacted,
        )))
        .unwrap();
        let windows = json["providers"][0]["windows"].as_array().unwrap();
        assert!(windows.iter().all(|window| window.get("idle").is_none()));
    }

    #[test]
    fn stale_after_floor_and_scaling() {
        let mut input_fast = input(vec![], DashboardIdentity::Redacted);
        input_fast.refresh_seconds = 30;
        assert_eq!(build_snapshot(&input_fast).stale_after_seconds, 180);
        input_fast.refresh_seconds = 120;
        assert_eq!(build_snapshot(&input_fast).stale_after_seconds, 360);
    }

    #[test]
    fn cost_payload_surfaces_today_and_30d() {
        let mut costs = HashMap::new();
        costs.insert(
            "claude".to_string(),
            RawCostPayload {
                today_usd: Some(1.25),
                last_30_days_usd: Some(40.5),
            },
        );
        let mut input = input(
            vec![provider_envelope(Ok(fetch_result(5.0, None, None)))],
            DashboardIdentity::Redacted,
        );
        input.costs = costs;
        let row = &serde_json::to_value(build_snapshot(&input)).unwrap()["providers"][0];
        assert_eq!(row["cost"]["todayUSD"], 1.25);
        assert_eq!(row["cost"]["last30DaysUSD"], 40.5);
    }

    #[test]
    fn claude_accounts_attach_to_first_claude_row_only() {
        let second = ProviderFetchEnvelope {
            id: "claude".to_string(),
            display_name: "Claude".to_string(),
            session_label: "Session".to_string(),
            weekly_label: "Weekly".to_string(),
            fetch: Ok(fetch_result(9.0, None, None)),
        };
        let mut input = input(
            vec![provider_envelope(Ok(fetch_result(3.0, None, None))), second],
            DashboardIdentity::Redacted,
        );
        input.claude_accounts = Some(ClaudeAccountsInput {
            accounts: Ok(vec![AccountFetchEnvelope {
                id: "uuid-1".to_string(),
                label: "Work".to_string(),
                active: true,
                fetch: Ok(fetch_result(66.0, Some("work@corp.example"), None)),
            }]),
        });
        let json = serde_json::to_value(build_snapshot(&input)).unwrap();
        let accounts = json["providers"][0]["accounts"].as_array().unwrap();
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0]["label"], "Work");
        assert_eq!(accounts[0]["active"], true);
        assert_eq!(
            accounts[0]["identity"]["accountEmail"],
            "redacted@corp.example"
        );
        assert!(json["providers"][1].get("accounts").is_none());
    }

    #[test]
    fn account_payload_serializes_camel_case_updated_at() {
        // Pinned v1: AccountPayload's snake_case `updated_at` field must cross
        // the wire as `updatedAt`. An errored account carries the deterministic
        // `generated_at` timestamp, so this golden is reproducible.
        let mut input = input(
            vec![provider_envelope(Ok(fetch_result(3.0, None, None)))],
            DashboardIdentity::Redacted,
        );
        input.claude_accounts = Some(ClaudeAccountsInput {
            accounts: Ok(vec![AccountFetchEnvelope {
                id: "uuid-1".to_string(),
                label: "Broken".to_string(),
                active: false,
                fetch: Err("cookie expired".to_string()),
            }]),
        });
        let json = serde_json::to_value(build_snapshot(&input)).unwrap();
        let account = &json["providers"][0]["accounts"][0];
        assert_eq!(account["error"], "cookie expired");
        assert_eq!(account["updatedAt"], "2026-08-08T01:02:03Z");
        assert!(
            account.get("updated_at").is_none(),
            "snake_case updated_at must not appear on the v1 wire"
        );
    }

    #[test]
    fn status_payload_serializes_camel_case_updated_at() {
        // v1 has no live status pipeline (status is null on rows), but the
        // schema struct itself must still serialize camelCase to match the
        // pinned v1 contract when a status is eventually attached.
        let status = StatusPayload {
            level: "ok".to_string(),
            label: "Healthy".to_string(),
            updated_at: Some(
                DateTime::parse_from_rfc3339("2026-08-08T01:02:03Z")
                    .unwrap()
                    .with_timezone(&Utc),
            ),
        };
        let json = serde_json::to_value(&status).unwrap();
        assert!(json.get("updatedAt").is_some(), "updatedAt must be present");
        assert_eq!(json["updatedAt"], "2026-08-08T01:02:03Z");
        assert!(
            json.get("updated_at").is_none(),
            "snake_case updated_at must not appear on the v1 wire"
        );
    }

    #[test]
    fn claude_accounts_adapter_error() {
        let mut input = input(
            vec![provider_envelope(Ok(fetch_result(3.0, None, None)))],
            DashboardIdentity::Redacted,
        );
        input.claude_accounts = Some(ClaudeAccountsInput {
            accounts: Err("token store unreadable".to_string()),
        });
        let row = &serde_json::to_value(build_snapshot(&input)).unwrap()["providers"][0];
        assert_eq!(row["accountsError"], "token store unreadable");
        assert!(row.get("accounts").is_none());
    }

    #[test]
    fn account_error_and_pace_rows() {
        let mut usage = UsageSnapshot::new(RateWindow::new(10.0));
        let mut weekly = RateWindow::new(40.0);
        weekly.resets_at = Some(Utc::now() + chrono::Duration::days(3));
        weekly.window_minutes = Some(10080);
        usage.secondary = Some(weekly);
        usage.account_email = Some("a@b.c".to_string());
        let mut input = input(
            vec![provider_envelope(Ok(fetch_result(3.0, None, None)))],
            DashboardIdentity::Redacted,
        );
        input.claude_accounts = Some(ClaudeAccountsInput {
            accounts: Ok(vec![
                AccountFetchEnvelope {
                    id: "u1".to_string(),
                    label: "Main".to_string(),
                    active: true,
                    fetch: Ok(ProviderFetchResult::new(usage, "oauth")),
                },
                AccountFetchEnvelope {
                    id: "u2".to_string(),
                    label: "Broken".to_string(),
                    active: false,
                    fetch: Err("cookie expired".to_string()),
                },
            ]),
        });
        let accounts =
            serde_json::to_value(build_snapshot(&input)).unwrap()["providers"][0]["accounts"]
                .as_array()
                .unwrap()
                .clone();
        assert_eq!(accounts[0]["windows"][0]["kind"], "session");
        assert_eq!(accounts[0]["windows"][1]["kind"], "weekly");
        let pace = &accounts[0]["pace"]["secondary"];
        assert!(pace["stage"].is_string());
        assert!(pace["expectedUsedPercent"].is_number());
        assert!(pace["summary"].is_string());
        assert_eq!(accounts[1]["error"], "cookie expired");
        assert!(accounts[1]["pace"].is_null());
    }

    #[test]
    fn status_is_null_in_v1_so_chip_is_hidden() {
        // #2723 parity: no status pipeline feeds dashboard v1, so every row
        // reports status null and the shell never renders a chip.
        let payload = build_snapshot(&input(
            vec![provider_envelope(Ok(fetch_result(1.0, None, None)))],
            DashboardIdentity::Redacted,
        ));
        assert!(serde_json::to_value(&payload).unwrap()["providers"][0]["status"].is_null());
    }

    #[test]
    fn cost_snapshot_from_fetch_does_not_leak_into_credits() {
        let mut result = fetch_result(1.0, None, None);
        result.cost = Some(CostSnapshot::new(500.5, "credits", "Monthly"));
        let payload = build_snapshot(&input(
            vec![provider_envelope(Ok(result))],
            DashboardIdentity::Redacted,
        ));
        assert!(serde_json::to_value(&payload).unwrap()["providers"][0]["credits"].is_null());
    }
}
