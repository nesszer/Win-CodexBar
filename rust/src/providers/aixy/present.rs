//! Map a validated Aixy key-usage response onto the shared provider result.

use chrono::SecondsFormat;

use super::model::{Budget, KeyUsage, Totals};
use crate::core::{
    CostSnapshot, NamedRateWindow, ProviderDisplayDetail, ProviderFetchResult, RateWindow,
    UsageSnapshot,
};
use crate::providers::format;

const COST_PERIOD: &str = "Last 7 days · attributed";
const LOGIN_METHOD: &str = "API key";

pub(super) fn build_result(usage: KeyUsage) -> ProviderFetchResult {
    // Overlapping budgets are never summed: only the two most utilised known
    // budgets take the primary/secondary lanes; the rest stay visible as
    // named windows.
    let selected = usage
        .budgets
        .iter()
        .filter(|budget| budget.known)
        .take(2)
        .map(|budget| budget.id.as_str())
        .collect::<Vec<_>>();

    let mut windows = selected
        .iter()
        .filter_map(|id| usage.budgets.iter().find(|budget| budget.id == *id))
        .map(rate_window);
    let primary = windows.next().unwrap_or_else(|| {
        RateWindow::informational(if usage.budgets.is_empty() {
            "No applicable budgets reported"
        } else {
            "Budget balance unavailable"
        })
    });
    let secondary = windows.next();

    let mut snapshot = UsageSnapshot::new(primary).with_login_method(LOGIN_METHOD);
    if let Some(secondary) = secondary {
        snapshot = snapshot.with_secondary(secondary);
    }
    for budget in usage
        .budgets
        .iter()
        .filter(|budget| !selected.contains(&budget.id.as_str()))
    {
        snapshot.extra_rate_windows.push(
            NamedRateWindow::new(
                format!("aixy-{}", budget.id),
                budget.title.clone(),
                rate_window(budget),
            )
            .with_usage_known(budget.known),
        );
    }

    let mut result = ProviderFetchResult::new(snapshot, "api").with_account_identity(&usage.key_id);
    if let Some(spend) = usage.totals.as_ref().and_then(|totals| totals.spend_usd) {
        result = result.with_cost(CostSnapshot::new(spend, "USD", COST_PERIOD));
    }
    for detail in details(&usage) {
        result = result.with_display_detail(detail);
    }
    result
}

fn rate_window(budget: &Budget) -> RateWindow {
    let balance = match (budget.known, budget.remaining) {
        (true, Some(remaining)) => format!("{} remaining", format::usd_plain(remaining)),
        _ => "Unavailable".to_owned(),
    };
    RateWindow::with_details(
        budget.used_percent,
        budget.window_minutes,
        budget.resets_at,
        Some(format!("{} · {balance}", budget.title)),
    )
    .with_usage_known(budget.known)
}

fn details(usage: &KeyUsage) -> Vec<Option<ProviderDisplayDetail>> {
    let mut rows = vec![
        ProviderDisplayDetail::new("key", "Key", &usage.key_label)
            .and_then(|row| row.with_section_title("Aixy key")),
        ProviderDisplayDetail::new("project", "Project", &usage.project_label)
            .and_then(|row| row.with_section_title("Aixy key")),
        ProviderDisplayDetail::new(
            "observed",
            "Observed",
            usage.as_of.to_rfc3339_opts(SecondsFormat::Millis, true),
        )
        .and_then(|row| row.with_section_title("Aixy key")),
    ];
    rows.extend(usage.budgets.iter().enumerate().map(budget_row));
    match &usage.totals {
        Some(totals) => rows.extend(totals_rows(totals)),
        None => rows.push(
            ProviderDisplayDetail::new("usage-7d", "Usage (last 7 days)", "Unavailable")
                .and_then(|row| row.with_section_title("Last 7 days · this key")),
        ),
    }
    rows
}

fn budget_row((index, budget): (usize, &Budget)) -> Option<ProviderDisplayDetail> {
    let id = format!("budget-{index}");
    let (Some(remaining), Some(spent), true) = (budget.remaining, budget.spent, budget.known)
    else {
        return ProviderDisplayDetail::new(id, &budget.title, "Unavailable")
            .and_then(|row| row.with_section_title("Applicable budgets"));
    };
    let spent = if budget.hard {
        format!(
            "{} spent · {} reserved",
            format::usd_plain(spent),
            format::usd_plain(budget.reserved)
        )
    } else {
        format!("{} spent", format::usd_plain(spent))
    };
    ProviderDisplayDetail::new(
        id,
        &budget.title,
        format!(
            "{} / {} remaining",
            format::usd_plain(remaining),
            format::usd_plain(budget.limit)
        ),
    )
    .and_then(|row| row.with_secondary_value(spent))
    .and_then(|row| row.with_progress(budget.used_percent, 100.0))
    .and_then(|row| row.with_section_title("Applicable budgets"))
}

fn totals_rows(totals: &Totals) -> Vec<Option<ProviderDisplayDetail>> {
    vec![
        ProviderDisplayDetail::new(
            "requests-7d",
            "Requests (last 7 days)",
            count(totals.requests),
        )
        .and_then(|row| row.with_section_title("Last 7 days · this key")),
        ProviderDisplayDetail::new(
            "tokens-7d",
            "Tokens (last 7 days)",
            count(totals.total_tokens),
        )
        .and_then(|row| row.with_section_title("Last 7 days · this key")),
        ProviderDisplayDetail::new(
            "spend-7d",
            "Attributed spend (last 7 days)",
            totals
                .spend_usd
                .map_or_else(|| "Unavailable".to_owned(), format::usd_plain),
        )
        .and_then(|row| row.with_section_title("Last 7 days · this key")),
        ProviderDisplayDetail::new(
            "coverage-7d",
            "Cost coverage (last 7 days)",
            format!(
                "{} / {} requests",
                count(totals.attributed_requests),
                count(totals.requests)
            ),
        )
        .and_then(|row| {
            row.with_secondary_value(format!("{} partial", count(totals.partial_requests)))
        })
        .and_then(|row| row.with_section_title("Last 7 days · this key")),
    ]
}

/// Integer with thousands separators.
fn count(value: u64) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}
