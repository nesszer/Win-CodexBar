//! Amp `usage` display text, shared by the CLI and the web `displayText`
//! payload. Ports upstream `AmpUsageParser.parse(displayText:)` and the
//! `AmpUsageSnapshot.toUsageSnapshot` mapping.

use regex_lite::Regex;

use crate::core::{
    ProviderDisplayDetail, ProviderError, ProviderFetchResult, RateWindow, UsageSnapshot,
};
use crate::providers::format::usd;

use super::subscription::{
    next_free_tier_reset, parse_amp_number, parse_amp_subscription_usage,
    usage_snapshot_from_subscription,
};

const AMOUNT: &str = r"([0-9][0-9,]*(?:\.[0-9]+)?)";
const CREDITS_SECTION: &str = "Credits";

/// Parsed Amp usage: quota windows plus the credit balances shown as rows.
pub(super) struct AmpDisplayUsage {
    pub usage: UsageSnapshot,
    pub details: Vec<ProviderDisplayDetail>,
}

impl AmpDisplayUsage {
    pub fn into_fetch_result(self, source_label: &str) -> ProviderFetchResult {
        ProviderFetchResult::new(self.usage, source_label).with_display_details(self.details)
    }
}

/// Amp Free allowance, from either the dollar or the percentage line.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct AmpFreeTierUsage {
    pub quota: f64,
    pub used: f64,
    pub hourly_replenishment: f64,
    pub window_hours: Option<f64>,
    pub resets_daily: bool,
}

