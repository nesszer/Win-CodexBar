//! Cursor API client for fetching usage information
//!
//! Uses browser cookies to authenticate with cursor.com API

use crate::core::{CostSnapshot, NamedRateWindow, ProviderError, RateWindow};
use crate::providers::browser_cookie_header;
use chrono::{DateTime, Utc};
use serde::{Deserialize, de::DeserializeOwned};
use std::collections::HashSet;
use std::time::{Duration, Instant};

const BASE_URL: &str = "https://cursor.com";
const COOKIE_DOMAINS: [&str; 2] = ["cursor.com", "cursor.sh"];
const TEAM_LOOKUP_TIMEOUT: Duration = Duration::from_secs(10);
const TEAM_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const TEAM_PAGE_SIZE: usize = 50;
const MAX_TEAM_PAGES: u32 = 20;

#[derive(Debug)]
pub struct CursorUsageResult {
    pub(super) primary: RateWindow,
    pub(super) secondary: Option<RateWindow>,
    pub(super) model_specific: Option<RateWindow>,
    pub(super) cost: Option<CostSnapshot>,
    pub(super) email: Option<String>,
    pub(super) plan_type: Option<String>,
    pub(super) grok_bot: Option<NamedRateWindow>,
}

/// Cursor API client
pub struct CursorApi {
    client: reqwest::Client,
}

impl CursorApi {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub(super) fn client(&self) -> &reqwest::Client {
        &self.client
    }

    /// Fetch usage information from Cursor API
    pub async fn fetch_usage(&self) -> Result<CursorUsageResult, ProviderError> {
        // Try to get cookies from browser
        let cookie_header = self.get_cookie_header()?;
        self.fetch_usage_with_cookie_header(&cookie_header).await
    }

    /// Fetch usage information with an already resolved Cookie header.
    pub async fn fetch_usage_with_cookie_header(
        &self,
        cookie_header: &str,
    ) -> Result<CursorUsageResult, ProviderError> {
        // Fetch usage summary and user info in parallel
        let (usage_result, user_result, sand_result) = tokio::join!(
            self.fetch_usage_summary(cookie_header),
            self.fetch_user_info(cookie_header),
            self.fetch_sand_usage(cookie_header)
        );

        let usage_summary = usage_result?;
        let user_info = user_result.ok();
        let team_budget = if usage_summary.is_team_plan() {
            if let Some(email) = user_info.as_ref().and_then(UserInfo::verified_email) {
                // Team spend is optional enrichment. A failed or ambiguous lookup must
                // leave the already-valid usage-summary result available.
                self.fetch_team_budget(cookie_header, email)
                    .await
                    .ok()
                    .flatten()
            } else {
                None
            }
        } else {
            None
        };
        let mut result =
            self.build_result_with_team_budget(usage_summary, user_info, team_budget)?;
        result.grok_bot = sand_result.ok().flatten();
        Ok(result)
    }

    fn get_cookie_header(&self) -> Result<String, ProviderError> {
        browser_cookie_header(&COOKIE_DOMAINS)
    }

