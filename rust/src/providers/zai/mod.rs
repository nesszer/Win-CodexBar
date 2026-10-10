//! z.ai provider implementation
//!
//! Fetches usage data from z.ai's quota API
//! Uses API token stored in Windows Credential Manager

mod balance;
pub mod region;
mod reset_plausibility;
pub mod settings;

pub use region::ZaiRegion;
pub use settings::ZaiSettingsError;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Url;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    RateWindow, SourceMode, UsageSnapshot,
};

use reset_plausibility::is_plausible_five_hour_reset;
use settings::ZaiSettingsReader;

const ZAI_USAGE_SCOPE_ENV: &str = "Z_AI_USAGE_SCOPE";
const ZAI_BIGMODEL_ORG_ENV: &str = "Z_AI_BIGMODEL_ORGANIZATION";
const ZAI_BIGMODEL_PROJECT_ENV: &str = "Z_AI_BIGMODEL_PROJECT";

/// Windows Credential Manager target for z.ai API token
const ZAI_CREDENTIAL_TARGET: &str = "codexbar-zai";

/// z.ai quota response structure
#[derive(Debug, Deserialize)]
struct ZaiQuotaResponse {
    #[serde(default)]
    code: Option<i32>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    data: Option<ZaiQuotaData>,
    /// Legacy flat limits array (backwards compat). Entries stay raw so an
    /// unknown limit type is skipped without requiring the legacy fields.
    #[serde(default)]
    limits: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Deserialize)]
struct ZaiQuotaData {
    #[serde(default)]
    limits: Option<Vec<serde_json::Value>>,
    #[serde(rename = "planName")]
    plan_name: Option<String>,
    /// Upstream plan-name fallbacks (`level` added in 0.48.0).
    #[serde(default)]
    plan: Option<String>,
    #[serde(default)]
    plan_type: Option<String>,
    #[serde(default, rename = "packageName")]
    package_name: Option<String>,
    #[serde(default)]
    level: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ZaiLimit {
    /// Limit type: "TOKENS_LIMIT" or "TIME_LIMIT" (upstream) or "tokens"/"mcp" (legacy)
    #[serde(rename = "type")]
    limit_type: Option<String>,
    /// Used amount (legacy response)
    used: Option<f64>,
    /// Total limit (current response)
    usage: Option<f64>,
    /// Current value (alternative to used)
    #[serde(rename = "currentValue")]
    current_value: Option<f64>,
    /// Total limit
    limit: Option<f64>,
    /// Remaining amount
    remaining: Option<f64>,
    /// Used percentage (current response)
    percentage: Option<f64>,
    /// Time unit enum: 1=days, 3=hours, 5=minutes, 6=weeks
    unit: Option<i32>,
    /// Number of time units in the window
    number: Option<i32>,
    /// Reset time (ISO 8601)
    #[serde(rename = "resetAt")]
    reset_at: Option<String>,
    /// Reset time as Unix epoch milliseconds (current response)
    #[serde(rename = "nextResetTime")]
    next_reset_time: Option<i64>,
}

impl ZaiLimit {
    /// Whether the entry carries any usage figure to derive a percentage from.
    fn has_quota_signal(&self) -> bool {
        [
            self.used,
            self.usage,
            self.current_value,
            self.limit,
            self.remaining,
            self.percentage,
        ]
        .iter()
        .any(Option::is_some)
    }
}

const ZAI_UNSUPPORTED_FORMAT: &str =
    "Unsupported z.ai quota format. Check Usage Dashboard for plan usage.";
const ZAI_UNSUPPORTED_ENTRY: &str =
    "Unsupported z.ai quota entry. Check Usage Dashboard for plan usage.";
const ZAI_UNAVAILABLE_HINT: &str = "Check Usage Dashboard for complete plan usage.";

/// Parsed quota plus the display detail explaining any unavailable quota.
#[derive(Debug)]
struct ZaiParsedQuota {
    usage: UsageSnapshot,
    unavailable_detail: Option<ProviderDisplayDetail>,
}

fn is_token_limit_type(limit_type: Option<&str>) -> bool {
    matches!(
        limit_type,
        Some("TOKENS_LIMIT") | Some("CREDIT_LIMIT") | Some("tokens")
    )
}

fn is_time_limit_type(limit_type: Option<&str>) -> bool {
    matches!(limit_type, Some("TIME_LIMIT") | Some("mcp"))
}

/// Split raw limit entries into recognized limits and a skipped-entry count.
///
/// Upstream 0.69.0 (#4091): a string `type` outside the known limit types is
/// skipped without needing legacy fields; a missing or non-string `type`, a
/// recognized entry that does not deserialize, or a recognized entry with no
/// quota signal at all (which would fabricate a 0% window) is a malformed entry.
fn recognized_limits(raw: &[serde_json::Value]) -> Result<(Vec<ZaiLimit>, usize), ProviderError> {
    let unsupported_entry = || ProviderError::Parse(ZAI_UNSUPPORTED_ENTRY.to_string());
    let mut limits = Vec::with_capacity(raw.len());
    let mut skipped = 0;
    for entry in raw {
        let Some(limit_type) = entry.get("type").and_then(serde_json::Value::as_str) else {
            return Err(unsupported_entry());
        };
        if !is_token_limit_type(Some(limit_type)) && !is_time_limit_type(Some(limit_type)) {
            skipped += 1;
            continue;
        }
        let limit = ZaiLimit::deserialize(entry).map_err(|_| unsupported_entry())?;
        if !limit.has_quota_signal() {
            return Err(unsupported_entry());
        }
        limits.push(limit);
    }
    Ok((limits, skipped))
}

/// Decode the quota envelope. A well-formed JSON body of the wrong shape is an
/// unsupported format (points at the Usage Dashboard); a syntax error keeps
/// the parser message.
fn parse_quota_body(body: &[u8]) -> Result<ZaiQuotaResponse, ProviderError> {
    serde_json::from_slice(body).map_err(|e| {
        if e.classify() == serde_json::error::Category::Data {
            ProviderError::Parse(ZAI_UNSUPPORTED_FORMAT.to_string())
        } else {
            ProviderError::Parse(e.to_string())
        }
    })
}

/// Detail row for quota the API returned but this client cannot show. The
/// title mirrors upstream: "Additional quota" when recognized token windows
/// exist, otherwise "Coding Plan usage".
fn unavailable_quota_detail(has_token_limits: bool) -> Option<ProviderDisplayDetail> {
    let (id, title) = if has_token_limits {
        ("additional-quota", "Additional quota")
    } else {
        ("coding-plan-usage", "Coding Plan usage")
    };
    ProviderDisplayDetail::new(id, title, "Unavailable")?.with_secondary_value(ZAI_UNAVAILABLE_HINT)
}

/// z.ai provider
#[derive(Default)]
pub struct ZaiProvider;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ZaiTeamContext {
    organization_id: String,
    project_id: String,
}

impl ZaiProvider {
    pub fn new() -> Self {
        Self
    }

