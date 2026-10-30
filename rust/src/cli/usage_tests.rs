//! Tests for the CLI usage renderer.

use super::*;
use crate::core::{
    CostSnapshot, FetchContext, NamedRateWindow, ProviderAccountData, ProviderDisplayDetail,
    ProviderFetchResult, ProviderId, ProviderInventoryItem, RateWindow, SourceMode, TokenAccount,
    TokenAccountKind, TokenAccountSupport, UsageSnapshot,
};
use crate::providers::claude::claude_swap::{ClaudeSwapAccount, ClaudeSwapAccountSource};
use crate::status::{ProviderStatus as StatusInfo, StatusIndicator};
use chrono::Utc;
use fetch_helpers::find_token_account;
use render::{render_json_result, render_text_with_status};

fn fetch_result(usage: UsageSnapshot) -> ProviderFetchResult {
    ProviderFetchResult::new(usage, "test")
}

fn sample_swap_account() -> ClaudeSwapAccount {
    use crate::providers::claude::claude_swap::{
        ClaudeSwapScopedWindowDto, ClaudeSwapUsageWindowDto,
    };
    ClaudeSwapAccount {
        id: "claude-swap:2".to_string(),
        source: ClaudeSwapAccountSource::ClaudeSwap,
        slot: 2,
        label: "work@example.com".to_string(),
        email: Some("work@example.com".to_string()),
        organization: None,
        alias: None,
        is_active: false,
        status: "ok".to_string(),
        error: None,
        five_hour: Some(ClaudeSwapUsageWindowDto {
            used_percent: 81.0,
            resets_at: None,
        }),
        seven_day: Some(ClaudeSwapUsageWindowDto {
            used_percent: 18.0,
            resets_at: None,
        }),
        scoped: vec![ClaudeSwapScopedWindowDto {
            name: "Fable only".to_string(),
            used_percent: 4.0,
            resets_at: None,
        }],
        action: Some(crate::providers::claude::claude_swap::ClaudeSwapAccountAction::Switch),
        is_disabled: false,
        spend: None,
        usage_fetched_at: None,
        historical_usage: None,
        session_equivalent_forecast: None,
    }
}

#[test]
fn claude_swap_json_payload_is_allow_listed() {
    let payload = super::claude_swap::claude_swap_json_payload(&sample_swap_account(), None);
    assert_eq!(payload["provider"], "claude");
    assert_eq!(payload["source"], "claude-swap");
    assert_eq!(payload["account"]["id"], "claude-swap:2");
    assert_eq!(payload["account"]["fiveHour"]["usedPercent"], 81.0);
    assert_eq!(payload["account"]["scoped"][0]["name"], "Fable only");
}

#[test]
fn claude_swap_json_payload_keeps_provider_status_distinct() {
    let status = StatusInfo {
        indicator: StatusIndicator::Minor,
        description: Some("Degraded Performance".to_string()),
        updated_at: None,
    };
    let payload =
        super::claude_swap::claude_swap_json_payload(&sample_swap_account(), Some(&status));
    assert_eq!(payload["account"]["status"], "ok");
    assert_eq!(payload["status"]["level"], "degraded");
    assert_eq!(payload["status"]["description"], "Degraded Performance");
}

/// The CLI keeps its pre-poller `status.level` values, symbols and status
/// line for every feed indicator; only raw `minor` changed, from `unknown`.
#[test]
fn cli_status_output_keeps_the_legacy_level_vocabulary() {
    use render::{append_status_line, render_status_indicator};
    let cases = [
        (StatusIndicator::Operational, "operational", " ●", false),
        (StatusIndicator::Minor, "degraded", " ◐", true),
        (StatusIndicator::Major, "major", " ○", true),
        (StatusIndicator::Critical, "major", " ○", true),
        (StatusIndicator::Maintenance, "unknown", " ?", false),
        (StatusIndicator::Unknown, "unknown", " ?", false),
    ];
    for (indicator, level, symbol, has_line) in cases {
        let status = StatusInfo {
            indicator,
            description: Some("Feed text".to_string()),
            updated_at: None,
        };
        let json = render_json_result(
            ProviderId::Claude,
            fetch_result(UsageSnapshot::new(RateWindow::new(10.0))),
            Some(&status),
            None,
        );
        assert_eq!(json["status"]["level"], level, "{indicator:?}");
        assert_eq!(json["status"]["description"], "Feed text", "{indicator:?}");
        let swap =
            super::claude_swap::claude_swap_json_payload(&sample_swap_account(), Some(&status));
        assert_eq!(swap["status"]["level"], level, "{indicator:?}");
        assert_eq!(
            render_status_indicator(Some(&status), false),
            symbol,
            "{indicator:?}"
        );
        let mut lines = Vec::new();
        append_status_line(&mut lines, Some(&status));
        let expected: Vec<String> = if has_line {
            vec!["  Status: Feed text".to_string()]
        } else {
            Vec::new()
        };
        assert_eq!(lines, expected, "{indicator:?}");
    }
}