    async fn fetch_usage_summary(
        &self,
        cookie_header: &str,
    ) -> Result<UsageSummary, ProviderError> {
        let url = format!("{}/api/usage-summary", BASE_URL);

        let response = self
            .client
            .get(&url)
            .header("Cookie", cookie_header)
            .header("Accept", "application/json")
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await?;

        if response.status() == 401 || response.status() == 403 {
            return Err(ProviderError::AuthRequired);
        }

        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Cursor API returned {}",
                response.status()
            )));
        }

        // Try structured deserialization first, fall back to raw JSON on failure
        let text = response
            .text()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        serde_json::from_str::<UsageSummary>(&text).map_err(|e| {
            tracing::warn!(
                "Cursor usage-summary parse error: {e}; response length: {} bytes",
                text.len()
            );
            ProviderError::Parse(e.to_string())
        })
    }

    async fn fetch_sand_usage(
        &self,
        cookie_header: &str,
    ) -> Result<Option<NamedRateWindow>, ProviderError> {
        let url = format!("{}/api/dashboard/get-sand-usage-status", BASE_URL);
        let response = self
            .client
            .post(&url)
            .header("Cookie", cookie_header)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Origin", BASE_URL)
            .body("{}")
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let status: SandUsageStatus = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))?;
        Ok(status.to_window(Utc::now()))
    }

    async fn fetch_user_info(&self, cookie_header: &str) -> Result<UserInfo, ProviderError> {
        let url = format!("{}/api/auth/me", BASE_URL);

        let response = self
            .client
            .get(&url)
            .header("Cookie", cookie_header)
            .header("Accept", "application/json")
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(ProviderError::Other(
                "Failed to fetch user info".to_string(),
            ));
        }

        response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(e.to_string()))
    }

    async fn fetch_team_budget(
        &self,
        cookie_header: &str,
        email: &str,
    ) -> Result<Option<CursorMemberBudget>, ProviderError> {
        let deadline = Instant::now() + TEAM_LOOKUP_TIMEOUT;
        let teams: CursorTeams = self
            .fetch_team_dashboard("teams", serde_json::json!({}), cookie_header, deadline)
            .await?;
        let Some(team_id) = teams.selected_id(cookie_header) else {
            return Ok(None);
        };

        let mut expected_pages = None;
        let mut candidate = None;
        for page in 1..=MAX_TEAM_PAGES {
            let spend: CursorTeamSpend = self
                .fetch_team_dashboard(
                    "get-team-spend",
                    serde_json::json!({
                        "teamId": team_id,
                        "page": page,
                        "pageSize": TEAM_PAGE_SIZE,
                        "sortBy": "name",
                        "sortDirection": "asc"
                    }),
                    cookie_header,
                    deadline,
                )
                .await?;
            let Some(total_pages) = spend
                .total_pages
                .filter(|pages| (1..=MAX_TEAM_PAGES).contains(pages))
            else {
                return Ok(None);
            };
            if expected_pages.is_some_and(|expected| expected != total_pages)
                || spend.team_member_spend.is_empty()
                || spend.team_member_spend.len() > TEAM_PAGE_SIZE
                || (page != total_pages && spend.team_member_spend.len() != TEAM_PAGE_SIZE)
            {
                return Ok(None);
            }
            expected_pages = Some(total_pages);

            match select_member_budget(&spend.team_member_spend, email) {
                Err(()) => return Ok(None),
                Ok(Some(page_candidate)) => {
                    if candidate.is_some() {
                        return Ok(None);
                    }
                    candidate = Some(page_candidate);
                }
                Ok(None) => {}
            }

            if page == total_pages {
                return Ok(candidate);
            }
        }
        Ok(None)
    }

    async fn fetch_team_dashboard<Response: DeserializeOwned>(
        &self,
        endpoint: &str,
        body: serde_json::Value,
        cookie_header: &str,
        deadline: Instant,
    ) -> Result<Response, ProviderError> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(ProviderError::Timeout);
        }
        let response = self
            .client
            .post(format!("{BASE_URL}/api/dashboard/{endpoint}"))
            .header("Cookie", cookie_header)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Origin", BASE_URL)
            .header("Referer", format!("{BASE_URL}/dashboard"))
            .json(&body)
            .timeout(remaining.min(TEAM_REQUEST_TIMEOUT))
            .send()
            .await?;
        if response.status() == 401 || response.status() == 403 {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Cursor team API returned {}",
                response.status()
            )));
        }
        response
            .json()
            .await
            .map_err(|error| ProviderError::Parse(error.to_string()))
    }

    fn build_result(
        &self,
        summary: UsageSummary,
        user_info: Option<UserInfo>,
    ) -> Result<CursorUsageResult, ProviderError> {
        self.build_result_with_team_budget(summary, user_info, None)
    }

    fn build_result_with_team_budget(
        &self,
        summary: UsageSummary,
        user_info: Option<UserInfo>,
        team_budget: Option<CursorMemberBudget>,
    ) -> Result<CursorUsageResult, ProviderError> {
        let billing_end = summary
            .billing_cycle_end
            .as_ref()
            .and_then(|s| parse_iso_date(s));

        let (percent_used, secondary, model_specific, cost_snapshot) =
            if let Some(team_budget) = team_budget.filter(|_| summary.is_team_plan()) {
                let percent = clamp_percent(team_budget.used_usd / team_budget.limit_usd * 100.0);
                let cost = Self::on_demand_cost(
                    summary
                        .individual_usage
                        .as_ref()
                        .and_then(|individual| individual.on_demand.as_ref()),
                    billing_end,
                )
                .or_else(|| {
                    summary
                        .team_usage
                        .as_ref()
                        .and_then(|team| Self::on_demand_cost(team.on_demand.as_ref(), billing_end))
                })
                .or_else(|| {
                    Some(Self::plan_cost(
                        team_budget.used_usd,
                        team_budget.limit_usd,
                        summary.billing_cycle_start.as_deref(),
                        billing_end,
                    ))
                });
                (percent, None, None, cost)
            } else if let Some(individual) = &summary.individual_usage {
                if let Some(plan) = &individual.plan {
                    let used_cents = plan.used.unwrap_or(0) as f64;
                    let limit_cents = plan
                        .limit
                        .or_else(|| plan.breakdown.as_ref().and_then(|b| b.total))
                        .unwrap_or(0) as f64;

                    // Upstream #2255: clamp plan usage at 100% when included usage
                    // exceeds the plan limit (overage must not paint >100% bars).
                    let percent = if let Some(percent) = plan.total_percent_used {
                        clamp_percent(percent)
                    } else if limit_cents > 0.0 {
                        clamp_percent((used_cents / limit_cents) * 100.0)
                    } else {
                        0.0
                    };

                    let secondary = plan.auto_percent_used.map(|v| {
                        RateWindow::with_details(clamp_percent(v), None, billing_end, None)
                    });

                    let model_specific = plan.api_percent_used.map(|v| {
                        RateWindow::with_details(clamp_percent(v), None, billing_end, None)
                    });

                    let cost = Self::on_demand_cost(individual.on_demand.as_ref(), billing_end)
                        .or_else(|| {
                            summary.team_usage.as_ref().and_then(|team| {
                                Self::on_demand_cost(team.on_demand.as_ref(), billing_end)
                            })
                        })
                        .unwrap_or_else(|| {
                            // Plan-included spend (cents → USD) when on-demand is off.
                            Self::plan_cost(
                                used_cents / 100.0,
                                limit_cents / 100.0,
                                summary.billing_cycle_start.as_deref(),
                                billing_end,
                            )
                        });

                    (percent, secondary, model_specific, Some(cost))
                } else if let Some(overall) = &individual.overall {
                    let percent = Self::usage_percent(overall).unwrap_or(0.0);
                    let cost = Self::on_demand_cost(Some(overall), billing_end);
                    (percent, None, None, cost)
                } else {
                    (0.0, None, None, None)
                }
            } else if let Some(team) = &summary.team_usage {
                if let Some(pooled) = &team.pooled {
                    let percent = Self::usage_percent(pooled).unwrap_or(0.0);
                    let cost = Self::on_demand_cost(Some(pooled), billing_end);
                    (percent, None, None, cost)
                } else {
                    (0.0, None, None, None)
                }
            } else {
                (0.0, None, None, None)
            };

        let primary = RateWindow::with_details(percent_used, None, billing_end, None);

        let plan_type = summary
            .membership_type
            .as_ref()
            .map(|t| match t.to_lowercase().as_str() {
                "enterprise" => "Cursor Enterprise".to_string(),
                "pro" => "Cursor Pro".to_string(),
                "hobby" => "Cursor Hobby".to_string(),
                "team" => "Cursor Team".to_string(),
                other => format!("Cursor {}", capitalize(other)),
            });

        let email = user_info.as_ref().and_then(|u| u.email.clone());

        Ok(CursorUsageResult {
            primary,
            secondary,
            model_specific,
            cost: cost_snapshot,
            email,
            plan_type,
            grok_bot: None,
        })
    }

    fn plan_cost(
        used_usd: f64,
        limit_usd: f64,
        billing_cycle_start: Option<&str>,
        billing_end: Option<DateTime<Utc>>,
    ) -> CostSnapshot {
        let mut cost = CostSnapshot::new(used_usd, "USD", plan_period_label(billing_cycle_start));
        if limit_usd > 0.0 {
            cost = cost.with_limit(limit_usd);
        }
        if let Some(reset) = billing_end {
            cost = cost.with_resets_at(reset);
        }
        cost
    }

    fn on_demand_cost(
        on_demand: Option<&OnDemandUsage>,
        billing_end: Option<DateTime<Utc>>,
    ) -> Option<CostSnapshot> {
        let usage = on_demand?;
        if usage.enabled == Some(false) {
            return None;
        }

        let used_cents = usage.used.unwrap_or(0) as f64;
        let limit_cents = usage
            .limit
            .or_else(|| {
                usage
                    .remaining
                    .map(|remaining| remaining + usage.used.unwrap_or(0))
            })
            .unwrap_or(0) as f64;

        if used_cents <= 0.0 && limit_cents <= 0.0 {
            return None;
        }

        // usage-summary exposes on-demand spend in cents for the billing cycle.
        // Label it explicitly so the tray/detail cost line is not a vague "Monthly".
        let mut cost = CostSnapshot::new(used_cents / 100.0, "USD", "On-demand (billing cycle)");
        if limit_cents > 0.0 {
            cost = cost.with_limit(limit_cents / 100.0);
        }
        if let Some(reset) = billing_end {
            cost = cost.with_resets_at(reset);
        }
        Some(cost)
    }

    fn usage_percent(usage: &OnDemandUsage) -> Option<f64> {
        let used = usage.used.unwrap_or(0) as f64;
        let limit = usage
            .limit
            .or_else(|| {
                usage
                    .remaining
                    .map(|remaining| remaining + usage.used.unwrap_or(0))
            })
            .unwrap_or(0) as f64;
        (limit > 0.0).then_some(clamp_percent(used / limit * 100.0))
    }
}