/// Parse `amp usage` text. Every line shape upstream accepts is recognized:
/// the identity line, Amp Free in dollars (`$12.50 / $20 remaining
/// (replenishes +$0.42 / hour)`) or percent (`61% remaining today (resets
/// daily)`), the Tier and Subscription lines, individual credits and
/// workspace balances. Any one usage line is enough.
pub(super) fn parse_amp_display_text(
    text: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<AmpDisplayUsage, ProviderError> {
    let text = strip_ansi(text).replace("**", "").replace("\r\n", "\n");
    let identity = parse_identity(&text);
    if identity.is_none() && looks_signed_out(&text) {
        return Err(ProviderError::AuthRequired);
    }

    let free = parse_amp_free_tier(&text);
    let subscription = parse_amp_subscription_usage(&text, now);
    let individual_credits = parse_individual_credits(&text);
    let workspaces = parse_workspace_balances(&text);
    if free.is_none()
        && subscription.is_none()
        && individual_credits.is_none()
        && workspaces.is_empty()
    {
        return Err(ProviderError::Parse("Missing Amp usage data".to_string()));
    }

    let free_window = free.as_ref().map(|free| free_tier_window(free, now));
    let mut usage = match (subscription, free_window) {
        (Some(subscription), free_window) => {
            let usage = usage_snapshot_from_subscription(subscription);
            match free_window {
                Some(window) => usage.with_extra_rate_window("amp-free", "Amp Free", window),
                None => usage,
            }
        }
        (None, Some(window)) => UsageSnapshot::new(window).with_login_method("Amp Free"),
        (None, None) => {
            UsageSnapshot::new(RateWindow::informational("Amp credits")).with_login_method("Amp")
        }
    };
    if let Some(identity) = identity {
        if let Some(email) = identity.email {
            usage = usage.with_email(email);
        }
        if let Some(organization) = identity.organization {
            usage = usage.with_organization(organization);
        }
    }

    Ok(AmpDisplayUsage {
        usage,
        details: credit_details(individual_credits, &workspaces),
    })
}

/// Test helper: the quota snapshot only.
#[cfg(test)]
pub(super) fn usage_snapshot_from_amp_display_text(
    text: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<UsageSnapshot> {
    parse_amp_display_text(text, now)
        .ok()
        .map(|parsed| parsed.usage)
}

/// The dollar line wins over the percentage line, as upstream.
pub(super) fn parse_amp_free_tier(text: &str) -> Option<AmpFreeTierUsage> {
    parse_free_dollars(text).or_else(|| parse_free_percent(text))
}

fn parse_free_dollars(text: &str) -> Option<AmpFreeTierUsage> {
    let re = Regex::new(&format!(
        r"(?im)^\s*Amp Free:\s*\$?{AMOUNT}\s*/\s*\$?{AMOUNT}\s+remaining(?:\s*\(replenishes\s*\+\$?{AMOUNT}\s*/\s*hour\))?"
    ))
    .ok()?;
    let caps = re.captures(text)?;
    let remaining = parse_amp_number(caps.get(1)?.as_str())?;
    let quota = parse_amp_number(caps.get(2)?.as_str())?;
    let hourly_replenishment = caps
        .get(3)
        .and_then(|amount| parse_amp_number(amount.as_str()))
        .unwrap_or(0.0);
    let window_hours =
        (hourly_replenishment > 0.0).then(|| (quota / hourly_replenishment).round().max(1.0));
    Some(AmpFreeTierUsage {
        quota,
        used: (quota - remaining).max(0.0),
        hourly_replenishment,
        window_hours,
        resets_daily: false,
    })
}

fn parse_free_percent(text: &str) -> Option<AmpFreeTierUsage> {
    let re = Regex::new(&format!(
        r"(?im)^\s*Amp Free:\s*{AMOUNT}\s*%\s+remaining(?:\s+today)?(?:\s*(\(resets\s+daily\)))?"
    ))
    .ok()?;
    let caps = re.captures(text)?;
    let remaining = parse_amp_number(caps.get(1)?.as_str())?.clamp(0.0, 100.0);
    Some(AmpFreeTierUsage {
        quota: 100.0,
        used: 100.0 - remaining,
        hourly_replenishment: 0.0,
        window_hours: Some(24.0),
        resets_daily: caps.get(2).is_some(),
    })
}

fn free_tier_window(free: &AmpFreeTierUsage, now: chrono::DateTime<chrono::Utc>) -> RateWindow {
    let quota = free.quota.max(0.0);
    let used = free.used.max(0.0);
    let used_percent = if quota > 0.0 {
        (used * 100.0 / quota).min(100.0)
    } else {
        0.0
    };
    let window_minutes = free
        .window_hours
        .filter(|hours| *hours > 0.0)
        .map(|hours| (hours * 60.0).round())
        .filter(|minutes| minutes.is_finite() && *minutes <= f64::from(u32::MAX))
        .map(|minutes| {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "minutes is a finite whole number bounded by u32::MAX above"
            )]
            let minutes = minutes as u32;
            minutes
        });
    // Daily percentage usage resets at the fixed New York boundary; the
    // dollar allowance refills hourly, so it is full again after used/rate.
    let resets_at = if free.resets_daily {
        next_free_tier_reset(now)
    } else if quota > 0.0 && free.hourly_replenishment > 0.0 {
        let seconds = (used / free.hourly_replenishment * 3600.0).max(0.0);
        std::time::Duration::try_from_secs_f64(seconds)
            .ok()
            .and_then(|duration| chrono::TimeDelta::from_std(duration).ok())
            .and_then(|delta| now.checked_add_signed(delta))
    } else {
        None
    };
    RateWindow::with_details(
        used_percent,
        window_minutes,
        resets_at,
        free.resets_daily.then(|| "resets daily".to_string()),
    )
}

struct AmpIdentity {
    email: Option<String>,
    organization: Option<String>,
}

fn parse_identity(text: &str) -> Option<AmpIdentity> {
    let re = Regex::new(r"(?im)^\s*Signed in as\s+([^\s(]+)(?:\s+\(([^\r\n)]+)\))?\s*$").ok()?;
    let caps = re.captures(text)?;
    let capture = |index| {
        caps.get(index)
            .map(|value| value.as_str().trim().to_string())
            .filter(|value| !value.is_empty())
    };
    Some(AmpIdentity {
        email: capture(1),
        organization: capture(2),
    })
}

fn parse_individual_credits(text: &str) -> Option<f64> {
    let re = Regex::new(&format!(
        r"(?im)^\s*Individual credits:\s*\$?{AMOUNT}\s+remaining"
    ))
    .ok()?;
    parse_amp_number(re.captures(text)?.get(1)?.as_str())
}