#[test]
fn claude_swap_brief_renderer_keeps_one_line_per_provider() {
    let mut first = sample_swap_account();
    first.is_active = true;
    let mut second = sample_swap_account();
    second.id = "claude-swap:3".to_string();
    second.slot = 3;
    second.label = "personal@example.com".to_string();
    let status = StatusInfo {
        indicator: StatusIndicator::Operational,
        description: Some("All Systems Operational".to_string()),
        updated_at: None,
    };

    let text = super::claude_swap::render_claude_swap_brief(
        ClaudeSwapAccountSource::ClaudeSwap,
        &[first, second],
        Some(&status),
        false,
    );
    assert!(!text.contains('\n'));
    assert!(text.contains("work@example.com (active)"));
    assert!(text.contains("personal@example.com"));
    assert!(text.contains("Status All Systems Operational"));
}

#[test]
fn claude_swap_text_renderer_shows_windows_and_status() {
    let text = super::claude_swap::render_claude_swap_text(&sample_swap_account(), None, false);
    assert!(text.contains("claude-swap"));
    assert!(text.contains("work@example.com"));
    assert!(text.contains("Session 81%"));
    assert!(text.contains("Weekly 18%"));
    assert!(text.contains("Fable only 4%"));
}

#[test]
fn claude_swap_detailed_text_shows_history_but_brief_does_not() {
    use crate::providers::claude::claude_swap::{
        ClaudeSwapHistoricalUsageDto, ClaudeSwapSpendWindowDto, ClaudeSwapUsageWindowDto,
    };
    let mut account = sample_swap_account();
    account.spend = Some(ClaudeSwapSpendWindowDto {
        used: 2.0,
        limit: 20.0,
        used_percent: 10.0,
        currency_code: Some("USD".to_string()),
        resets_at: None,
    });
    account.historical_usage = Some(ClaudeSwapHistoricalUsageDto {
        five_hour: Some(ClaudeSwapUsageWindowDto {
            used_percent: 44.0,
            resets_at: None,
        }),
        seven_day: None,
        scoped: vec![],
        spend: None,
        fetched_at: "2026-09-12T00:45:00Z".parse().unwrap(),
        provenance: "source_reported_last_good",
    });
    let detailed = super::claude_swap::render_claude_swap_text(&account, None, false);
    assert!(detailed.contains("Spend 2.00/20.00 USD (10%)"));
    assert!(detailed.contains("Last known usage (captured 2026-09-12T00:45:00+00:00)"));
    assert!(detailed.contains("Session 44%"));

    let brief = super::claude_swap::render_claude_swap_brief(
        ClaudeSwapAccountSource::ClaudeSwap,
        &[account],
        None,
        false,
    );
    assert!(!brief.contains("Last known usage"));
    assert!(!brief.contains("44%"));
}

/// tcr rows name their own tool in JSON, text and brief output instead of
/// the claude-swap label.
#[test]
fn tcr_rows_are_labelled_with_their_source() {
    let mut account = sample_swap_account();
    account.id = "tcr:2".to_string();
    account.source = ClaudeSwapAccountSource::Tcr;
    account.action = None;
    account.is_disabled = true;

    let payload = super::claude_swap::claude_swap_json_payload(&account, None);
    assert_eq!(payload["source"], "tcr");
    assert_eq!(payload["account"]["id"], "tcr:2");

    let text = super::claude_swap::render_claude_swap_text(&account, None, false);
    assert!(text.starts_with("Claude (tcr)  work@example.com"), "{text}");
    assert!(text.contains("Disabled by tcr."), "{text}");
    assert!(!text.contains("claude-swap"), "{text}");

    let brief = super::claude_swap::render_claude_swap_brief(
        ClaudeSwapAccountSource::Tcr,
        &[account],
        None,
        false,
    );
    assert!(brief.starts_with("Claude (tcr): "), "{brief}");
}

#[test]
fn all_accounts_conflicts_with_explicit_account() {
    let args = UsageArgs {
        all_accounts: true,
        account: Some("work".to_string()),
        ..Default::default()
    };
    assert!(UsageCommand::from_args_with(args, || panic!("settings must not be read")).is_err());
}

fn no_settings_read() -> Vec<ProviderId> {
    panic!("an explicit --provider must not read the enabled providers")
}

#[test]
fn enabled_default_mirrors_upstream_provider_selection() {
    use ProviderId::{Claude, Codex, Cursor, Gemini};

    assert_eq!(
        ProviderSelection::for_enabled(Vec::new()),
        ProviderSelection::Custom(Vec::new())
    );
    assert_eq!(
        ProviderSelection::for_enabled(vec![Cursor]),
        ProviderSelection::Single(Cursor)
    );
    // Exactly the primary pair is `Both`, in either display order.
    for pair in [vec![Codex, Claude], vec![Claude, Codex]] {
        let selection = ProviderSelection::for_enabled(pair);
        assert_eq!(selection, ProviderSelection::Both);
        assert_eq!(selection.as_list(), vec![Codex, Claude]);
    }
    // Any other set keeps the enabled display order.
    assert_eq!(
        ProviderSelection::for_enabled(vec![Gemini, Codex]).as_list(),
        vec![Gemini, Codex]
    );
    assert_eq!(
        ProviderSelection::for_enabled(vec![Claude, Cursor, Codex]).as_list(),
        vec![Claude, Cursor, Codex]
    );
}

