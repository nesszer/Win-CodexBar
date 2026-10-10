//! Tests for the CLI usage renderer.

use super::*;
use crate::core::{
    CostSnapshot, FetchContext, NamedRateWindow, ProviderAccountData, ProviderDisplayDetail,
    ProviderFetchResult, ProviderId, ProviderInventoryItem, RateWindow, SourceMode, TokenAccount,
    TokenAccountKind, TokenAccountSupport, UsageSnapshot,
};
use crate::providers::claude::claude_swap::ClaudeSwapAccount;
use crate::status::{ProviderStatus as StatusInfo, StatusLevel};
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
        historical_usage: None,
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
        level: StatusLevel::Degraded,
        description: "Degraded Performance".to_string(),
        ..Default::default()
    };
    let payload =
        super::claude_swap::claude_swap_json_payload(&sample_swap_account(), Some(&status));
    assert_eq!(payload["account"]["status"], "ok");
    assert_eq!(payload["status"]["level"], "degraded");
    assert_eq!(payload["status"]["description"], "Degraded Performance");
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
        level: StatusLevel::Operational,
        description: "All Systems Operational".to_string(),
        ..Default::default()
    };

    let text = super::claude_swap::render_claude_swap_brief(&[first, second], Some(&status), false);
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

    let brief = super::claude_swap::render_claude_swap_brief(&[account], None, false);
    assert!(!brief.contains("Last known usage"));
    assert!(!brief.contains("44%"));
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

    let output = render_text_with_status(ProviderId::Codex, &result, None, false);

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
    let full = render_text_with_status(ProviderId::Antigravity, &result, None, false);
    let brief = render_brief_text(ProviderId::Antigravity, &result);
    assert!(full.contains("Weekly:"));
    assert!(brief.contains("Weekly 20%"));
}
#[test]
fn primary_label_override_is_shared_by_full_and_brief_renderers() {
    let result =
        fetch_result(UsageSnapshot::new(RateWindow::new(42.0)).with_primary_label("Monthly"));

    let full = render_text_with_status(ProviderId::Grok, &result, None, false);
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

    let output = render_text_with_status(ProviderId::OpenRouter, &result, None, false);

    assert!(output.contains("Last 30 days (UTC): $0.00"));
    assert!(!output.contains("Cost:    $0.00"));

    let json = render_json_result(ProviderId::OpenRouter, result, None);
    assert!(json.get("usage").is_some());
    assert!(json.get("cost").is_some());
    assert!(json.get("history").is_none());
}

#[test]
fn ordinary_costs_keep_the_existing_cost_line() {
    let result = fetch_result(UsageSnapshot::new(RateWindow::new(0.0)))
        .with_cost(CostSnapshot::new(2.5, "EUR", "This month (API key)"));

    let output = render_text_with_status(ProviderId::OpenRouter, &result, None, false);

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

    let full = render_text_with_status(ProviderId::Grok, &result, None, false);
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

    let full = render_text_with_status(ProviderId::Grok, &result, None, false);
    let json = render_json_result(ProviderId::Grok, result, None);

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

    let text = render_text_with_status(ProviderId::LiteLLM, &result, None, false);
    let json = render_json_result(ProviderId::LiteLLM, result, None);

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

    let json = render_json_result(ProviderId::Grok, result, None);
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
    render_text_with_status(ProviderId::OpenRouter, &result, None, false)
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
    let json = render_json_result(ProviderId::OpenRouter, result, None);
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
    let json = render_json_result(ProviderId::Mistral, detail_backed_result(None), None);
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