fn parse_workspace_balances(text: &str) -> Vec<(String, f64)> {
    let Ok(re) = Regex::new(&format!(
        r"(?im)^\s*Workspace\s+(.+?):\s*\$?{AMOUNT}\s+remaining"
    )) else {
        return Vec::new();
    };
    re.captures_iter(text)
        .filter_map(|caps| {
            let name = caps.get(1)?.as_str().trim();
            let remaining = parse_amp_number(caps.get(2)?.as_str())?;
            (!name.is_empty()).then(|| (name.to_string(), remaining))
        })
        .collect()
}

fn credit_details(
    individual_credits: Option<f64>,
    workspaces: &[(String, f64)],
) -> Vec<ProviderDisplayDetail> {
    let individual = individual_credits.and_then(|remaining| {
        ProviderDisplayDetail::new("amp-credits-individual", "Individual", usd(remaining))?
            .with_section_title(CREDITS_SECTION)?
            .with_secondary_value("For agent and orb usage")
    });
    let workspaces = workspaces
        .iter()
        .enumerate()
        .filter_map(|(index, (name, remaining))| {
            ProviderDisplayDetail::new(
                format!("amp-credits-workspace-{index}"),
                format!("Workspace {name}"),
                usd(*remaining),
            )?
            .with_section_title(CREDITS_SECTION)
        });
    individual.into_iter().chain(workspaces).collect()
}

fn looks_signed_out(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("sign in") || lower.contains("log in") || lower.contains("login")
}