#[test]
fn explicit_provider_never_reads_enabled_providers() {
    assert_eq!(
        ProviderSelection::from_arg_or_enabled(Some("codex"), no_settings_read).unwrap(),
        ProviderSelection::Single(ProviderId::Codex)
    );
    assert_eq!(
        ProviderSelection::from_arg_or_enabled(Some("ALL"), no_settings_read).unwrap(),
        ProviderSelection::All
    );
    assert_eq!(
        ProviderSelection::from_arg_or_enabled(Some("both"), no_settings_read).unwrap(),
        ProviderSelection::Both
    );
    assert!(
        ProviderSelection::from_arg_or_enabled(Some("not-a-provider"), no_settings_read).is_err()
    );
    // `cost` keeps its own default.
    assert_eq!(
        ProviderSelection::from_arg(None).unwrap(),
        ProviderSelection::Single(ProviderId::Claude)
    );
}

#[test]
fn provider_flag_accepts_every_catalog_id_from_config_providers() {
    // `config providers --format json` reports each provider by its CLI name,
    // and `--provider` must select that same provider (upstream #4643).
    for id in ProviderId::all() {
        assert_eq!(
            ProviderSelection::from_arg(Some(id.cli_name())).unwrap(),
            ProviderSelection::Single(*id),
            "--provider {} should select {}",
            id.cli_name(),
            id.display_name()
        );
    }
}

#[test]
fn plain_usage_queries_the_enabled_providers() {
    let command = UsageCommand::from_args_with(UsageArgs::default(), || {
        vec![ProviderId::Claude, ProviderId::Cursor]
    })
    .unwrap();
    assert_eq!(
        command.providers,
        vec![ProviderId::Claude, ProviderId::Cursor]
    );

    let command = UsageCommand::from_args_with(UsageArgs::default(), Vec::new).unwrap();
    assert!(command.providers.is_empty());

    let explicit = UsageArgs {
        provider: Some("codex".to_string()),
        ..Default::default()
    };
    let command = UsageCommand::from_args_with(explicit, no_settings_read).unwrap();
    assert_eq!(command.providers, vec![ProviderId::Codex]);
}

#[test]
fn account_requires_one_provider_including_the_enabled_default() {
    let with_account = || UsageArgs {
        account: Some("work".to_string()),
        ..Default::default()
    };
    let error = UsageCommand::from_args_with(with_account(), || {
        vec![ProviderId::Codex, ProviderId::Claude]
    })
    .err()
    .expect("several enabled providers cannot take --account");
    assert!(error.to_string().contains("single --provider"), "{error}");

    let command = UsageCommand::from_args_with(with_account(), || vec![ProviderId::Codex]).unwrap();
    assert_eq!(command.providers, vec![ProviderId::Codex]);
    assert_eq!(command.account.as_deref(), Some("work"));
}

#[test]
fn usage_output_format_accepts_toon() {
    assert_eq!(
        "toon".parse::<UsageOutputFormat>(),
        Ok(UsageOutputFormat::Toon)
    );
    assert!("toon".parse::<OutputFormat>().is_err());
}

#[test]
fn openrouter_account_ref_resolves_labeled_key() {
    let mut data = ProviderAccountData::new();
    data.add_account(TokenAccount::new("Personal", "sk-or-v1-personal"));
    data.add_account(TokenAccount::new("Work", "sk-or-v1-work"));
    data.set_active(0);

    let work = find_token_account(&data, "Work").unwrap();
    let env = TokenAccountSupport::env_override(ProviderId::OpenRouter, &work.token).unwrap();
    assert_eq!(
        env.get("OPENROUTER_API_KEY").map(String::as_str),
        Some("sk-or-v1-work")
    );

    let by_index = find_token_account(&data, "2").unwrap();
    assert_eq!(by_index.token, "sk-or-v1-work");
}

#[test]
fn kimi_account_projection_forces_isolated_web_and_preserves_region() {
    let account = TokenAccount::new("work", "selected-kimi-auth");
    let mut ctx = FetchContext {
        source_mode: SourceMode::OAuth,
        api_region: Some("international".into()),
        api_key: Some("ambient-api-key".into()),
        ..FetchContext::default()
    };

    super::fetch_helpers::project_token_account(ProviderId::Kimi, &account, &mut ctx);

    assert_eq!(ctx.source_mode, SourceMode::Web);
    assert_eq!(
        ctx.manual_cookie_header.as_deref(),
        Some("kimi-auth=selected-kimi-auth")
    );
    assert_eq!(ctx.api_key, None);
    assert_eq!(ctx.api_region.as_deref(), Some("international"));
    assert!(ctx.token_account_isolated);
}

#[test]
fn doubao_account_projection_uses_only_the_selected_ark_key() {
    let account = TokenAccount::new("work", "selected-ark-key");
    let mut ctx = FetchContext {
        source_mode: SourceMode::Cli,
        api_key: Some("ambient-key".into()),
        ..FetchContext::default()
    };

    super::fetch_helpers::project_token_account(ProviderId::Doubao, &account, &mut ctx);

    assert_eq!(ctx.source_mode, SourceMode::OAuth);
    assert_eq!(ctx.api_key.as_deref(), Some("selected-ark-key"));
    assert_eq!(ctx.token_account_kind, Some(TokenAccountKind::ApiKey));
    assert!(ctx.token_account_isolated);
}

