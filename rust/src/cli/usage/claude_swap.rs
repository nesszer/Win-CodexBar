use serde::Serialize;

use super::{
    UsageCommand, UsageOutput, UsageOutputFormat, append_status_line, fetch_provider_json_output,
    fetch_provider_text_output, format_percent, render_status_indicator, render_text_error,
    status_json,
};
use crate::core::ProviderId;
use crate::providers::claude::claude_swap::{
    self, ClaudeSwapAccount, ClaudeSwapAccountAction, ClaudeSwapHistoricalUsageDto,
    ClaudeSwapScopedWindowDto, ClaudeSwapSpendWindowDto, ClaudeSwapUsageWindowDto,
};
use crate::settings::Settings;
use crate::status::{ProviderStatus as StatusInfo, fetch_provider_status};
/// Settings-derived context for the read-only claude-swap `--all-accounts` path.
struct ClaudeSwapCliContext {
    executable_path: String,
    hide_personal_info: bool,
}

/// Returns the adapter context only when the opt-in integration is enabled and
/// configured; otherwise `--all-accounts` behaves like a normal single fetch.
fn claude_swap_cli_context() -> Option<ClaudeSwapCliContext> {
    let settings = Settings::load();
    if !settings.claude_swap_enabled() {
        return None;
    }
    let executable_path = settings.claude_swap_executable_path().trim().to_string();
    if executable_path.is_empty() {
        return None;
    }
    Some(ClaudeSwapCliContext {
        executable_path,
        hide_personal_info: settings.hide_personal_info,
    })
}

/// Read and project the external accounts. Raw cswap output never escapes this
/// boundary: only the allow-listed projection is returned, and failures become
/// plain error strings (already sanitized by the adapter).
fn read_claude_swap_accounts(ctx: &ClaudeSwapCliContext) -> Result<Vec<ClaudeSwapAccount>, String> {
    claude_swap::read_account_list(&ctx.executable_path)
        .map(|list| claude_swap::project_accounts(&list, ctx.hide_personal_info))
        .map_err(|error| error.to_string())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeSwapCliAccount<'a> {
    id: &'a str,
    slot: u32,
    label: &'a str,
    email: Option<&'a str>,
    organization: Option<&'a str>,
    alias: Option<&'a str>,
    is_active: bool,
    action: Option<ClaudeSwapAccountAction>,
    is_disabled: bool,
    status: &'a str,
    error: Option<&'a str>,
    five_hour: Option<&'a ClaudeSwapUsageWindowDto>,
    seven_day: Option<&'a ClaudeSwapUsageWindowDto>,
    scoped: &'a [ClaudeSwapScopedWindowDto],
    spend: Option<&'a ClaudeSwapSpendWindowDto>,
    historical_usage: Option<&'a ClaudeSwapHistoricalUsageDto>,
}

impl<'a> From<&'a ClaudeSwapAccount> for ClaudeSwapCliAccount<'a> {
    fn from(account: &'a ClaudeSwapAccount) -> Self {
        Self {
            id: &account.id,
            slot: account.slot,
            label: &account.label,
            email: account.email.as_deref(),
            organization: account.organization.as_deref(),
            alias: account.alias.as_deref(),
            is_active: account.is_active,
            action: account.action,
            is_disabled: account.is_disabled,
            status: &account.status,
            error: account.error.as_deref(),
            five_hour: account.five_hour.as_ref(),
            seven_day: account.seven_day.as_ref(),
            scoped: &account.scoped,
            spend: account.spend.as_ref(),
            historical_usage: account.historical_usage.as_ref(),
        }
    }
}

pub(super) fn claude_swap_json_payload(
    account: &ClaudeSwapAccount,
    status: Option<&StatusInfo>,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "provider": ProviderId::Claude.cli_name(),
        "source": "claude-swap",
        "account": ClaudeSwapCliAccount::from(account),
    });
    if let Some(status) = status {
        payload["status"] = status_json(status);
    }
    payload
}