fn strip_ansi(text: &str) -> String {
    match Regex::new(r"\x1B\[[0-9;?]*[ -/]*[@-~]") {
        Ok(re) => re.replace_all(text, "").into_owned(),
        Err(_) => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};

    /// `amp usage` output from the Mac-parity rig's Amp pack (synthetic values).
    const PARITY_CLI_OUTPUT: &str = include_str!("../fixtures/amp/parity-cli-usage.txt");

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(seconds, 0).unwrap()
    }

    fn parse(text: &str, now: DateTime<Utc>) -> AmpDisplayUsage {
        parse_amp_display_text(text, now).expect("parsed Amp usage")
    }

    fn detail<'a>(parsed: &'a AmpDisplayUsage, title: &str) -> Option<&'a ProviderDisplayDetail> {
        parsed.details.iter().find(|row| row.title() == title)
    }

    fn detail_value<'a>(parsed: &'a AmpDisplayUsage, title: &str) -> Option<&'a str> {
        detail(parsed, title).map(ProviderDisplayDetail::value)
    }

    #[test]
    fn parity_pack_cli_output_parses_free_dollars_identity_and_credits() {
        let now = Utc.with_ymd_and_hms(2026, 10, 10, 12, 0, 0).unwrap();
        let lf = PARITY_CLI_OUTPUT.replace("\r\n", "\n");
        // The Windows fake CLI (`amp.cmd`) echoes CRLF lines; the Mac one prints LF.
        for text in [lf.clone(), lf.replace('\n', "\r\n")] {
            let parsed = super::super::cli::usage_from_amp_cli_output(&text, now)
                .expect("pack output parses");
            let usage = &parsed.usage;

            // $12.50 of $20 remaining: $7.50 used.
            assert!((usage.primary.used_percent - 37.5).abs() < 1e-9);
            // 20 / 0.42 = 47.6, rounded to 48 hours.
            assert_eq!(usage.primary.window_minutes, Some(48 * 60));
            // Full again after 7.50 / 0.42 hours = 17h 51m 25s.
            let reset = usage.primary.resets_at.expect("replenishment reset");
            assert_eq!((reset - now).num_seconds(), 64_285);
            assert_eq!(usage.primary.reset_description, None);
            assert!(usage.secondary.is_none());
            assert_eq!(usage.login_method.as_deref(), Some("Amp Free"));
            assert_eq!(
                usage.account_email.as_deref(),
                Some("parity.user@example.com")
            );
            assert_eq!(usage.account_organization.as_deref(), Some("Parity Labs"));

            let individual = detail(&parsed, "Individual").expect("individual credits");
            assert_eq!(individual.value(), "$5.00");
            assert_eq!(individual.section_title(), Some("Credits"));
            assert_eq!(
                individual.secondary_value(),
                Some("For agent and orb usage")
            );
            assert_eq!(
                detail_value(&parsed, "Workspace Parity Workspace"),
                Some("$3.00")
            );
            assert_eq!(parsed.details.len(), 2);

            let result = parsed.into_fetch_result("cli");
            assert_eq!(result.display_details().len(), 2);
            assert_eq!(result.source_label, "cli");
        }
    }

    #[test]
    fn parses_current_display_text_with_ansi_and_links() {
        let now = at(1_700_000_000);
        let text = "\u{1B}[2mSigned in as ampcode@3kh0.net (echo)\u{1B}[0m\n\
Amp Free: $4.71/$10 remaining (replenishes +$0.42/hour) - https://ampcode.com/settings#amp-free\n\
Individual credits: $25.64 remaining (set up automatic top-up to avoid running out) - https://ampcode.com/settings\n\
Workspace meow: $10.22 remaining (set up automatic top-up to avoid running out) - https://ampcode.com/workspaces/meow\n";

        let parsed = parse(text, now);
        assert!((parsed.usage.primary.used_percent - 52.9).abs() < 1e-6);
        assert_eq!(parsed.usage.primary.window_minutes, Some(24 * 60));
        assert_eq!(
            parsed.usage.account_email.as_deref(),
            Some("ampcode@3kh0.net")
        );
        assert_eq!(parsed.usage.account_organization.as_deref(), Some("echo"));
        assert_eq!(detail_value(&parsed, "Individual"), Some("$25.64"));
        assert_eq!(detail_value(&parsed, "Workspace meow"), Some("$10.22"));
    }

    #[test]
    fn percentage_free_line_resets_daily_only_when_stated() {
        let now = at(1_700_000_000);
        let parsed = parse(
            "Signed in as user@example.com (example)\n\
Amp Free: 61% remaining today (resets daily) - https://ampcode.com/settings#amp-free\n\
Individual credits: $9.86 remaining (set up automatic top-up to avoid running out)\n\
Workspace example: $5.33 remaining (set up automatic top-up to avoid running out)\n",
            now,
        );
        let primary = &parsed.usage.primary;
        assert_eq!(primary.used_percent, 39.0);
        assert_eq!(primary.window_minutes, Some(1440));
        assert_eq!(
            primary.resets_at,
            Some(Utc.with_ymd_and_hms(2023, 11, 15, 1, 0, 0).unwrap())
        );
        assert_eq!(primary.reset_description.as_deref(), Some("resets daily"));
        assert_eq!(detail_value(&parsed, "Individual"), Some("$9.86"));
        assert_eq!(detail_value(&parsed, "Workspace example"), Some("$5.33"));

        let plain = parse(
            "Signed in as user@example.com\nAmp Free: 61% remaining",
            now,
        );
        assert_eq!(plain.usage.primary.used_percent, 39.0);
        assert_eq!(plain.usage.primary.resets_at, None);
        assert_eq!(plain.usage.primary.reset_description, None);
    }

    #[test]
    fn dollar_free_line_wins_over_percentage_line() {
        let now = at(1_700_000_000);
        let parsed = parse(
            "Signed in as user@example.com\n\
Amp Free: $6/$10 remaining (replenishes +$0.5/hour)\n\
Amp Free: 61% remaining today (resets daily)\n",
            now,
        );
        let primary = &parsed.usage.primary;
        assert_eq!(primary.used_percent, 40.0);
        assert_eq!(primary.resets_at, Some(now + chrono::Duration::hours(8)));
        assert_eq!(primary.reset_description, None);
    }

    #[test]
    fn bold_subscription_keeps_free_tier_as_extra_window() {
        let now = Utc.with_ymd_and_hms(2026, 8, 24, 12, 0, 0).unwrap();
        let parsed = parse(
            "Signed in as you@example.com (name)\n\
**Amp Free:** 0% remaining today (resets daily) - https://ampcode.com/settings\n\
**Amp Megawatt Subscription:** 68% other usage and 97% orb usage remaining - resets upon renewal in 5 days\n\
**Individual credits:** $3.23 remaining (set up auto-reload to avoid running out) - https://ampcode.com/settings\n",
            now,
        );
        let usage = &parsed.usage;
        assert_eq!(usage.primary.used_percent, 32.0);
        assert_eq!(
            usage.secondary.as_ref().map(|window| window.used_percent),
            Some(3.0)
        );
        assert_eq!(usage.login_method.as_deref(), Some("Megawatt"));
        assert_eq!(usage.account_organization.as_deref(), Some("name"));
        let free = &usage.extra_rate_windows[0];
        assert_eq!(
            (free.id.as_str(), free.title.as_str()),
            ("amp-free", "Amp Free")
        );
        assert_eq!(free.window.used_percent, 100.0);
        assert_eq!(detail_value(&parsed, "Individual"), Some("$3.23"));
    }

    #[test]
    fn bold_legacy_free_and_workspace_labels() {
        let parsed = parse(
            "Signed in as user@example.com (team)\n\
**Amp Free:** $6/$10 remaining (replenishes +$0.5/hour)\n\
**Workspace Test Team:** $7.25 remaining\n",
            at(1_700_000_000),
        );
        assert_eq!(parsed.usage.primary.used_percent, 40.0);
        assert_eq!(parsed.usage.primary.window_minutes, Some(20 * 60));
        assert_eq!(detail_value(&parsed, "Workspace Test Team"), Some("$7.25"));
    }

    #[test]
    fn credits_without_usage_window_still_parse() {
        let parsed = parse(
            "Signed in as paid@example.com\nIndividual credits: $25.64 remaining",
            at(1_700_000_000),
        );
        assert!(parsed.usage.primary.is_informational);
        assert!(parsed.usage.secondary.is_none());
        assert_eq!(parsed.usage.login_method.as_deref(), Some("Amp"));
        assert_eq!(detail_value(&parsed, "Individual"), Some("$25.64"));

        let workspaces = parse(
            "Signed in as workspace@example.com (team)\n\
Workspace Alpha Team: $1,234.56 remaining\n\
Workspace Beta: $7 remaining\n",
            at(1_700_000_000),
        );
        assert!(workspaces.usage.primary.is_informational);
        assert_eq!(
            detail_value(&workspaces, "Workspace Alpha Team"),
            Some("$1,234.56")
        );
        assert_eq!(detail_value(&workspaces, "Workspace Beta"), Some("$7.00"));
    }

    #[test]
    fn identity_containing_login_is_not_signed_out() {
        let parsed = parse(
            "Signed in as login@example.com (login-team)\n\
Amp Free: $6/$10 remaining (replenishes +$0.5/hour)\n",
            at(1_700_000_000),
        );
        assert_eq!(
            parsed.usage.account_email.as_deref(),
            Some("login@example.com")
        );
        assert_eq!(
            parsed.usage.account_organization.as_deref(),
            Some("login-team")
        );
    }

    #[test]
    fn signed_out_and_unrecognized_output_are_errors() {
        let now = at(1_700_000_000);
        assert!(matches!(
            parse_amp_display_text("Please sign in to Amp.", now),
            Err(ProviderError::AuthRequired)
        ));
        assert!(matches!(
            parse_amp_display_text("Amp usage is not available right now", now),
            Err(ProviderError::Parse(_))
        ));
        assert!(matches!(
            super::super::cli::usage_from_amp_cli_output("unexpected", now),
            Err(ProviderError::Parse(message)) if message == "Amp CLI returned unrecognized usage output"
        ));
    }

    #[test]
    fn replenishment_overflow_keeps_free_usage_without_reset() {
        for hourly in ["0.000000000000000001", "0.0000000000000000000000000000001"] {
            let parsed = parse(
                &format!("Amp Free: $0.5/$1 remaining (replenishes +${hourly}/hour)"),
                at(1_700_000_000),
            );
            assert_eq!(parsed.usage.primary.used_percent, 50.0);
            assert_eq!(parsed.usage.primary.window_minutes, None);
            assert_eq!(parsed.usage.primary.resets_at, None);
        }
    }

    #[test]
    fn subscription_renewal_overflow_keeps_credits() {
        for unit in ["days", "months"] {
            let text = format!(
                "Subscription Pro: 50% other usage and 50% orb usage remaining - \
resets upon renewal in {} {unit}\nIndividual credits: $12 remaining\n",
                i64::MAX
            );
            let parsed = parse(&text, at(1_700_000_000));
            assert!(parsed.usage.primary.is_informational);
            assert!(parsed.usage.secondary.is_none());
            assert_eq!(detail_value(&parsed, "Individual"), Some("$12.00"));
        }
    }
}