#[test]
fn opencodego_account_projection_distinguishes_api_and_cookie_routes() {
    let mut api_ctx = FetchContext::default();
    super::fetch_helpers::project_token_account(
        ProviderId::OpenCodeGo,
        &TokenAccount::new("api", "selected-opencode-key"),
        &mut api_ctx,
    );
    assert_eq!(api_ctx.source_mode, SourceMode::Auto);
    assert_eq!(api_ctx.api_key.as_deref(), Some("selected-opencode-key"));
    assert_eq!(api_ctx.token_account_kind, Some(TokenAccountKind::ApiKey));
    assert!(!api_ctx.auto_prefer_web);

    let mut cookie_ctx = FetchContext::default();
    super::fetch_helpers::project_token_account(
        ProviderId::OpenCodeGo,
        &TokenAccount::new("web", "Cookie: session=selected-session"),
        &mut cookie_ctx,
    );
    assert_eq!(cookie_ctx.source_mode, SourceMode::Web);
    assert_eq!(
        cookie_ctx.manual_cookie_header.as_deref(),
        Some("Cookie: session=selected-session")
    );
    assert_eq!(
        cookie_ctx.token_account_kind,
        Some(TokenAccountKind::Cookie)
    );

    cookie_ctx.source_mode = SourceMode::Cli;
    super::fetch_helpers::project_token_account(
        ProviderId::OpenCodeGo,
        &TokenAccount::new("api", "another-key"),
        &mut cookie_ctx,
    );
    assert_eq!(cookie_ctx.source_mode, SourceMode::Cli);
}

#[test]
fn text_rendering_shows_sub_one_percent_usage() {
    let result = fetch_result(UsageSnapshot::new(RateWindow::new(0.4)));

    let output = render_text_with_status(ProviderId::Codex, &result, None, false, None);

    assert!(output.contains("<1% used"));
}

#[test]
fn brief_rendering_keeps_one_line_per_provider() {
    let result = fetch_result(
        UsageSnapshot::new(RateWindow::new(0.4))
            .with_secondary(RateWindow::new(100.0))
            .with_login_method("Pro"),
    );

    let output = render_brief_text(ProviderId::Claude, &result);

    assert_eq!(
        output,
        "Claude: Session (5h) <1%, Weekly 100%, resets n/a, Pro"
    );
}

#[test]
fn secondary_label_override_is_shared_by_full_and_brief_renderers() {
    let result = fetch_result(
        UsageSnapshot::new(RateWindow::new(10.0))
            .with_secondary(RateWindow::new(20.0))
            .with_secondary_label("Weekly"),
    );
    let full = render_text_with_status(ProviderId::Antigravity, &result, None, false, None);
    let brief = render_brief_text(ProviderId::Antigravity, &result);
    assert!(full.contains("Weekly:"));
    assert!(brief.contains("Weekly 20%"));
}
#[test]
fn primary_label_override_is_shared_by_full_and_brief_renderers() {
    let result =
        fetch_result(UsageSnapshot::new(RateWindow::new(42.0)).with_primary_label("Monthly"));

    let full = render_text_with_status(ProviderId::Grok, &result, None, false, None);
    let brief = render_brief_text(ProviderId::Grok, &result);

    assert!(full.contains("Monthly:"));
    assert!(brief.contains("Grok: Monthly 42%"));
    assert!(!brief.contains("Credits 42%"));
}

#[test]
fn gemini_plan_preserves_acronym_casing() {
    let result = fetch_result(
        UsageSnapshot::new(RateWindow::new(0.0))
            .with_login_method("Gemini Code Assist in Google One AI Pro"),
    );

    let output = render_text(ProviderId::Gemini, &result, false);

    assert!(output.contains("Plan:    Gemini Code Assist in Google One AI Pro"));
    assert!(!output.contains("Google One Ai Pro"));
}

#[test]
fn openrouter_history_preserves_period_and_known_zero_in_text() {
    let result = fetch_result(UsageSnapshot::new(RateWindow::new(0.0)))
        .with_cost(CostSnapshot::new(0.0, "USD", "Last 30 days (UTC)").always_visible());

    let output = render_text_with_status(ProviderId::OpenRouter, &result, None, false, None);

    assert!(output.contains("Last 30 days (UTC): $0.00"));
    assert!(!output.contains("Cost:    $0.00"));

    let json = render_json_result(ProviderId::OpenRouter, result, None, None);
    assert!(json.get("usage").is_some());
    assert!(json.get("cost").is_some());
    assert!(json.get("history").is_none());
}

#[test]
fn ordinary_costs_keep_the_existing_cost_line() {
    let result = fetch_result(UsageSnapshot::new(RateWindow::new(0.0)))
        .with_cost(CostSnapshot::new(2.5, "EUR", "This month (API key)"));

    let output = render_text_with_status(ProviderId::OpenRouter, &result, None, false, None);

    assert!(output.contains("Cost:    €2.50 (This month (API key))"));
    assert!(!output.contains("Last 30 days"));
}

#[test]
fn inventory_is_rendered_in_full_text_but_not_brief_text() {
    let result = fetch_result(UsageSnapshot::new(RateWindow::new(10.0))).with_inventory_item(
        ProviderInventoryItem {
            id: "reset-credits".to_string(),
            title: "Limit Reset Credits".to_string(),
            available_count: 2,
            next_expires_at: Some(Utc::now() + chrono::Duration::hours(3)),
        },
    );

    let full = render_text_with_status(ProviderId::Grok, &result, None, false, None);
    let brief = render_brief_text(ProviderId::Grok, &result);

    assert!(full.contains("Limit Reset Credits: 2 available"));
    assert!(full.contains("Next expires in"));
    assert!(!brief.contains("Limit Reset Credits"));
}