    /// Effective API region (upstream 0.48.0): an explicit settings value
    /// wins; otherwise the region is inferred from canonical endpoint
    /// overrides (`inferredRegion`).
    fn effective_region(ctx: &FetchContext, env: &settings::EnvMap) -> ZaiRegion {
        match ctx
            .api_region
            .as_deref()
            .map(str::trim)
            .filter(|raw| !raw.is_empty())
        {
            Some(raw) => ZaiRegion::from_settings_value(Some(raw)),
            None => ZaiSettingsReader::inferred_region(env),
        }
    }

    /// Get API token from ctx, Windows Credential Manager, or region-bound env.
    fn get_api_token(
        api_key: Option<&str>,
        region: ZaiRegion,
        env: &settings::EnvMap,
    ) -> Result<String, ProviderError> {
        // Check ctx.api_key first (from settings)
        if let Some(key) = api_key
            && let Some(cleaned) = settings::cleaned(key)
        {
            return Ok(cleaned);
        }

        // Try Windows Credential Manager
        if let Ok(entry) = keyring::Entry::new(ZAI_CREDENTIAL_TARGET, "api_token")
            && let Ok(token) = entry.get_password()
        {
            return Ok(token);
        }

        let home = dirs::home_dir().unwrap_or_default();
        ZaiSettingsReader::api_token(env, &home, region).ok_or_else(|| {
            ProviderError::NotInstalled(match region {
                ZaiRegion::BigModelCn => "z.ai (BigModel CN) API token not found. Set in Preferences → Providers, Z_AI_API_KEY, BIGMODEL_API_KEY, ZHIPU_API_KEY, ZHIPUAI_API_KEY, or GLM_API_KEY.".to_string(),
                ZaiRegion::Global => "z.ai API token not found. Set in Preferences → Providers or Z_AI_API_KEY.".to_string(),
            })
        })
    }

