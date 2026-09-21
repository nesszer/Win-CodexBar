//! Amp subscription and quota text parsing.

use crate::core::{RateWindow, UsageSnapshot};

/// Monthly pace window sentinel used by Amp subscription/pace UI (30 days).
pub(super) const AMP_MONTHLY_WINDOW_MINUTES: u32 = 30 * 24 * 60;

/// Parsed Amp subscription (legacy dual credits or current Tier allowances).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct AmpSubscriptionUsage {
    pub plan: String,
    pub reset_description: String,
    pub kind: AmpSubscriptionKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum AmpSubscriptionKind {
    Legacy {
        other_used_percent: f64,
        orb_used_percent: f64,
        resets_at: chrono::DateTime<chrono::Utc>,
    },
    Tier {
        agent: AmpAllowance,
        orb: Option<AmpAllowance>,
        period_start: Option<chrono::DateTime<chrono::Utc>>,
        resets_at: Option<chrono::DateTime<chrono::Utc>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct AmpAllowance {
    pub remaining: f64,
    pub limit: f64,
}

impl AmpAllowance {
    fn used_percent(&self) -> f64 {
        if self.limit > 0.0 {
            ((self.limit - self.remaining) / self.limit * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        }
    }
}

impl AmpSubscriptionUsage {
    pub fn other_used_percent(&self) -> f64 {
        match &self.kind {
            AmpSubscriptionKind::Legacy {
                other_used_percent, ..
            } => *other_used_percent,
            AmpSubscriptionKind::Tier { agent, .. } => agent.used_percent(),
        }
    }

    pub fn orb_used_percent(&self) -> Option<f64> {
        match &self.kind {
            AmpSubscriptionKind::Legacy {
                orb_used_percent, ..
            } => Some(*orb_used_percent),
            AmpSubscriptionKind::Tier { orb, .. } => orb.as_ref().map(AmpAllowance::used_percent),
        }
    }

    pub fn resets_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        match &self.kind {
            AmpSubscriptionKind::Legacy { resets_at, .. } => Some(*resets_at),
            AmpSubscriptionKind::Tier { resets_at, .. } => *resets_at,
        }
    }

    pub fn agent_remaining(&self) -> Option<f64> {
        match &self.kind {
            AmpSubscriptionKind::Legacy { .. } => None,
            AmpSubscriptionKind::Tier { agent, .. } => Some(agent.remaining),
        }
    }

    pub fn agent_limit(&self) -> Option<f64> {
        match &self.kind {
            AmpSubscriptionKind::Legacy { .. } => None,
            AmpSubscriptionKind::Tier { agent, .. } => Some(agent.limit),
        }
    }

    pub fn period_start(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        match &self.kind {
            AmpSubscriptionKind::Legacy { .. } => None,
            AmpSubscriptionKind::Tier { period_start, .. } => *period_start,
        }
    }

    pub fn orb_hours_remaining(&self) -> Option<f64> {
        match &self.kind {
            AmpSubscriptionKind::Legacy { .. } => None,
            AmpSubscriptionKind::Tier { orb, .. } => {
                orb.as_ref().map(|allowance| allowance.remaining)
            }
        }
    }

    pub fn orb_hours_limit(&self) -> Option<f64> {
        match &self.kind {
            AmpSubscriptionKind::Legacy { .. } => None,
            AmpSubscriptionKind::Tier { orb, .. } => orb.as_ref().map(|allowance| allowance.limit),
        }
    }
}

/// Parse Amp Free percentage lines from CLI/display text (upstream 0.42.1+ shape).
///
/// Matches lines like:
/// - `Amp Free: 72% remaining today`
/// - `Amp Free: 72% remaining (resets daily)`
///
/// Returns **used** percent (100 - remaining). The CLI fetch path passes its
/// `amp usage` output through this parser before falling back to the API path.
pub(super) fn parse_amp_free_percent_remaining(text: &str) -> Option<f64> {
    let text = text.replace("**", "");
    for line in text.lines() {
        let line = line.trim();
        let lower = line.to_ascii_lowercase();
        if !lower.starts_with("amp free:") {
            continue;
        }
        let rest = line["amp free:".len()..].trim();
        // Prefer percentage form over dollar `$used / $quota remaining`.
        let Some(percent_idx) = rest.find('%') else {
            continue;
        };
        let number_part = rest[..percent_idx].trim();
        // Reject dollar amounts mistaken for percentages (e.g. "$12 remaining").
        if number_part.contains('$') {
            continue;
        }
        let after = rest[percent_idx + 1..].trim().to_ascii_lowercase();
        if !after.starts_with("remaining") {
            continue;
        }
        let remaining: f64 = number_part.replace(',', "").parse().ok()?;
        if !remaining.is_finite() {
            continue;
        }
        let clamped = remaining.clamp(0.0, 100.0);
        return Some(100.0 - clamped);
    }
    None
}

fn normalize_amp_subscription_line(line: &str) -> String {
    let trimmed = line.trim();
    let Some(rest) = trimmed.strip_prefix("Amp ") else {
        return line.to_string();
    };
    let Some((plan, suffix)) = rest.split_once(" Subscription:") else {
        return line.to_string();
    };
    let plan = plan.trim();
    if plan.is_empty() {
        return line.to_string();
    }
    format!("Subscription {plan}:{suffix}")
}

/// Parse Amp subscription display text (Megawatt/Gigawatt dual other/orb windows).
///
/// Matches:
/// `Subscription Megawatt: 42% other usage and 88% orb usage remaining - resets upon renewal in 12 days`
/// `Subscription Gigawatt: 10% other usage and 95% orb usage remaining - resets upon renewal in 2 months`
///
/// Upstream 0.49.6 #2601: monthly (Gigawatt) renewals advance by calendar
/// month, not 30-day buckets.
pub(super) fn parse_amp_subscription_usage(
    text: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<AmpSubscriptionUsage> {
    let text = text.replace("**", "");

    // Current Amp output reports the agent allowance in dollars and the Orb
    // allowance in hours. The displayed percentages are rounded, so derive
    // both usage lanes from their exact remaining/limit values instead.
    let tier_re = regex_lite::Regex::new(
        r"(?im)^\s*Amp\s+(.+?)\s+Tier:\s*agent\s+usage\s+\$([0-9][0-9,]*(?:\.[0-9]+)?)\s+of\s+\$([0-9][0-9,]*(?:\.[0-9]+)?)\s+remaining\b(.*?)resets\s+upon\s+renewal\s+in\s+([0-9][0-9,]*)\s+(days?|months?)\b",
    )
    .ok()?;
    let orb_re = regex_lite::Regex::new(
        r"(?i)\borb\s+usage\s+([0-9][0-9,]*(?:\.[0-9]+)?)h\s+of\s+([0-9][0-9,]*(?:\.[0-9]+)?)h\s+a1\.small\s+orb\s+hours\s+remaining\b",
    )
    .ok()?;

    for line in text.lines() {
        let Some(caps) = tier_re.captures(line) else {
            continue;
        };
        let plan = caps.get(1)?.as_str().trim();
        let agent_remaining = parse_amp_number(caps.get(2)?.as_str())?;
        let agent_limit = parse_amp_number(caps.get(3)?.as_str())?;
        let details = caps.get(4)?.as_str();
        let renewal_text = caps.get(5)?.as_str().replace(',', "");
        let renewal_value = renewal_text.parse::<i64>().ok();
        let renewal_unit = caps.get(6)?.as_str().to_ascii_lowercase();
        let period = parse_amp_tier_period(details);
        let has_period_text = details.to_ascii_lowercase().contains("period ");
        let resets_at = period.map(|(_, end)| end).or_else(|| {
            (!has_period_text)
                .then(|| {
                    renewal_value
                        .and_then(|value| subscription_reset_date(value, &renewal_unit, now))
                })
                .flatten()
        });
        let reset_description = amp_renewal_description_text(&renewal_text, &renewal_unit);
        let period_start = period.map(|(start, _)| start);
        let orb = orb_re.captures(details).and_then(|orb_caps| {
            let remaining = parse_amp_number(orb_caps.get(1)?.as_str())?;
            let limit = parse_amp_number(orb_caps.get(2)?.as_str())?;
            (limit > 0.0).then_some(AmpAllowance { remaining, limit })
        });
        return Some(AmpSubscriptionUsage {
            plan: plan.to_string(),
            reset_description,
            kind: AmpSubscriptionKind::Tier {
                agent: AmpAllowance {
                    remaining: agent_remaining,
                    limit: agent_limit,
                },
                orb,
                period_start,
                resets_at,
            },
        });
    }

    let re = regex_lite::Regex::new(
        r"(?im)^\s*Subscription\s+(.+?):\s*([0-9][0-9,]*(?:\.[0-9]+)?)\s*%\s+other\s+usage\s+and\s+([0-9][0-9,]*(?:\.[0-9]+)?)\s*%\s+orb\s+usage\s+remaining\s*-\s*resets\s+upon\s+renewal\s+in\s+([0-9][0-9,]*)\s+(days?|months?)(?:\s+-\s+https?://\S+)?\s*$",
    )
    .ok()?;

    for line in text.lines() {
        let normalized_line = normalize_amp_subscription_line(line);
        let Some(caps) = re.captures(&normalized_line) else {
            continue;
        };
        let plan = caps.get(1)?.as_str().trim();
        if plan.is_empty() {
            continue;
        }
        let other_remaining = parse_amp_number(caps.get(2)?.as_str())?;
        let orb_remaining = parse_amp_number(caps.get(3)?.as_str())?;
        let renewal_value: i64 = caps.get(4)?.as_str().replace(',', "").parse().ok()?;
        if renewal_value < 0 {
            continue;
        }
        let unit = caps.get(5)?.as_str().to_ascii_lowercase();
        let resets_at = subscription_reset_date(renewal_value, &unit, now)?;
        let reset_description = amp_renewal_description(renewal_value, &unit);
        return Some(AmpSubscriptionUsage {
            plan: plan.to_string(),
            reset_description,
            kind: AmpSubscriptionKind::Legacy {
                other_used_percent: 100.0 - other_remaining.clamp(0.0, 100.0),
                orb_used_percent: 100.0 - orb_remaining.clamp(0.0, 100.0),
                resets_at,
            },
        });
    }
    None
}

fn amp_renewal_description(value: i64, unit: &str) -> String {
    let singular = if unit.starts_with("month") {
        "month"
    } else {
        "day"
    };
    if value == 1 {
        format!("renews in 1 {singular}")
    } else {
        format!("renews in {value} {singular}s")
    }
}

fn amp_renewal_description_text(value: &str, unit: &str) -> String {
    let singular = if unit.starts_with("month") {
        "month"
    } else {
        "day"
    };
    if value == "1" {
        format!("renews in 1 {singular}")
    } else {
        format!("renews in {value} {singular}s")
    }
}

fn subscription_reset_date(
    value: i64,
    unit: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    if value < 0 {
        return None;
    }
    if unit.starts_with("month") {
        add_calendar_months(now, value)
    } else {
        let seconds = value.checked_mul(24 * 60 * 60)?;
        now.checked_add_signed(chrono::Duration::seconds(seconds))
    }
}

fn parse_amp_tier_period(
    text: &str,
) -> Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)> {
    use chrono::NaiveDate;

    let re =
        regex_lite::Regex::new(r"(?i)\bperiod\s+(\d{4}-\d{2}-\d{2})\s+to\s+(\d{4}-\d{2}-\d{2})\b")
            .ok()?;
    let caps = re.captures(text)?;
    let start = NaiveDate::parse_from_str(caps.get(1)?.as_str(), "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)?
        .and_utc();
    let end = NaiveDate::parse_from_str(caps.get(2)?.as_str(), "%Y-%m-%d")
        .ok()?
        .and_hms_opt(0, 0, 0)?
        .and_utc();
    (end > start).then_some((start, end))
}

/// Add whole calendar months via chrono's calendar arithmetic, mirroring
/// upstream `Calendar.date(byAdding: .month:)` for monthly renewals.
fn add_calendar_months(
    now: chrono::DateTime<chrono::Utc>,
    months: i64,
) -> Option<chrono::DateTime<chrono::Utc>> {
    now.checked_add_months(chrono::Months::new(u32::try_from(months).ok()?))
}

/// Build a [`UsageSnapshot`] from Amp Free / subscription display text.
///
/// Subscription (Megawatt) wins for primary/secondary windows when present:
/// - primary = other usage
/// - secondary = orb usage
///
/// Free percent path fills primary when there is no subscription match.
pub(super) fn usage_snapshot_from_amp_display_text(
    text: &str,
    now: chrono::DateTime<chrono::Utc>,
) -> Option<UsageSnapshot> {
    if let Some(sub) = parse_amp_subscription_usage(text, now) {
        return Some(usage_snapshot_from_subscription(sub));
    }

    let free_used = parse_amp_free_percent_remaining(text)?;
    // Upstream 0.49.6 #2601: the Amp Free daily tier resets at 8:00 PM
    // America/New_York, not local midnight.
    let primary = RateWindow::with_details(
        free_used,
        Some(24 * 60),
        next_free_tier_reset(now),
        Some("resets daily".to_string()),
    );
    Some(UsageSnapshot::new(primary).with_login_method("Amp Free"))
}

fn usage_snapshot_from_subscription(sub: AmpSubscriptionUsage) -> UsageSnapshot {
    let AmpSubscriptionUsage {
        plan,
        reset_description,
        kind,
    } = sub;

    match kind {
        AmpSubscriptionKind::Legacy {
            other_used_percent,
            orb_used_percent,
            resets_at,
        } => {
            let window_minutes = RateWindow::monthly_window_minutes(Some(resets_at))
                .or(Some(AMP_MONTHLY_WINDOW_MINUTES));
            let primary = RateWindow::with_details(
                other_used_percent,
                window_minutes,
                Some(resets_at),
                Some(reset_description.clone()),
            );
            let secondary = RateWindow::with_details(
                orb_used_percent,
                window_minutes,
                Some(resets_at),
                Some(reset_description),
            );
            UsageSnapshot::new(primary)
                .with_secondary(secondary)
                .with_login_method(plan)
        }
        AmpSubscriptionKind::Tier {
            agent,
            orb,
            period_start,
            resets_at,
        } => {
            let window_minutes = match (period_start, resets_at) {
                (Some(start), Some(end)) => u32::try_from((end - start).num_minutes())
                    .ok()
                    .filter(|minutes| *minutes > 0),
                _ => None,
            };
            let primary = if agent.limit <= 0.0 {
                RateWindow::informational("No active Amp tier allowance")
            } else {
                RateWindow::with_details(
                    agent.used_percent(),
                    window_minutes,
                    resets_at,
                    Some(tier_allowance_description(
                        &reset_description,
                        &agent,
                        AmpAllowanceUnit::Dollars,
                    )),
                )
            };
            let mut usage = UsageSnapshot::new(primary)
                .with_login_method(plan)
                .with_primary_label("Agent usage");
            if let Some(orb) = orb {
                let secondary = RateWindow::with_details(
                    orb.used_percent(),
                    window_minutes,
                    resets_at,
                    Some(tier_allowance_description(
                        &reset_description,
                        &orb,
                        AmpAllowanceUnit::Hours,
                    )),
                );
                usage = usage
                    .with_secondary(secondary)
                    .with_secondary_label("Orb usage");
            }
            usage
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AmpAllowanceUnit {
    Dollars,
    Hours,
}

fn tier_allowance_description(
    reset_description: &str,
    allowance: &AmpAllowance,
    unit: AmpAllowanceUnit,
) -> String {
    match unit {
        AmpAllowanceUnit::Hours => format!(
            "{reset_description} · {:.2}h/{:.2}h remaining",
            allowance.remaining, allowance.limit
        ),
        AmpAllowanceUnit::Dollars => format!(
            "{reset_description} · ${:.2}/${:.2} remaining",
            allowance.remaining, allowance.limit
        ),
    }
}

/// Next 8:00 PM America/New_York boundary strictly after `now`.
fn next_free_tier_reset(
    now: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::{Datelike, TimeZone};
    let tz = chrono_tz::America::New_York;
    let local_now = now.with_timezone(&tz);
    let today = local_now.date_naive();
    let today_reset = tz
        .with_ymd_and_hms(today.year(), today.month(), today.day(), 20, 0, 0)
        .single()?
        .with_timezone(&chrono::Utc);
    if today_reset > now {
        return Some(today_reset);
    }
    let tomorrow = today + chrono::Duration::days(1);
    tz.with_ymd_and_hms(tomorrow.year(), tomorrow.month(), tomorrow.day(), 20, 0, 0)
        .single()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

fn parse_amp_number(raw: &str) -> Option<f64> {
    let value: f64 = raw.replace(',', "").parse().ok()?;
    value.is_finite().then_some(value)
}