#[test]
fn display_details_are_rendered_in_full_text_and_json() {
    let result = fetch_result(UsageSnapshot::new(RateWindow::new(10.0))).with_display_detail(
        ProviderDisplayDetail::new("credits", "Used this cycle", "12")
            .and_then(|row| row.with_secondary_value("Monthly refill: 100"))
            .and_then(|row| row.with_progress(12.0, 100.0)),
    );

    let full = render_text_with_status(ProviderId::Grok, &result, None, false, None);
    let json = render_json_result(ProviderId::Grok, result, None, None);

    assert!(full.contains("Used this cycle: 12 (Monthly refill: 100) [12.00/100.00]"));
    assert_eq!(json["details"][0]["title"], "Used this cycle");
    assert_eq!(json["details"][0]["progress"]["total"], 100.0);
}

#[test]
fn sectioned_display_details_print_one_heading_per_group() {
    let row = |id: &str, title: &str, section: Option<&str>| {
        let row = ProviderDisplayDetail::new(id, title, "1");
        match section {
            Some(section) => row.and_then(|row| row.with_section_title(section)),
            None => row,
        }
    };
    let result = fetch_result(UsageSnapshot::new(RateWindow::new(10.0)))
        .with_display_detail(row("plain", "Plain", None))
        .with_display_detail(row("a", "Alpha", Some("Model activity")))
        .with_display_detail(row("b", "Beta", Some("Model activity")));

    let text = render_text_with_status(ProviderId::LiteLLM, &result, None, false, None);
    let json = render_json_result(ProviderId::LiteLLM, result, None, None);

    assert_eq!(text.matches("Model activity:").count(), 1);
    assert!(text.find("Plain: 1").unwrap() < text.find("Model activity:").unwrap());
    assert!(text.find("Model activity:").unwrap() < text.find("Alpha: 1").unwrap());
    assert_eq!(json["details"][0]["sectionTitle"], serde_json::Value::Null);
    assert_eq!(json["details"][2]["sectionTitle"], "Model activity");
}

#[test]
fn json_inventory_is_additive_and_contains_no_redemption_token() {
    let result = fetch_result(UsageSnapshot::new(RateWindow::new(10.0))).with_inventory_item(
        ProviderInventoryItem {
            id: "reset-credits".to_string(),
            title: "Limit Reset Credits".to_string(),
            available_count: 1,
            next_expires_at: None,
        },
    );

    let json = render_json_result(ProviderId::Grok, result, None, None);
    assert_eq!(json["inventory"][0]["availableCount"], 1);
    assert!(
        serde_json::to_string(&json)
            .unwrap()
            .contains("reset-credits")
    );
    assert!(
        !serde_json::to_string(&json)
            .unwrap()
            .contains("coupon-token-secret")
    );
}

fn history_output(cost: CostSnapshot) -> String {
    let result = fetch_result(UsageSnapshot::new(RateWindow::new(0.0))).with_cost(cost);
    render_text_with_status(ProviderId::OpenRouter, &result, None, false, None)
}

#[test]
fn history_line_shows_provenance_and_token_total() {
    use crate::spend_contract::CostProvenance;

    let cases = [
        (CostProvenance::VendorMetered, "$1.25 (reported)"),
        (CostProvenance::ListPriceEstimate, "$1.25 (estimated)"),
        (CostProvenance::Mixed, "$1.25 (includes estimates)"),
        (CostProvenance::Unknown, "$1.25"),
    ];
    for (provenance, spend) in cases {
        let output = history_output(
            CostSnapshot::new(1.25, "USD", "Last 30 days (UTC)")
                .with_history_tokens(15)
                .with_provenance(provenance)
                .always_visible(),
        );
        let expected = format!("  Last 30 days (UTC): {spend} · 15 tokens");
        assert_eq!(output.matches(&expected).count(), 1, "{output}");
    }
}

#[test]
fn history_line_preserves_known_zero_singular_token_and_currency() {
    use crate::spend_contract::CostProvenance;

    let zero = history_output(
        CostSnapshot::new(0.0, "USD", "Last 30 days (UTC)")
            .with_history_tokens(0)
            .with_provenance(CostProvenance::VendorMetered)
            .always_visible(),
    );
    assert!(zero.contains("Last 30 days (UTC): $0.00 (reported) · 0 tokens"));

    let one = history_output(
        CostSnapshot::new(0.0, "USD", "Last 1 day")
            .with_history_tokens(1)
            .always_visible(),
    );
    assert!(one.ends_with("Last 1 day: $0.00 · 1 token"), "{one}");

    let euro = history_output(CostSnapshot::new(2.5, "EUR", "Billing period").always_visible());
    assert!(euro.contains("Billing period: €2.50"));
    assert!(!euro.contains("token"));
}

#[test]
fn history_line_compacts_large_token_totals() {
    let cases = [
        (999, "999 tokens"),
        (1_000, "1K tokens"),
        (1_250, "1.2K tokens"),
        (15_400, "15K tokens"),
        (999_499, "999K tokens"),
        (999_500, "1M tokens"),
        (2_500_000, "2.5M tokens"),
        (999_500_000, "1B tokens"),
    ];
    for (tokens, expected) in cases {
        let output = history_output(
            CostSnapshot::new(1.0, "USD", "Last 30 days (UTC)")
                .with_history_tokens(tokens)
                .always_visible(),
        );
        assert!(
            output.contains(&format!("$1.00 · {expected}")),
            "{tokens}: {output}"
        );
    }
}