    /// Quota URL: `Z_AI_QUOTA_URL` full override → `Z_AI_API_HOST` host
    /// override → the selected region's canonical endpoint.
    fn quota_url(env: &settings::EnvMap, region: ZaiRegion) -> Result<Url, ProviderError> {
        let provider_err = |err: ZaiSettingsError| ProviderError::Other(err.to_string());
        if let Some(url) = ZaiSettingsReader::quota_url_override(env).map_err(provider_err)? {
            return Ok(url);
        }
        if let Some(url) = ZaiSettingsReader::quota_url_from_api_host(env).map_err(provider_err)? {
            return Ok(url);
        }
        Ok(region.quota_limit_url())
    }

    fn request_url(
        env: &settings::EnvMap,
        region: ZaiRegion,
        team_context: Option<&ZaiTeamContext>,
    ) -> Result<Url, ProviderError> {
        let mut url = Self::quota_url(env, region)?;
        if team_context.is_some() {
            url.query_pairs_mut().append_pair("type", "2");
        }
        Ok(url)
    }

    fn team_context(ctx: &FetchContext) -> Result<Option<ZaiTeamContext>, ProviderError> {
        let explicit_scope = std::env::var(ZAI_USAGE_SCOPE_ENV)
            .ok()
            .and_then(|value| settings::cleaned(&value))
            .is_some_and(|value| value.eq_ignore_ascii_case("team"));
        let context = ctx
            .workspace_id
            .as_deref()
            .and_then(parse_team_context_pair)
            .or_else(ZaiTeamContext::from_env);

        if explicit_scope && context.is_none() {
            return Err(ProviderError::Other(
                "z.ai team usage requires Z_AI_BIGMODEL_ORGANIZATION and Z_AI_BIGMODEL_PROJECT, or workspace_id as organization|project."
                    .to_string(),
            ));
        }
        Ok(context)
    }

    /// Fetch usage from z.ai API
    async fn fetch_usage_api(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let env = settings::process_env();
        let region = Self::effective_region(ctx, &env);
        // Canonical cross-region overrides are rejected before any bearer
        // token is sent; custom relay hosts stay legal (upstream #2621/#2623).
        ZaiSettingsReader::validate_endpoint_overrides(&env, region)
            .map_err(|err| ProviderError::Other(err.to_string()))?;
        let api_token = Self::get_api_token(ctx.api_key.as_deref(), region, &env)?;

        let client = crate::core::credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let team_context = Self::team_context(ctx)?;
        let request_url = Self::request_url(&env, region, team_context.as_ref())?;
        let authorization = authorization_header(&api_token);
        let mut request = client
            .get(request_url)
            .header("Authorization", authorization.as_str())
            .header("Accept", "application/json");
        if let Some(team) = &team_context {
            request = request
                .header("Bigmodel-Organization", team.organization_id.as_str())
                .header("Bigmodel-Project", team.project_id.as_str());
        }
        let resp = request.send().await?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(ProviderError::AuthRequired);
        }

        if !resp.status().is_success() {
            return Err(ProviderError::Other(format!(
                "z.ai API returned status {}",
                resp.status()
            )));
        }

        let resp_bytes = resp
            .bytes()
            .await
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        // Handle empty response body (can happen with wrong region/endpoint)
        if resp_bytes.is_empty() {
            return Err(ProviderError::Parse(
                "Empty response body from z.ai API. Check API region and token.".to_string(),
            ));
        }

        let quota = parse_quota_body(&resp_bytes)?;

        let ZaiParsedQuota {
            mut usage,
            unavailable_detail,
        } = self.parse_quota_response(&quota)?;
        if region == ZaiRegion::BigModelCn
            && let Some(balance) = balance::fetch_cn_balance(&client, &authorization).await
        {
            let mut row = RateWindow::informational(format!("¥{balance:.2}"));
            row.reset_description = Some(format!("¥{balance:.2} available"));
            usage = usage.with_extra_rate_window("zai-account-balance", "Account balance", row);
        }
        Ok(ProviderFetchResult::new(usage, "oauth").with_display_detail(unavailable_detail))
    }