fn clamp_percent(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 100.0)
}

/// Period label for plan-included spend from usage-summary (no new network calls).
fn plan_period_label(billing_cycle_start: Option<&str>) -> String {
    // Upstream 0.50.1 #2951: match the Cursor dashboard's name for the
    // included-usage pool (Cursor + third-party models).
    match billing_cycle_start {
        Some(start) if !start.is_empty() => format!("Cursor and Third Party (since {start})"),
        _ => "Cursor and Third Party (billing cycle)".to_string(),
    }
}
impl Default for CursorApi {
    fn default() -> Self {
        Self::new()
    }
}

// --- API Response Types ---

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageSummary {
    billing_cycle_start: Option<String>,
    billing_cycle_end: Option<String>,
    membership_type: Option<String>,
    limit_type: Option<String>,
    is_unlimited: Option<bool>,
    individual_usage: Option<IndividualUsage>,
    team_usage: Option<TeamUsage>,
}

impl UsageSummary {
    fn is_team_plan(&self) -> bool {
        matches!(
            self.membership_type
                .as_deref()
                .map(|membership| membership.to_ascii_lowercase())
                .as_deref(),
            Some("enterprise" | "business" | "team" | "teams")
        ) || self
            .limit_type
            .as_deref()
            .is_some_and(|limit_type| limit_type.eq_ignore_ascii_case("team"))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorTeamSpend {
    team_member_spend: Vec<CursorTeamMember>,
    total_pages: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CursorTeamMember {
    email: Option<String>,
    overall_spend_cents: Option<f64>,
    effective_per_user_limit_dollars: Option<f64>,
    monthly_limit_dollars: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CursorMemberBudget {
    used_usd: f64,
    limit_usd: f64,
}

impl CursorTeamMember {
    fn budget(&self) -> Option<CursorMemberBudget> {
        let used_usd = self.overall_spend_cents? / 100.0;
        let limit_usd = self
            .effective_per_user_limit_dollars
            .or(self.monthly_limit_dollars)?;
        (used_usd.is_finite() && used_usd >= 0.0 && limit_usd.is_finite() && limit_usd > 0.0)
            .then_some(CursorMemberBudget {
                used_usd,
                limit_usd,
            })
    }
}

fn select_member_budget(
    members: &[CursorTeamMember],
    email: &str,
) -> Result<Option<CursorMemberBudget>, ()> {
    let mut candidate = None;
    for member in members {
        if member
            .email
            .as_deref()
            .map(str::trim)
            .is_some_and(|member_email| member_email.eq_ignore_ascii_case(email))
        {
            // More than one matching member, or an invalid matching budget, is
            // ambiguous and must not select an arbitrary record.
            if candidate.is_some() {
                return Err(());
            }
            candidate = Some(member.budget().ok_or(())?);
        }
    }
    Ok(candidate)
}

#[derive(Debug, Deserialize)]
struct CursorTeams {
    teams: Vec<CursorTeam>,
}

#[derive(Debug, Deserialize)]
struct CursorTeam {
    id: i64,
}

impl CursorTeams {
    fn selected_id(&self, cookie_header: &str) -> Option<i64> {
        let ids: HashSet<i64> = self
            .teams
            .iter()
            .map(|team| team.id)
            .filter(|id| *id > 0)
            .collect();
        if ids.len() != self.teams.len() {
            return None;
        }

        for name in ["portal-selected-team-id", "team_id"] {
            let values: Vec<&str> = cookie_header
                .split(';')
                .filter_map(|part| {
                    let (cookie_name, value) = part.trim().split_once('=')?;
                    (cookie_name.trim() == name).then_some(value.trim())
                })
                .collect();
            if !values.is_empty() {
                let [value] = values.as_slice() else {
                    return None;
                };
                let id = value.parse::<i64>().ok().filter(|id| ids.contains(id))?;
                return Some(id);
            }
        }
        (ids.len() == 1).then(|| ids.into_iter().next()).flatten()
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndividualUsage {
    plan: Option<PlanUsage>,
    on_demand: Option<OnDemandUsage>,
    overall: Option<OnDemandUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanUsage {
    enabled: Option<bool>,
    used: Option<i64>,
    limit: Option<i64>,
    remaining: Option<i64>,
    breakdown: Option<PlanBreakdown>,
    auto_percent_used: Option<f64>,
    api_percent_used: Option<f64>,
    total_percent_used: Option<f64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanBreakdown {
    included: Option<i64>,
    bonus: Option<i64>,
    total: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OnDemandUsage {
    enabled: Option<bool>,
    used: Option<i64>,
    limit: Option<i64>,
    remaining: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TeamUsage {
    on_demand: Option<OnDemandUsage>,
    pooled: Option<OnDemandUsage>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SandUsageStatus {
    current_period_start: Option<String>,
    next_reset_timestamp_utc: Option<String>,
    usage_percent: Option<f64>,
    has_non_zero_included_limit: Option<bool>,
    included_limit_zero: Option<bool>,
    sand_trial_expires_at: Option<String>,
}

impl SandUsageStatus {
    fn to_window(&self, now: DateTime<Utc>) -> Option<NamedRateWindow> {
        let has_limit = self
            .included_limit_zero
            .map(|is_zero| !is_zero)
            .or(self.has_non_zero_included_limit);
        let has_trial = has_limit != Some(true)
            && self
                .sand_trial_expires_at
                .as_deref()
                .and_then(parse_iso_date)
                .is_some_and(|expires_at| expires_at > now);
        if has_limit != Some(true) && !has_trial {
            return None;
        }
        let used = clamp_percent(self.usage_percent?);
        let start = self
            .current_period_start
            .as_deref()
            .and_then(parse_iso_date);
        let reset = (!has_trial)
            .then(|| {
                self.next_reset_timestamp_utc
                    .as_deref()
                    .and_then(parse_iso_date)
            })
            .flatten();
        let minutes = start.zip(reset).and_then(|(start, reset)| {
            let minutes = (reset - start).num_minutes();
            // Guarded by the minutes > 0 check; rate windows are far
            // shorter than u32::MAX minutes.
            #[allow(
                clippy::cast_possible_truncation,
                reason = "guarded by the minutes > 0 check; rate windows are far shorter than u32::MAX minutes"
            )]
            let minutes_u32 = minutes as u32;
            (minutes > 0).then_some(minutes_u32)
        });
        Some(NamedRateWindow::new(
            "cursor-grok-bot",
            "Grok Bot",
            RateWindow::with_details(used, minutes, reset, None),
        ))
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserInfo {
    email: Option<String>,
    email_verified: Option<bool>,
    name: Option<String>,
    sub: Option<String>,
    created_at: Option<String>,
    updated_at: Option<String>,
    picture: Option<String>,
}

impl UserInfo {
    /// The email comes from the authenticated `/api/auth/me` response, so it is
    /// the only identity allowed to select a team member budget.
    fn verified_email(&self) -> Option<&str> {
        self.email
            .as_deref()
            .map(str::trim)
            .filter(|email| !email.is_empty())
    }
}

// --- Helper functions ---

fn parse_iso_date(s: &str) -> Option<DateTime<Utc>> {
    // Try with fractional seconds
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&Utc));
    }

    // Try without fractional seconds
    if let Ok(dt) = chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%SZ") {
        return Some(dt.with_timezone(&Utc));
    }

    None
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api() -> CursorApi {
        CursorApi::new()
    }

    fn parse_summary(json: &str) -> UsageSummary {
        serde_json::from_str(json).expect("fixture should parse")
    }

    #[test]
    fn sand_usage_maps_to_weekly_extra_window() {
        let status = SandUsageStatus {
            current_period_start: Some("2026-08-18T00:00:00Z".into()),
            next_reset_timestamp_utc: Some("2026-08-25T00:00:00Z".into()),
            usage_percent: Some(37.5),
            has_non_zero_included_limit: Some(true),
            included_limit_zero: None,
            sand_trial_expires_at: None,
        };
        let row = status
            .to_window("2026-08-20T00:00:00Z".parse().unwrap())
            .expect("grok bot window");
        assert_eq!(row.id, "cursor-grok-bot");
        assert_eq!(row.title, "Grok Bot");
        assert!((row.window.used_percent - 37.5).abs() < 0.001);
        assert_eq!(row.window.window_minutes, Some(10080));
    }

    #[test]
    fn sand_usage_hides_accounts_without_included_allowance() {
        let status = SandUsageStatus {
            current_period_start: None,
            next_reset_timestamp_utc: None,
            usage_percent: Some(0.0),
            has_non_zero_included_limit: Some(false),
            included_limit_zero: None,
            sand_trial_expires_at: None,
        };
        assert!(
            status
                .to_window("2026-08-20T00:00:00Z".parse().unwrap())
                .is_none()
        );
    }

    #[test]
    fn sand_usage_maps_paid_allowance_from_explicit_zero_flag() {
        let status = SandUsageStatus {
            current_period_start: Some("2026-08-18T00:00:00Z".into()),
            next_reset_timestamp_utc: Some("2026-08-25T00:00:00Z".into()),
            usage_percent: Some(37.5),
            has_non_zero_included_limit: Some(false),
            included_limit_zero: Some(false),
            sand_trial_expires_at: None,
        };
        let row = status
            .to_window("2026-08-20T00:00:00Z".parse().unwrap())
            .expect("paid Grok Bot window");
        assert_eq!(row.window.window_minutes, Some(10080));
        assert_eq!(
            row.window.resets_at,
            Some("2026-08-25T00:00:00Z".parse().unwrap())
        );
    }

    #[test]
    fn sand_usage_maps_an_active_trial_without_a_recurring_reset() {
        let status = SandUsageStatus {
            current_period_start: Some("2026-08-18T00:00:00Z".into()),
            next_reset_timestamp_utc: Some("2026-08-25T00:00:00Z".into()),
            usage_percent: Some(12.5),
            has_non_zero_included_limit: Some(false),
            included_limit_zero: Some(true),
            sand_trial_expires_at: Some("2026-08-28T00:00:00Z".into()),
        };
        let row = status
            .to_window("2026-08-20T00:00:00Z".parse().unwrap())
            .expect("active trial window");
        assert_eq!(row.window.resets_at, None);
        assert_eq!(row.window.window_minutes, None);
        assert!((row.window.used_percent - 12.5).abs() < 0.001);
    }

    #[test]
    fn sand_usage_hides_expired_trial() {
        let status = SandUsageStatus {
            current_period_start: None,
            next_reset_timestamp_utc: None,
            usage_percent: Some(12.5),
            has_non_zero_included_limit: Some(false),
            included_limit_zero: Some(true),
            sand_trial_expires_at: Some("2026-08-19T00:00:00Z".into()),
        };
        assert!(
            status
                .to_window("2026-08-20T00:00:00Z".parse().unwrap())
                .is_none()
        );
    }

    #[test]
    fn test_cursor_build_result_with_lanes() {
        let json = r#"{
            "billingCycleStart": "2026-03-01T00:00:00Z",
            "billingCycleEnd": "2026-04-01T00:00:00Z",
            "membershipType": "pro",
            "individualUsage": {
                "plan": {
                    "used": 1500,
                    "limit": 5000,
                    "totalPercentUsed": 30.0,
                    "autoPercentUsed": 20.0,
                    "apiPercentUsed": 10.0
                }
            }
        }"#;

        let summary = parse_summary(json);
        let result = api().build_result(summary, None).unwrap();

        assert!((result.primary.used_percent - 30.0).abs() < 0.01);

        let sec = result.secondary.expect("secondary should be present");
        assert!((sec.used_percent - 20.0).abs() < 0.01);
        assert!(sec.resets_at.is_some());

        let ms = result
            .model_specific
            .expect("model_specific should be present");
        assert!((ms.used_percent - 10.0).abs() < 0.01);
        assert!(ms.resets_at.is_some());

        assert!(result.cost.is_some());
        assert_eq!(result.plan_type.as_deref(), Some("Cursor Pro"));
    }

    #[test]
    fn clamps_plan_usage_percent_at_100_when_over_limit() {
        // Upstream #2255: included usage past limit must not paint >100%.
        let json = r#"{
            "membershipType": "pro",
            "individualUsage": {
                "plan": {
                    "used": 6000,
                    "limit": 5000,
                    "totalPercentUsed": 120.0,
                    "autoPercentUsed": 110.0,
                    "apiPercentUsed": 105.0
                }
            }
        }"#;
        let summary = parse_summary(json);
        let result = api().build_result(summary, None).unwrap();
        assert!((result.primary.used_percent - 100.0).abs() < 0.01);
        assert!((result.secondary.unwrap().used_percent - 100.0).abs() < 0.01);
        assert!((result.model_specific.unwrap().used_percent - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_cursor_build_result_prefers_api_percent_fields() {
        let json = r#"{
            "membershipType": "pro",
            "autoModelSelectedDisplayMessage": "You've used 13% of your included total usage",
            "individualUsage": {
                "plan": {
                    "used": 2000,
                    "limit": 2000,
                    "breakdown": {
                        "included": 2000,
                        "bonus": 580,
                        "total": 2580
                    },
                    "autoPercentUsed": 17.2,
                    "apiPercentUsed": 0,
                    "totalPercentUsed": 13.230769230769232
                }
            }
        }"#;

        let summary = parse_summary(json);
        let result = api().build_result(summary, None).unwrap();

        assert!((result.primary.used_percent - 13.230769230769232).abs() < 0.01);
        assert!((result.secondary.unwrap().used_percent - 17.2).abs() < 0.01);
        assert!((result.model_specific.unwrap().used_percent - 0.0).abs() < 0.01);

        let cost = result
            .cost
            .expect("plan usage should still produce cost snapshot");
        assert!((cost.used - 20.0).abs() < 0.01);
        assert_eq!(cost.limit, Some(20.0));
        assert_eq!(result.plan_type.as_deref(), Some("Cursor Pro"));
    }

    #[test]
    fn test_cursor_build_result_cents_only() {
        let json = r#"{
            "billingCycleEnd": "2026-04-01T00:00:00Z",
            "membershipType": "pro",
            "individualUsage": {
                "plan": {
                    "used": 2500,
                    "limit": 5000
                }
            }
        }"#;

        let summary = parse_summary(json);
        let result = api().build_result(summary, None).unwrap();

        assert!((result.primary.used_percent - 50.0).abs() < 0.01);
        assert!(result.secondary.is_none(), "no autoPercentUsed in payload");
        assert!(
            result.model_specific.is_none(),
            "no apiPercentUsed in payload"
        );
        assert!(result.cost.is_some());
    }

    #[test]
    fn test_cursor_build_result_missing_plan() {
        let json = r#"{
            "membershipType": "hobby",
            "individualUsage": {}
        }"#;

        let summary = parse_summary(json);
        let result = api().build_result(summary, None).unwrap();

        assert!((result.primary.used_percent).abs() < 0.01);
        assert!(result.secondary.is_none());
        assert!(result.model_specific.is_none());
        assert!(result.cost.is_none());
    }

    #[test]
    fn test_cursor_on_demand_as_cost() {
        let json = r#"{
            "billingCycleEnd": "2026-04-01T00:00:00Z",
            "membershipType": "pro",
            "individualUsage": {
                "plan": {
                    "used": 800,
                    "limit": 5000,
                    "totalPercentUsed": 16.0
                },
                "onDemand": {
                    "enabled": true,
                    "used": 350,
                    "limit": 1000
                }
            }
        }"#;

        let summary = parse_summary(json);
        let result = api().build_result(summary, None).unwrap();

        assert!((result.primary.used_percent - 16.0).abs() < 0.01);
        let cost = result.cost.expect("cost should exist from on-demand usage");
        assert!((cost.used - 3.5).abs() < 0.01);
        assert_eq!(cost.limit, Some(10.0));
        assert_eq!(cost.period, "On-demand (billing cycle)");
    }

    #[test]
    fn plan_cost_period_uses_billing_cycle_start() {
        let json = r#"{
            "billingCycleStart": "2026-03-01T00:00:00Z",
            "billingCycleEnd": "2026-04-01T00:00:00Z",
            "membershipType": "pro",
            "individualUsage": {
                "plan": {
                    "used": 2500,
                    "limit": 5000
                }
            }
        }"#;
        let summary = parse_summary(json);
        let result = api().build_result(summary, None).unwrap();
        let cost = result.cost.expect("plan cost");
        assert!((cost.used - 25.0).abs() < 0.01);
        assert_eq!(cost.limit, Some(50.0));
        assert_eq!(
            cost.period,
            "Cursor and Third Party (since 2026-03-01T00:00:00Z)"
        );
    }

    #[test]
    fn test_cursor_individual_overall_fallback() {
        let summary =
            parse_summary(r#"{"individualUsage":{"overall":{"used":2500,"limit":10000}}}"#);
        let result = api().build_result(summary, None).unwrap();
        assert!((result.primary.used_percent - 25.0).abs() < 0.01);
        assert_eq!(result.cost.unwrap().limit, Some(100.0));
    }

    #[test]
    fn test_cursor_team_pooled_fallback() {
        let summary = parse_summary(r#"{"teamUsage":{"pooled":{"used":5000,"limit":10000}}}"#);
        let result = api().build_result(summary, None).unwrap();
        assert!((result.primary.used_percent - 50.0).abs() < 0.01);
        assert_eq!(result.cost.unwrap().used, 50.0);
    }

    #[test]
    fn team_member_budget_preserves_true_zero_and_rejects_invalid_limits() {
        let zero: CursorTeamMember = serde_json::from_str(
            r#"{"overallSpendCents":0,"effectivePerUserLimitDollars":150,"monthlyLimitDollars":200}"#,
        )
        .unwrap();
        assert_eq!(
            zero.budget(),
            Some(CursorMemberBudget {
                used_usd: 0.0,
                limit_usd: 150.0
            })
        );
        let monthly_fallback: CursorTeamMember =
            serde_json::from_str(r#"{"overallSpendCents":1312,"monthlyLimitDollars":150}"#)
                .unwrap();
        assert_eq!(
            monthly_fallback.budget(),
            Some(CursorMemberBudget {
                used_usd: 13.12,
                limit_usd: 150.0
            })
        );
        assert_eq!(
            select_member_budget(&[monthly_fallback], "missing@example.com"),
            Ok(None)
        );
        let duplicate = CursorTeamMember {
            email: Some("member@example.com".into()),
            overall_spend_cents: Some(1312.0),
            effective_per_user_limit_dollars: Some(150.0),
            monthly_limit_dollars: None,
        };
        assert!(
            select_member_budget(&[duplicate.clone(), duplicate], "member@example.com").is_err()
        );

        for json in [
            r#"{"overallSpendCents":1312,"effectivePerUserLimitDollars":0,"monthlyLimitDollars":150}"#,
            r#"{"overallSpendCents":-1,"monthlyLimitDollars":150}"#,
            r#"{"overallSpendCents":1312,"monthlyLimitDollars":-1}"#,
        ] {
            let member: CursorTeamMember = serde_json::from_str(json).unwrap();
            assert!(
                member.budget().is_none(),
                "invalid budget must fail closed: {json}"
            );
        }
    }

    #[test]
    fn team_selection_requires_one_valid_identity() {
        let teams: CursorTeams =
            serde_json::from_str(r#"{"teams":[{"id":11},{"id":22}]}"#).unwrap();
        assert_eq!(
            teams.selected_id("auth=fixture; portal-selected-team-id=22"),
            Some(22)
        );
        assert_eq!(teams.selected_id("auth=fixture"), None);
        assert_eq!(
            teams.selected_id("auth=fixture; portal-selected-team-id=99; team_id=11"),
            None
        );
        assert_eq!(
            teams.selected_id("auth=fixture; team_id=11; team_id=22"),
            None
        );

        let single: CursorTeams = serde_json::from_str(r#"{"teams":[{"id":22}]}"#).unwrap();
        assert_eq!(single.selected_id("auth=fixture"), Some(22));
    }

    #[test]
    fn member_lookup_requires_nonempty_authenticated_email() {
        for email in [None, Some(String::new()), Some("  ".to_string())] {
            let user = UserInfo {
                email,
                email_verified: None,
                name: None,
                sub: None,
                created_at: None,
                updated_at: None,
                picture: None,
            };
            assert!(user.verified_email().is_none());
        }
    }

    #[test]
    fn verified_team_budget_replaces_summary_plan_and_keeps_zero_summary_fallback() {
        let summary = parse_summary(
            r#"{
                "billingCycleStart":"2026-09-01T00:00:00Z",
                "billingCycleEnd":"2026-10-01T00:00:00Z",
                "membershipType":"enterprise",
                "individualUsage":{"plan":{"used":0,"limit":2000,"totalPercentUsed":0}}
            }"#,
        );
        let result = api()
            .build_result_with_team_budget(
                summary,
                None,
                Some(CursorMemberBudget {
                    used_usd: 13.12,
                    limit_usd: 150.0,
                }),
            )
            .unwrap();
        assert!((result.primary.used_percent - 8.7466666667).abs() < 0.00001);
        let cost = result.cost.expect("verified member budget cost");
        assert!((cost.used - 13.12).abs() < 0.00001);
        assert_eq!(cost.limit, Some(150.0));

        let fallback_summary = parse_summary(
            r#"{
                "billingCycleStart":"2026-09-01T00:00:00Z",
                "billingCycleEnd":"2026-10-01T00:00:00Z",
                "membershipType":"enterprise",
                "individualUsage":{"plan":{"used":0,"limit":2000,"totalPercentUsed":0}}
            }"#,
        );
        let fallback = api().build_result(fallback_summary, None).unwrap();
        assert_eq!(fallback.primary.used_percent, 0.0);
        assert_eq!(
            fallback.cost.expect("summary fallback cost").limit,
            Some(20.0)
        );
    }
}