fn claude_swap_windows(account: &ClaudeSwapAccount) -> Vec<String> {
    window_labels(
        account.five_hour.as_ref(),
        account.seven_day.as_ref(),
        &account.scoped,
    )
}

/// `Session N% | Weekly N% | <scoped> N%` parts, skipping absent windows.
fn window_labels(
    five_hour: Option<&ClaudeSwapUsageWindowDto>,
    seven_day: Option<&ClaudeSwapUsageWindowDto>,
    scoped: &[ClaudeSwapScopedWindowDto],
) -> Vec<String> {
    let mut windows = Vec::new();
    if let Some(window) = five_hour {
        windows.push(format!("Session {}", format_percent(window.used_percent)));
    }
    if let Some(window) = seven_day {
        windows.push(format!("Weekly {}", format_percent(window.used_percent)));
    }
    for window in scoped {
        windows.push(format!(
            "{} {}",
            window.name,
            format_percent(window.used_percent)
        ));
    }
    windows
}

fn claude_swap_spend_line(
    spend: Option<&crate::providers::claude::claude_swap::ClaudeSwapSpendWindowDto>,
) -> Option<String> {
    spend.map(|spend| {
        format!(
            "Spend {:.2}/{:.2} {} ({})",
            spend.used,
            spend.limit,
            spend.currency_code.as_deref().unwrap_or(""),
            format_percent(spend.used_percent)
        )
    })
}

fn claude_swap_historical_windows(historical: &ClaudeSwapHistoricalUsageDto) -> Vec<String> {
    window_labels(
        historical.five_hour.as_ref(),
        historical.seven_day.as_ref(),
        &historical.scoped,
    )
}

pub(super) fn render_claude_swap_text(
    account: &ClaudeSwapAccount,
    status: Option<&StatusInfo>,
    use_color: bool,
) -> String {
    let mut lines = Vec::new();
    let active = if account.is_active { " (active)" } else { "" };
    let status_indicator = render_status_indicator(status, use_color);
    lines.push(format!(
        "{} (claude-swap)  {}{}{}",
        ProviderId::Claude.display_name(),
        account.label,
        active,
        status_indicator
    ));
    append_status_line(&mut lines, status);
    let windows = claude_swap_windows(account);
    if !windows.is_empty() {
        lines.push(format!("  {}", windows.join(" | ")));
    }
    if let Some(spend) = claude_swap_spend_line(account.spend.as_ref()) {
        lines.push(format!("  {spend}"));
    }
    if let Some(historical) = &account.historical_usage {
        let windows = claude_swap_historical_windows(historical);
        let spend = claude_swap_spend_line(historical.spend.as_ref());
        let details = [(!windows.is_empty()).then(|| windows.join(" | ")), spend]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if !details.is_empty() {
            lines.push(format!(
                "  Last known usage (captured {}): {}",
                historical.fetched_at.to_rfc3339(),
                details.join(" | ")
            ));
        }
    }
    if account.is_disabled {
        lines.push("  Disabled by claude-swap.".to_string());
    }
    if let Some(error) = &account.error {
        lines.push(format!("  {error}"));
    }
    lines.join("\n")
}

pub(super) fn render_claude_swap_brief(
    accounts: &[ClaudeSwapAccount],
    status: Option<&StatusInfo>,
    use_color: bool,
) -> String {
    let status_indicator = render_status_indicator(status, use_color);
    let mut account_parts = Vec::new();
    for account in accounts {
        let active = if account.is_active { " (active)" } else { "" };
        let windows = claude_swap_windows(account);
        let suffix = if windows.is_empty() {
            account
                .error
                .as_deref()
                .map(|error| format!(" | {error}"))
                .unwrap_or_default()
        } else {
            format!(" | {}", windows.join(" | "))
        };
        account_parts.push(format!("{}{}{}", account.label, active, suffix));
    }
    let accounts_text = if account_parts.is_empty() {
        "no accounts".to_string()
    } else {
        account_parts.join(" | ")
    };
    let provider_status = status
        .map(|status| format!(" | Status {}", status.description))
        .unwrap_or_default();
    format!(
        "{} (claude-swap){}: {}{}",
        ProviderId::Claude.display_name(),
        status_indicator,
        accounts_text,
        provider_status
    )
}