    fn parse_quota_response(
        &self,
        quota: &ZaiQuotaResponse,
    ) -> Result<ZaiParsedQuota, ProviderError> {
        if quota.code.is_some_and(|code| code != 0 && code != 200) {
            return Err(ProviderError::Other(
                quota
                    .message
                    .as_deref()
                    .filter(|message| !message.trim().is_empty())
                    .unwrap_or("z.ai API returned an error")
                    .to_string(),
            ));
        }

        // Get limits from data.limits (upstream) or flat limits (legacy)
        let raw_limits = match &quota.data {
            Some(data) => data.limits.as_deref(),
            None => quota.limits.as_deref(),
        }
        .ok_or_else(|| ProviderError::Parse(ZAI_UNSUPPORTED_FORMAT.to_string()))?;
        let (limits, skipped_limits) = recognized_limits(raw_limits)?;
        // Upstream 0.48.0 plan-name fallbacks: planName, plan, plan_type,
        // packageName, level — first non-empty trimmed wins.
        let plan_name = quota
            .data
            .as_ref()
            .and_then(|data| {
                [
                    data.plan_name.as_deref(),
                    data.plan.as_deref(),
                    data.plan_type.as_deref(),
                    data.package_name.as_deref(),
                    data.level.as_deref(),
                ]
                .into_iter()
                .filter_map(|raw| raw.map(str::trim))
                .find(|raw| !raw.is_empty())
            })
            .unwrap_or("z.ai");

        // Collect token/credit limit entries (upstream 0.49.0 #2724: credit
        // Coding Plans report `CREDIT_LIMIT` rows with the same shape as
        // `TOKENS_LIMIT`; upstream uses "TOKENS_LIMIT", legacy uses "tokens").
        let is_tokens = |l: &&ZaiLimit| is_token_limit_type(l.limit_type.as_deref());
        let is_time = |l: &&ZaiLimit| is_time_limit_type(l.limit_type.as_deref());
        let mut token_limits: Vec<&ZaiLimit> = limits.iter().filter(is_tokens).collect();
        // Upstream ordering: ascending window minutes, unknown windows last.
        token_limits.sort_by_key(|l| Self::window_minutes(l).unwrap_or(u32::MAX));
        let time_limit = limits.iter().find(is_time);

        // Compute used percent for a limit entry (upstream 0.49.0 `parseLimit`):
        // when the response carries a positive `usage` total, the absolute
        // used signal (`usage - remaining`, or `currentValue`) wins over the
        // API's own `percentage`; otherwise `percentage` is trusted, and
        // legacy `limit`/`used` responses fall back to the old math.
        fn compute_percent(l: &ZaiLimit) -> f64 {
            if let Some(usage) = l.usage.filter(|&usage| usage > 0.0) {
                let used = if let Some(remaining) = l.remaining {
                    let from_remaining = usage - remaining;
                    let baseline = l.current_value.unwrap_or(from_remaining);
                    from_remaining.max(baseline)
                } else {
                    l.current_value.unwrap_or(0.0)
                };
                let clamped = used.clamp(0.0, usage);
                return (clamped / usage * 100.0).clamp(0.0, 100.0);
            }
            if let Some(percentage) = l.percentage {
                return percentage.clamp(0.0, 100.0);
            }

            let limit = l.limit.unwrap_or(0.0);
            if limit <= 0.0 {
                return if l.used.unwrap_or(0.0) > 0.0 || l.current_value.unwrap_or(0.0) > 0.0 {
                    100.0
                } else {
                    0.0
                };
            }
            let used = {
                let from_remaining = l.remaining.map(|r| limit - r);
                let from_current = l.current_value;
                let from_used = l.used;
                // Use max of available signals
                let candidates = [from_remaining, from_current, from_used];
                candidates.iter().filter_map(|&v| v).fold(0.0_f64, f64::max)
            };
            ((used / limit) * 100.0).clamp(0.0, 100.0)
        }

        // Upstream 0.48.0 `rateWindow`/`resetDescription`: only token-type
        // windows keep duration minutes; TIME_LIMIT (MCP) carries the "MCP"
        // label and no window duration; 5-hour token windows are labeled
        // "5-hour"; otherwise the explicit window label is used.
        let now = Utc::now();
        let make_window = |l: &ZaiLimit| -> RateWindow {
            let window_mins = if is_token_limit_type(l.limit_type.as_deref()) {
                ZaiProvider::window_minutes(l)
            } else {
                None
            };
            let resets_at = l
                .next_reset_time
                .and_then(DateTime::<Utc>::from_timestamp_millis)
                .or_else(|| {
                    l.reset_at
                        .as_deref()
                        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
                        .map(|timestamp| timestamp.with_timezone(&Utc))
                });
            let resets_at =
                resets_at.filter(|reset| is_plausible_five_hour_reset(window_mins, *reset, now));
            RateWindow::with_details(
                compute_percent(l),
                window_mins,
                resets_at,
                rate_window_reset_description(l, window_mins),
            )
        };

        // Upstream 0.48.0 bucket split: with 2+ token limits, the shortest
        // window → session (5-hour GLM Coding Plan window) and the longest →
        // weekly token quota; with one token limit it stands alone; with none
        // the MCP (time) limit is the primary.
        let (token_limit, session_token_limit) = match token_limits.as_slice() {
            [] => (None, None),
            [single] => (Some(*single), None),
            _ => (token_limits.last().copied(), token_limits.first().copied()),
        };
        let primary_limit = session_token_limit.or(token_limit).or(time_limit);
        let secondary_limit = if session_token_limit.is_some() {
            token_limit
        } else {
            None
        };

        // No recognized limit: never fabricate a 0% quota window. The detail
        // row below explains that plan usage is unavailable.
        let primary = primary_limit
            .map(make_window)
            .unwrap_or_else(|| RateWindow::informational("Unavailable"));
        let mut usage = UsageSnapshot::new(primary).with_login_method(plan_name);
        if let Some(secondary) = secondary_limit {
            usage = usage.with_secondary(make_window(secondary));
        }
        // MCP usage is a separate named window whenever a coding-limit
        // primary exists; with no token limits it already owns the primary.
        if (token_limit.is_some() || session_token_limit.is_some())
            && let Some(mcp) = time_limit
        {
            usage = usage.with_extra_rate_window("zai-mcp", "MCP", make_window(mcp));
        }

        let unavailable_detail = (limits.is_empty() || skipped_limits > 0)
            .then(|| unavailable_quota_detail(!token_limits.is_empty()))
            .flatten();

        Ok(ZaiParsedQuota {
            usage,
            unavailable_detail,
        })
    }