#[test]
fn history_fields_do_not_change_the_cost_json_contract() {
    use crate::spend_contract::CostProvenance;

    let result = fetch_result(UsageSnapshot::new(RateWindow::new(0.0))).with_cost(
        CostSnapshot::new(1.25, "USD", "Last 30 days (UTC)")
            .with_history_tokens(15)
            .with_provenance(CostProvenance::VendorMetered)
            .always_visible(),
    );
    let json = render_json_result(ProviderId::OpenRouter, result, None, None);
    let cost = json.get("cost").and_then(|cost| cost.as_object()).unwrap();
    assert!(!cost.contains_key("historyTokens") && !cost.contains_key("history_tokens"));
    assert!(!cost.contains_key("provenance"));
}

fn detail_window(used: f64, detail: &str, resets_at: Option<chrono::DateTime<Utc>>) -> RateWindow {
    RateWindow::with_details(used, None, resets_at, Some(detail.to_string()))
        .with_description_as_detail()
}

fn detail_backed_result(resets_at: Option<chrono::DateTime<Utc>>) -> ProviderFetchResult {
    let mut usage = UsageSnapshot::new(detail_window(
        75.0,
        "19.17 EUR / 25.50 EUR · 6.33 EUR remaining",
        resets_at,
    ))
    .with_primary_label("Included API");
    usage.extra_rate_windows.push(NamedRateWindow::new(
        "mistral-monthly-plan",
        "Monthly Plan",
        detail_window(
            13.0,
            "34.07 EUR / 255.00 EUR · 220.93 EUR remaining",
            resets_at,
        ),
    ));
    fetch_result(usage)
}

#[test]
fn detail_backed_windows_print_reset_then_amounts_lines() {
    let resets_at = Utc::now() + chrono::Duration::minutes(61);
    let output = render_text(
        ProviderId::Mistral,
        &detail_backed_result(Some(resets_at)),
        false,
    );
    let lines: Vec<&str> = output.lines().collect();

    let primary = lines
        .iter()
        .position(|line| line.starts_with("  Included API:"))
        .expect("primary line");
    assert!(lines[primary].ends_with("75% used"));
    assert!(lines[primary + 1].starts_with("    resets in "));
    assert_eq!(
        lines[primary + 2],
        "    19.17 EUR / 25.50 EUR · 6.33 EUR remaining"
    );
    let plan = lines
        .iter()
        .position(|line| line.starts_with("  Monthly Plan:"))
        .expect("plan line");
    assert!(lines[plan].ends_with("13% used"));
    assert!(lines[plan + 1].starts_with("    resets in "));
    assert_eq!(
        lines[plan + 2],
        "    34.07 EUR / 255.00 EUR · 220.93 EUR remaining"
    );
    assert!(!output.contains("(resets in"));
}

#[test]
fn detail_backed_windows_omit_reset_line_without_reset_date() {
    let output = render_text(ProviderId::Mistral, &detail_backed_result(None), false);
    let lines: Vec<&str> = output.lines().collect();

    assert!(!output.contains("resets in"));
    let plan = lines
        .iter()
        .position(|line| line.starts_with("  Monthly Plan:"))
        .expect("plan line");
    assert_eq!(
        lines[plan + 1],
        "    34.07 EUR / 255.00 EUR · 220.93 EUR remaining"
    );
}

#[test]
fn detail_backed_flag_stays_out_of_json_output() {
    let json = render_json_result(ProviderId::Mistral, detail_backed_result(None), None, None);
    let windows = json["usage"]["extra_rate_windows"]
        .as_array()
        .expect("extra windows");
    assert_eq!(windows[0]["id"], "mistral-monthly-plan");
    assert_eq!(
        windows[0]["window"]["reset_description"],
        "34.07 EUR / 255.00 EUR · 220.93 EUR remaining"
    );
    assert!(windows[0]["window"].get("descriptionIsDetail").is_none());
    assert!(windows[0]["window"].get("description_is_detail").is_none());
}

#[test]
fn cli_lane_pace_scores_copilot_against_the_february_cycle() {
    use chrono::TimeZone;
    // Copilot window with no length, reset Mar 1, now Feb 15: half of the
    // 28-day cycle has elapsed.
    let now = Utc.with_ymd_and_hms(2026, 2, 15, 0, 0, 0).unwrap();
    let reset = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
    let window = RateWindow::with_details(60.0, None, Some(reset), None);

    let pace = render::lane_pace_at(ProviderId::Copilot, &window, now, None, || None)
        .expect("copilot reset-window pace");
    assert_eq!(pace.expected_used_percent, 50.0);
    assert_eq!(pace.delta_percent, 10.0);
    assert_eq!(pace.eta_seconds, Some(806_400.0));

    // Providers without the capability use the lane's own rule.
    let mut fallback_used = false;
    let codex = render::lane_pace_at(ProviderId::Codex, &window, now, None, || {
        fallback_used = true;
        None
    });
    assert!(codex.is_none());
    assert!(fallback_used);
}