async fn claude_swap_provider_status(command: &UsageCommand) -> Option<StatusInfo> {
    if command.fetch_status {
        fetch_provider_status(ProviderId::Claude.cli_name()).await
    } else {
        None
    }
}

/// True when the provider should be expanded through the claude-swap adapter.
fn uses_claude_swap_all_accounts(command: &UsageCommand, provider_id: ProviderId) -> bool {
    command.all_accounts && provider_id == ProviderId::Claude
}

async fn collect_json_results(command: &UsageCommand) -> Vec<serde_json::Value> {
    let mut results = Vec::new();
    for provider_id in &command.providers {
        if uses_claude_swap_all_accounts(command, *provider_id)
            && let Some(ctx) = claude_swap_cli_context()
        {
            let status = claude_swap_provider_status(command).await;
            match read_claude_swap_accounts(&ctx) {
                Ok(accounts) => {
                    results.extend(
                        accounts
                            .iter()
                            .map(|account| claude_swap_json_payload(account, status.as_ref())),
                    );
                }
                Err(error) => results.push(claude_swap_error_payload(&error, status.as_ref())),
            }
            continue;
        }
        results.push(fetch_provider_json_output(*provider_id, command).await);
    }
    results
}

/// JSON row for a failed claude-swap account read. Like every usage error
/// row it carries `errorKind`; an external tool failure is `unknown`.
fn claude_swap_error_payload(error: &str, status: Option<&StatusInfo>) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "provider": ProviderId::Claude.cli_name(),
        "source": "claude-swap",
        "error": error,
        "errorKind": crate::cli::error_kind::ERROR_KIND_UNKNOWN,
    });
    if let Some(status) = status {
        payload["status"] = status_json(status);
    }
    payload
}

pub(super) async fn collect_usage_output(command: &UsageCommand) -> UsageOutput {
    match command.format {
        UsageOutputFormat::Text => {
            let mut sections = Vec::new();
            for provider_id in &command.providers {
                if uses_claude_swap_all_accounts(command, *provider_id)
                    && let Some(ctx) = claude_swap_cli_context()
                {
                    let status = claude_swap_provider_status(command).await;
                    match read_claude_swap_accounts(&ctx) {
                        Ok(accounts) => {
                            if command.brief {
                                sections.push(render_claude_swap_brief(
                                    &accounts,
                                    status.as_ref(),
                                    command.use_color,
                                ));
                            } else {
                                sections.extend(accounts.iter().map(|account| {
                                    render_claude_swap_text(
                                        account,
                                        status.as_ref(),
                                        command.use_color,
                                    )
                                }));
                            }
                        }
                        Err(error) => {
                            let mut text =
                                render_text_error(ProviderId::Claude, &error, command.use_color);
                            if let Some(status) = status.as_ref() {
                                text.push_str(&format!(" | Status {}", status.description));
                            }
                            sections.push(text);
                        }
                    }
                    continue;
                }
                sections.push(fetch_provider_text_output(*provider_id, command).await);
            }
            UsageOutput::Text(sections)
        }
        UsageOutputFormat::Json => UsageOutput::Json {
            results: collect_json_results(command).await,
            pretty: command.pretty,
        },
        UsageOutputFormat::Toon => UsageOutput::Toon(collect_json_results(command).await),
    }
}

#[cfg(test)]
mod tests {
    use super::claude_swap_error_payload;
    use serde_json::json;

    #[test]
    fn claude_swap_error_rows_carry_an_error_kind() {
        assert_eq!(
            claude_swap_error_payload("claude-swap exited with status 2", None),
            json!({
                "provider": "claude",
                "source": "claude-swap",
                "error": "claude-swap exited with status 2",
                "errorKind": "unknown",
            })
        );
    }
}
