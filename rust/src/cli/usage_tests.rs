//! Tests for the CLI usage renderer.

use super::*;
use crate::core::{
    CostSnapshot, ProviderAccountData, ProviderDisplayDetail, ProviderInventoryItem, RateWindow,
    TokenAccount, TokenAccountSupport, UsageSnapshot,
};
use crate::providers::claude::claude_swap::ClaudeSwapAccount;
use crate::status::{ProviderStatus as StatusInfo, StatusLevel};
use chrono::Utc;
use fetch_helpers::find_token_account;
use render::render_json_result;

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
    assert!(UsageCommand::from_args(args).is_err());
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