#[test]
fn cli_lane_pace_scores_a_reported_codex_30_day_window_without_calendar_inference() {
    use chrono::TimeZone;
    // A March reset makes accidental February calendar-month inference
    // observable: 20 of 30 days left is a third expected, 20 of 28 would not be.
    let reset = Utc.with_ymd_and_hms(2026, 3, 1, 16, 8, 45).unwrap();
    let now = reset - chrono::Duration::days(20);
    let window = RateWindow::with_details(11.0, Some(43_200), Some(reset), None);

    let pace = render::lane_pace_at(ProviderId::Codex, &window, now, None, || {
        panic!("a reported 30-day Codex window must use the reset-window rule")
    })
    .expect("codex reset-window pace");
    assert!((pace.expected_used_percent - 100.0 / 3.0).abs() < 1e-9);
    assert!((pace.delta_percent - (11.0 - 100.0 / 3.0)).abs() < 1e-9);
    assert!(pace.will_last_to_reset);
}

#[test]
fn cli_lane_pace_keeps_reset_window_pace_for_a_resolved_31_day_cycle() {
    use chrono::TimeZone;
    // StepFun already expands the 30-day sentinel to the real cycle: 31
    // days ending Mar 31. Mar 15 12:00 is 15.5 days in.
    let now = Utc.with_ymd_and_hms(2026, 3, 15, 12, 0, 0).unwrap();
    let reset = Utc.with_ymd_and_hms(2026, 3, 31, 0, 0, 0).unwrap();
    let window = RateWindow::with_details(70.0, Some(44_640), Some(reset), None);

    let pace = render::lane_pace_at(ProviderId::StepFun, &window, now, None, || {
        panic!("a calendar-month window must use the reset-window rule")
    })
    .expect("stepfun reset-window pace");
    assert_eq!(pace.expected_used_percent, 50.0);
    assert_eq!(pace.delta_percent, 20.0);
}

#[test]
fn cli_reset_window_pace_needs_three_percent_expected() {
    use chrono::TimeZone;
    // Upstream `CLIRenderer.paceMinimumExpectedPercent` (3). Copilot's cycle
    // ending Mar 31 is 31 days long; 12 hours in, 0.5 / 31 = 1.61% is
    // expected, so the CLI shows no pace and does not fall back.
    let reset = Utc.with_ymd_and_hms(2026, 3, 31, 0, 0, 0).unwrap();
    let window = RateWindow::with_details(5.0, None, Some(reset), None);
    let early = Utc.with_ymd_and_hms(2026, 2, 28, 12, 0, 0).unwrap();
    let pace = render::lane_pace_at(ProviderId::Copilot, &window, early, None, || {
        panic!("reset-window lanes never fall back")
    });
    assert!(pace.is_none());
    // One day in (3.23%), the pace shows.
    let day_one = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
    let pace = render::lane_pace_at(ProviderId::Copilot, &window, day_one, None, || None)
        .expect("pace after 3% expected");
    assert_eq!(pace.expected_used_percent, 100.0 / 31.0);
}

#[test]
fn copilot_monthly_windows_get_reset_window_pace_in_json_and_text() {
    use chrono::Duration;
    // Copilot windows have a reset date and no length: the CLI scores them
    // against the calendar month ending at the reset, as the Mac does.
    let reset = Utc::now() + Duration::days(10);
    let cycle_minutes = f64::from(RateWindow::calendar_month_window_minutes(reset).unwrap());
    let expected = 100.0 * (cycle_minutes - 10.0 * 1440.0) / cycle_minutes;
    let usage = UsageSnapshot::new(RateWindow::with_details(5.0, None, Some(reset), None))
        .with_secondary(RateWindow::with_details(5.0, None, Some(reset), None));

    let json = render_json_result(ProviderId::Copilot, fetch_result(usage.clone()), None, None);
    let primary = json["pace"]["primary"]["expectedUsedPercent"]
        .as_f64()
        .expect("copilot primary pace");
    assert!((primary - expected).abs() < 0.01, "{primary} vs {expected}");
    let secondary = json["pace"]["secondary"]["expectedUsedPercent"]
        .as_f64()
        .expect("copilot secondary pace");
    // Without the capability the secondary would use a 7-day fallback and a
    // 10-day reset would give no pace at all.
    assert!(
        (secondary - expected).abs() < 0.01,
        "{secondary} vs {expected}"
    );

    let text = render_text_with_status(
        ProviderId::Copilot,
        &fetch_result(usage.clone()),
        None,
        false,
        None,
    );
    assert_eq!(text.matches("  Pace:").count(), 2, "{text}");

    // Codex has no reset-window capability: a length-less primary has no pace.
    let codex = render_json_result(ProviderId::Codex, fetch_result(usage), None, None);
    assert!(codex["pace"].is_null() || codex["pace"]["primary"].is_null());
}

#[test]
fn json_pace_follows_weekly_work_days() {
    use chrono::{Datelike, Duration, Local, TimeZone};
    // Weekly window ending at the next local Monday 00:00, so it covers the
    // current Mon..Sun and Mon-Fri workday pace is ahead of linear pace at any
    // instant inside it.
    let today = Local::now().date_naive();
    let next_monday = today + Duration::days(7 - i64::from(today.weekday().num_days_from_monday()));
    let reset = Local
        .from_local_datetime(&next_monday.and_hms_opt(0, 0, 0).unwrap())
        .single()
        .unwrap()
        .with_timezone(&Utc);
    if reset - Utc::now() >= Duration::days(7) {
        return; // Monday 00:00 exactly or a DST-shortened week: nothing to compare.
    }
    let usage = UsageSnapshot::new(RateWindow::new(10.0)).with_secondary(RateWindow::with_details(
        10.0,
        Some(10080),
        Some(reset),
        None,
    ));
    let expected = |work_days: Option<u8>| {
        render_json_result(
            ProviderId::Codex,
            fetch_result(usage.clone()),
            None,
            work_days,
        )["pace"]["secondary"]["expectedUsedPercent"]
            .as_f64()
            .unwrap()
    };

    let linear = expected(None);
    // Each render reads the clock again, so compare within a small tolerance.
    assert!((expected(Some(7)) - linear).abs() < 0.01);
    assert!(expected(Some(5)) > linear);
}