    /// Compute window_minutes from a limit's unit + number fields.
    /// Returns `None` when number ≤ 0 or unit is unknown (upstream windowMinutes).
    fn window_minutes(l: &ZaiLimit) -> Option<u32> {
        let number = l.number.filter(|&n| n > 0)? as u32;
        let (minutes_per_unit, _, _) = unit_spec(l.unit?)?;
        Some(number * minutes_per_unit)
    }
}

/// Upstream 0.48.0 `resetDescription`: MCP (TIME_LIMIT) → "MCP"; 5-hour
/// token window → "5-hour"; else the explicit window label, if any.
fn rate_window_reset_description(l: &ZaiLimit, window_mins: Option<u32>) -> Option<String> {
    if is_time_limit_type(l.limit_type.as_deref()) {
        return Some("MCP".to_string());
    }
    if is_token_limit_type(l.limit_type.as_deref()) && window_mins == Some(300) {
        return Some("5-hour".to_string());
    }
    window_label(l)
}

fn window_label(l: &ZaiLimit) -> Option<String> {
    let number = l.number.filter(|&n| n > 0)?;
    let (_, one, many) = unit_spec(l.unit?)?;
    let unit_label = if number == 1 { one } else { many };
    Some(format!("{number} {unit_label} window"))
}

/// z.ai limit unit code → (minutes per unit, singular label, plural label).
fn unit_spec(unit: i32) -> Option<(u32, &'static str, &'static str)> {
    Some(match unit {
        1 => (1440, "day", "days"),
        3 => (60, "hour", "hours"),
        5 => (1, "minute", "minutes"),
        6 => (10080, "week", "weeks"),
        _ => return None,
    })
}

impl ZaiTeamContext {
    fn from_env() -> Option<Self> {
        let organization_id = std::env::var(ZAI_BIGMODEL_ORG_ENV)
            .ok()
            .and_then(|value| settings::cleaned(&value))?;
        let project_id = std::env::var(ZAI_BIGMODEL_PROJECT_ENV)
            .ok()
            .and_then(|value| settings::cleaned(&value))?;
        Some(Self {
            organization_id,
            project_id,
        })
    }
}

fn parse_team_context_pair(raw: &str) -> Option<ZaiTeamContext> {
    let (organization_id, project_id) = raw
        .split_once('|')
        .or_else(|| raw.split_once(','))
        .or_else(|| raw.split_once(';'))?;
    Some(ZaiTeamContext {
        organization_id: settings::cleaned(organization_id)?,
        project_id: settings::cleaned(project_id)?,
    })
}

fn authorization_header(token: &str) -> String {
    let trimmed = token.trim();
    if trimmed.to_ascii_lowercase().starts_with("bearer ") {
        trimmed.to_string()
    } else {
        format!("Bearer {trimmed}")
    }
}

#[async_trait]
impl Provider for ZaiProvider {
    fn automatic_metric_prioritizes_exhausted_window(&self) -> bool {
        false
    }

    fn id(&self) -> ProviderId {
        ProviderId::Zai
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching z.ai usage");

        // z.ai only supports OAuth/API token - no CLI or web cookie fallback
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_usage_api(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                // z.ai doesn't support web cookies or CLI
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

#[cfg(test)]
mod tests;