mod app_cache_fallback {
    use super::super::fetch_helpers::{
        AppCacheFallback, app_cache_fallback, render_app_cache_json, render_app_cache_text,
    };
    use super::*;
    use crate::cli_usage_cache::{self, CliUsageCache, Entry};
    use crate::core::ProviderError;
    use chrono::{Duration, TimeZone};

    fn cached_usage() -> UsageSnapshot {
        let mut usage = UsageSnapshot::new(RateWindow::with_details(42.0, Some(300), None, None));
        usage.updated_at = Utc.with_ymd_and_hms(2026, 10, 20, 11, 58, 30).unwrap();
        usage
    }

    fn fallback(age_seconds: i64) -> AppCacheFallback {
        AppCacheFallback {
            result: ProviderFetchResult::new(cached_usage(), cli_usage_cache::SOURCE_LABEL),
            age_seconds,
            status: None,
        }
    }

    fn plain_command() -> UsageCommand {
        let mut command = UsageCommand::from_args_with(
            UsageArgs {
                provider: Some("codex".to_string()),
                source: "auto".to_string(),
                ..UsageArgs::default()
            },
            Vec::new,
        )
        .unwrap();
        command.use_color = false;
        command
    }

    #[test]
    fn json_row_is_marked_as_app_cache_with_its_age() {
        let row = render_app_cache_json(ProviderId::Codex, &plain_command(), fallback(90));
        assert_eq!(
            row,
            serde_json::json!({
                "provider": "codex",
                "source": "codexbar-app",
                "ageSeconds": 90,
                "usage": {
                    "primary": {
                        "used_percent": 42.0,
                        "window_minutes": 300,
                        "is_informational": false,
                    },
                    "updated_at": "2026-10-20T11:58:30Z",
                },
                "cost": null,
            })
        );
    }

    #[test]
    fn text_output_says_it_came_from_the_app_cache() {
        let command = plain_command();
        let text = render_app_cache_text(ProviderId::Codex, &command, &fallback(150));
        assert!(text.contains("(codexbar-app)"), "{text}");
        assert!(
            text.ends_with("Cached by the CodexBar app 2m ago (credentials could not be decrypted in this session)."),
            "{text}"
        );
        let mut brief = plain_command();
        brief.brief = true;
        let line = render_app_cache_text(ProviderId::Codex, &brief, &fallback(5));
        assert!(line.contains("Cached by the CodexBar app 5s ago"), "{line}");
        assert!(!line.contains('\n'), "{line}");
    }

    #[test]
    fn gate_is_closed_by_default_and_for_explicit_selections() {
        let command = plain_command();
        assert!(!command.app_cache_gate().opted_in);
        assert!(command.app_cache_gate().default_account_fetch);
        for args in [
            UsageArgs {
                source: "web".to_string(),
                ..UsageArgs::default()
            },
            UsageArgs {
                provider: Some("codex".to_string()),
                account: Some("1".to_string()),
                ..UsageArgs::default()
            },
            UsageArgs {
                all_accounts: true,
                ..UsageArgs::default()
            },
        ] {
            let command = UsageCommand::from_args_with(args, Vec::new).unwrap();
            assert!(!command.app_cache_gate().default_account_fetch);
        }
    }

    #[tokio::test]
    async fn fallback_serves_the_cache_only_when_sharing_is_on() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(cli_usage_cache::CACHE_FILE_NAME);
        let mut usage = cached_usage();
        usage.updated_at = Utc::now() - Duration::seconds(30);
        CliUsageCache::new(vec![Entry::new(ProviderId::Codex, &usage).unwrap()])
            .write(&path)
            .unwrap();
        // The process-wide witness only ever turns on, so setting it here
        // cannot open a gate that another test expects closed.
        cli_usage_cache::note_dpapi_decrypt_failure();
        let auth: anyhow::Error = ProviderError::AuthRequired.into();

        let mut command = plain_command();
        command.app_cache_path = Some(path.clone());
        assert!(
            app_cache_fallback(ProviderId::Codex, &command, &auth)
                .await
                .is_none(),
            "sharing off must not fall back"
        );

        command.app_cache_opted_in = true;
        let served = app_cache_fallback(ProviderId::Codex, &command, &auth)
            .await
            .expect("sharing on falls back");
        assert_eq!(served.result.source_label, "codexbar-app");
        assert!(
            !served.result.pace_authoritative,
            "stale cached rows must not feed pace advice"
        );
        assert!((30..=40).contains(&served.age_seconds));

        let timeout: anyhow::Error = ProviderError::Timeout.into();
        assert!(
            app_cache_fallback(ProviderId::Codex, &command, &timeout)
                .await
                .is_none()
        );
        let not_a_provider_error = anyhow::anyhow!("No active token account for Codex");
        assert!(
            app_cache_fallback(ProviderId::Codex, &command, &not_a_provider_error)
                .await
                .is_none()
        );
        assert!(
            app_cache_fallback(ProviderId::Claude, &command, &auth)
                .await
                .is_none()
        );
    }
}
