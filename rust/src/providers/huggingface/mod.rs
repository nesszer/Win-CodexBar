//! Hugging Face billing provider.
//!
//! Hugging Face exposes inference billing and optional ZeroGPU usage through
//! authenticated JSON endpoints. The billing data is presented as cost and
//! transient detail rows; it is deliberately not converted into a quota
//! window or a persisted identity record.

use async_trait::async_trait;
use chrono::{DateTime, Datelike, TimeZone, Utc};
use futures::StreamExt;
use reqwest::{Client, StatusCode, Url};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderDisplayDetail, ProviderError,
    ProviderFetchResult, ProviderId, ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const BILLING_URL: &str = "https://huggingface.co/api/settings/billing/usage-v2";
const WHOAMI_URL: &str = "https://huggingface.co/api/whoami-v2";
const ZEROGPU_URL: &str = "https://huggingface.co/api/spaces/zero-gpu/quota";
const CREDENTIAL_TARGET: &str = "codexbar-huggingface";
const USER_AGENT: &str = "CodexBar";
const PRIMARY_TIMEOUT: Duration = Duration::from_secs(15);
const OPTIONAL_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const NANO_UNITS_PER_DOLLAR: f64 = 1_000_000_000.0;

#[derive(Debug, Clone, PartialEq)]
struct BillingSnapshot {
    used_usd: f64,
    included_usd: f64,
    billable_usd: f64,
    limit_usd: Option<f64>,
    requests: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
struct ZeroGpuSnapshot {
    used_minutes: f64,
    remaining_minutes: f64,
    total_minutes: f64,
    resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IdentitySnapshot {
    name: Option<String>,
    email: Option<String>,
    plan: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct TokenEnvironment {
    config_api_key: Option<String>,
    hf_token: Option<String>,
    hub_token: Option<String>,
    token_path: Option<PathBuf>,
    hf_home: Option<PathBuf>,
    xdg_cache_home: Option<PathBuf>,
    default_cache_dir: Option<PathBuf>,
    home_dir: Option<PathBuf>,
}

impl TokenEnvironment {
    fn from_process() -> Self {
        Self {
            config_api_key: std::env::var("CODEXBAR_HUGGINGFACE_API_KEY").ok(),
            hf_token: std::env::var("HF_TOKEN").ok(),
            hub_token: std::env::var("HUGGING_FACE_HUB_TOKEN").ok(),
            token_path: std::env::var_os("HF_TOKEN_PATH").map(PathBuf::from),
            hf_home: std::env::var_os("HF_HOME").map(PathBuf::from),
            xdg_cache_home: std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from),
            default_cache_dir: dirs::cache_dir(),
            home_dir: dirs::home_dir(),
        }
    }

    fn resolve(&self) -> Option<String> {
        for candidate in [
            self.config_api_key.as_deref(),
            self.hf_token.as_deref(),
            self.hub_token.as_deref(),
        ] {
            if let Some(token) = candidate.and_then(clean_token) {
                return Some(token);
            }
        }

        self.file_candidates()
            .into_iter()
            .find_map(|path| read_token_file(&path))
    }

    fn file_candidates(&self) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        let mut push_unique = |path: PathBuf| {
            if !candidates.contains(&path) {
                candidates.push(path);
            }
        };

        if let Some(path) = self.token_path.as_deref() {
            push_unique(expand_tilde(path, self.home_dir.as_deref()));
        }
        if let Some(path) = self.hf_home.as_deref() {
            push_unique(expand_tilde(path, self.home_dir.as_deref()).join("token"));
        }
        if let Some(path) = self.xdg_cache_home.as_deref() {
            push_unique(
                expand_tilde(path, self.home_dir.as_deref())
                    .join("huggingface")
                    .join("token"),
            );
        }

        let fallback_cache = self
            .default_cache_dir
            .clone()
            .or_else(|| self.home_dir.as_deref().map(|home| home.join(".cache")));
        if let Some(path) = fallback_cache {
            push_unique(path.join("huggingface").join("token"));
        }

        candidates
    }
}

pub struct HuggingFaceProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl HuggingFaceProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::HuggingFace,
                display_name: "Hugging Face",
                session_label: "Credits",
                weekly_label: "ZeroGPU",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://huggingface.co/settings/billing"),
                status_page_url: Some("https://status.huggingface.co"),
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(PRIMARY_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_api(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let token = resolve_token(ctx)?;
        let now = Utc::now();
        let billing_url = billing_url(now)?;
        let whoami_url = Url::parse(WHOAMI_URL)
            .map_err(|_| ProviderError::Other("Invalid Hugging Face whoami URL.".to_string()))?;
        let zerogpu_url = Url::parse(ZEROGPU_URL)
            .map_err(|_| ProviderError::Other("Invalid Hugging Face ZeroGPU URL.".to_string()))?;

        let (billing, identity, zerogpu) = tokio::join!(
            self.fetch_json(billing_url, &token, PRIMARY_TIMEOUT),
            self.fetch_optional_json(whoami_url, &token),
            self.fetch_optional_json(zerogpu_url, &token),
        );
        let billing = parse_billing(billing?)?;
        let identity = identity.and_then(|value| parse_identity(&value));
        let zerogpu = zerogpu.and_then(|value| parse_zerogpu(&value));

        Ok(build_result(billing, identity, zerogpu))
    }

    async fn fetch_optional_json(&self, url: Url, token: &str) -> Option<Value> {
        self.fetch_json(url, token, OPTIONAL_TIMEOUT).await.ok()
    }

    async fn fetch_json(
        &self,
        url: Url,
        token: &str,
        timeout: Duration,
    ) -> Result<Value, ProviderError> {
        // Wrapper timeout, not just the client's PRIMARY_TIMEOUT: the
        // optional-fetch path (fetch_optional_json) overrides this with
        // OPTIONAL_TIMEOUT (2s) so enrichment cannot stall the main fetch.
        tokio::time::timeout(timeout, async {
            let response = self
                .client
                .get(url)
                .bearer_auth(token)
                .header(reqwest::header::USER_AGENT, USER_AGENT)
                .header(reqwest::header::ACCEPT, "application/json")
                .send()
                .await?;
            let status = response.status();
            if !status.is_success() {
                return Err(classify_status(status));
            }

            let mut body = Vec::new();
            let mut stream = response.bytes_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| {
                    ProviderError::Parse(
                        "Hugging Face returned an unreadable JSON body.".to_string(),
                    )
                })?;
                body.extend_from_slice(&chunk);
                if body.len() > MAX_RESPONSE_BYTES {
                    return Err(ProviderError::Parse(
                        "Hugging Face returned an oversized JSON body.".to_string(),
                    ));
                }
            }
            serde_json::from_slice(&body).map_err(|_| {
                ProviderError::Parse("Hugging Face returned invalid JSON.".to_string())
            })
        })
        .await
        .map_err(|_| ProviderError::Timeout)?
    }
}

impl Default for HuggingFaceProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for HuggingFaceProvider {
    fn id(&self) -> ProviderId {
        ProviderId::HuggingFace
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            // The shared source enum uses OAuth as the persisted token/API
            // lane for providers whose transport is not an OAuth flow.
            SourceMode::Auto | SourceMode::OAuth => self.fetch_api(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

fn resolve_token(ctx: &FetchContext) -> Result<String, ProviderError> {
    if let Ok(raw) =
        crate::providers::resolve_api_key(ctx.api_key.as_deref(), CREDENTIAL_TARGET, &[])
        && let Some(token) = clean_token(&raw)
    {
        return Ok(token);
    }

    TokenEnvironment::from_process().resolve().ok_or_else(|| {
        ProviderError::NotInstalled(
            "Missing Hugging Face token. Add one in Settings, set HF_TOKEN, or run hf auth login."
                .to_string(),
        )
    })
}

fn clean_token(raw: &str) -> Option<String> {
    let mut value = raw.trim();
    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value = value[1..value.len() - 1].trim();
    }
    (!value.is_empty()).then_some(value.to_string())
}

fn expand_tilde(path: &Path, home: Option<&Path>) -> PathBuf {
    let raw = path.to_string_lossy();
    let Some(home) = home else {
        return path.to_path_buf();
    };
    if raw == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("~\\")) {
        return home.join(rest);
    }
    path.to_path_buf()
}

fn read_token_file(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()?
        .lines()
        .find_map(clean_token)
}

fn billing_url(now: DateTime<Utc>) -> Result<Url, ProviderError> {
    let mut url = Url::parse(BILLING_URL)
        .map_err(|_| ProviderError::Other("Invalid Hugging Face billing URL.".to_string()))?;
    let start = month_start(now).timestamp().to_string();
    let end = now.timestamp().to_string();
    url.query_pairs_mut()
        .append_pair("startDate", &start)
        .append_pair("endDate", &end);
    Ok(url)
}

fn month_start(now: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .unwrap_or_else(Utc::now)
}

fn parse_billing(value: Value) -> Result<BillingSnapshot, ProviderError> {
    let inference = value
        .get("usage")
        .and_then(|usage| usage.get("inferenceProviders"))
        .ok_or_else(|| invalid_billing("inferenceProviders"))?;
    let used_nano = required_nonnegative_number(inference, "usedNanoUsd")?;
    let included_nano = required_nonnegative_number(inference, "includedNanoUsd")?;
    let limit_usd = optional_nonnegative_number(inference, "limitNanoUsd")
        .map(|value| value / NANO_UNITS_PER_DOLLAR)
        .filter(|value| *value > 0.0);
    let requests = inference.get("numRequests").and_then(Value::as_u64);

    let used_usd = used_nano / NANO_UNITS_PER_DOLLAR;
    let included_usd = included_nano / NANO_UNITS_PER_DOLLAR;
    let billable_usd = (used_nano - included_nano).max(0.0) / NANO_UNITS_PER_DOLLAR;
    Ok(BillingSnapshot {
        used_usd,
        included_usd,
        billable_usd,
        limit_usd,
        requests,
    })
}

fn required_nonnegative_number(object: &Value, field: &str) -> Result<f64, ProviderError> {
    let Some(value) = object.get(field).and_then(Value::as_f64) else {
        return Err(invalid_billing(field));
    };
    if !value.is_finite() || value < 0.0 {
        return Err(invalid_billing(field));
    }
    Ok(value)
}

fn optional_nonnegative_number(object: &Value, field: &str) -> Option<f64> {
    object
        .get(field)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value >= 0.0)
}

fn invalid_billing(field: &str) -> ProviderError {
    ProviderError::Parse(format!("Hugging Face billing field '{field}' was invalid."))
}

fn parse_zerogpu(value: &Value) -> Option<ZeroGpuSnapshot> {
    let total_minutes = optional_nonnegative_number(value, "base")?;
    if total_minutes <= 0.0 {
        return None;
    }
    let current_minutes = optional_nonnegative_number(value, "current")?;
    let used_minutes = (total_minutes - current_minutes).max(0.0);
    let remaining_minutes = current_minutes.min(total_minutes);
    let resets_at = value.get("resetsAt").and_then(parse_timestamp);
    Some(ZeroGpuSnapshot {
        used_minutes,
        remaining_minutes,
        total_minutes,
        resets_at,
    })
}

fn parse_timestamp(value: &Value) -> Option<DateTime<Utc>> {
    value
        .as_i64()
        .and_then(|seconds| Utc.timestamp_opt(seconds, 0).single())
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
                .map(|date| date.with_timezone(&Utc))
        })
}

fn parse_identity(value: &Value) -> Option<IdentitySnapshot> {
    let name = safe_text(value.get("name").and_then(Value::as_str));
    let email = safe_text(value.get("email").and_then(Value::as_str));
    let plan = value
        .get("isPro")
        .and_then(Value::as_bool)
        .map(|is_pro| if is_pro { "Pro" } else { "Free" }.to_string());
    (name.is_some() || email.is_some() || plan.is_some()).then_some(IdentitySnapshot {
        name,
        email,
        plan,
    })
}

fn safe_text(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() || value.chars().count() > 256 || value.chars().any(char::is_control) {
        return None;
    }
    Some(value.to_string())
}

fn build_result(
    billing: BillingSnapshot,
    identity: Option<IdentitySnapshot>,
    zerogpu: Option<ZeroGpuSnapshot>,
) -> ProviderFetchResult {
    let mut result = ProviderFetchResult::new(
        UsageSnapshot::new(RateWindow::informational("Hugging Face billing"))
            .with_primary_label("Credits"),
        "api",
    )
    .with_non_authoritative_pace();

    let mut cost = CostSnapshot::new(billing.billable_usd, "USD", "Current month");
    if let Some(limit) = billing.limit_usd {
        cost = cost.with_limit(limit);
    }
    result = result.with_cost(cost);

    let mut details: Vec<(&str, &str, String)> = vec![
        (
            "billable-usage",
            "Billable inference usage",
            format_usd(billing.billable_usd),
        ),
        (
            "gross-inference-usage",
            "Gross inference usage",
            format_usd(billing.used_usd),
        ),
        (
            "included-inference-amount",
            "Included inference amount",
            format_usd(billing.included_usd),
        ),
    ];
    if let Some(limit) = billing.limit_usd {
        details.push(("spending-limit", "Spending limit", format_usd(limit)));
    }
    if let Some(requests) = billing.requests {
        details.push(("inference-requests", "Requests", requests.to_string()));
    }

    let mut rows: Vec<Option<ProviderDisplayDetail>> = details
        .into_iter()
        .map(|(id, title, value)| ProviderDisplayDetail::new(id, title, value))
        .collect();

    if let Some(identity_row) = identity {
        if let Some(name) = identity_row.name {
            rows.push(ProviderDisplayDetail::new("account-name", "Account", name));
        }
        if let Some(email) = identity_row.email {
            rows.push(ProviderDisplayDetail::new("account-email", "Email", email));
        }
        if let Some(plan) = identity_row.plan {
            rows.push(ProviderDisplayDetail::new("account-plan", "Plan", plan));
        }
    }

    if let Some(zerogpu) = zerogpu {
        let reset = zerogpu
            .resets_at
            .map(|date| format!(" · resets {}", date.to_rfc3339()))
            .unwrap_or_default();
        rows.push(
            ProviderDisplayDetail::new(
                "zerogpu-quota",
                "ZeroGPU quota",
                format!("{:.0} minutes used", zerogpu.used_minutes),
            )
            .and_then(|row| {
                row.with_secondary_value(format!(
                    "{:.0} minutes remaining{reset}",
                    zerogpu.remaining_minutes
                ))
            })
            .and_then(|row| row.with_progress(zerogpu.used_minutes, zerogpu.total_minutes)),
        );
    }

    for row in rows {
        result = result.with_display_detail(row);
    }
    result
}

fn format_usd(value: f64) -> String {
    format!("${value:.2}")
}

fn classify_status(status: StatusCode) -> ProviderError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => ProviderError::AuthRequired,
        StatusCode::TOO_MANY_REQUESTS => {
            ProviderError::Other("Hugging Face API rate limited (HTTP 429).".to_string())
        }
        status if status.is_server_error() => {
            ProviderError::Other("Hugging Face service unavailable (HTTP 5xx).".to_string())
        }
        status => ProviderError::Other(format!("Hugging Face API request failed (HTTP {status}).")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn fixture_environment() -> TokenEnvironment {
        TokenEnvironment {
            default_cache_dir: None,
            home_dir: None,
            ..TokenEnvironment::default()
        }
    }

    #[test]
    fn provider_is_api_only_and_disabled_by_default() {
        let provider = HuggingFaceProvider::new();
        assert_eq!(provider.id(), ProviderId::HuggingFace);
        assert_eq!(
            provider.available_sources(),
            vec![SourceMode::Auto, SourceMode::OAuth]
        );
        assert!(!provider.metadata().default_enabled);
        assert_eq!(provider.metadata().session_label, "Credits");
    }

    #[test]
    fn token_environment_precedence_and_quote_cleanup_are_deterministic() {
        let mut environment = fixture_environment();
        environment.config_api_key = Some("  \"configured\"  ".to_string());
        environment.hf_token = Some("hf_env".to_string());
        environment.hub_token = Some("hf_legacy".to_string());
        assert_eq!(environment.resolve().as_deref(), Some("configured"));

        environment.config_api_key = Some("  \"\"  ".to_string());
        assert_eq!(environment.resolve().as_deref(), Some("hf_env"));
        environment.hf_token = Some("  ".to_string());
        assert_eq!(environment.resolve().as_deref(), Some("hf_legacy"));
    }

    #[test]
    fn token_files_follow_explicit_home_xdg_and_default_order() {
        let root = tempdir().unwrap();
        let explicit = root.path().join("explicit-token");
        let hf_home = root.path().join("hf-home");
        let xdg = root.path().join("xdg");
        std::fs::write(&explicit, "\n  'explicit-token'  \nsecond").unwrap();
        std::fs::create_dir_all(&hf_home).unwrap();
        std::fs::write(hf_home.join("token"), "hf-home-token").unwrap();
        std::fs::create_dir_all(xdg.join("huggingface")).unwrap();
        std::fs::write(xdg.join("huggingface/token"), "xdg-token").unwrap();

        let environment = TokenEnvironment {
            token_path: Some(explicit),
            hf_home: Some(hf_home),
            xdg_cache_home: Some(xdg),
            ..fixture_environment()
        };
        assert_eq!(environment.resolve().as_deref(), Some("explicit-token"));
    }

    #[test]
    fn token_file_reads_first_nonempty_line_and_expands_tilde() {
        let root = tempdir().unwrap();
        let file = root.path().join("hf/token");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "\n\"from-file\"\nignored").unwrap();
        let environment = TokenEnvironment {
            token_path: Some(PathBuf::from("~/hf/token")),
            home_dir: Some(root.path().to_path_buf()),
            ..fixture_environment()
        };
        assert_eq!(environment.resolve().as_deref(), Some("from-file"));
    }

    #[test]
    fn billing_url_uses_utc_month_start_and_current_end() {
        let now = DateTime::parse_from_rfc3339("2026-09-19T16:47:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let url = billing_url(now).unwrap();
        let query = url.query_pairs().collect::<Vec<_>>();
        assert_eq!(query[0].0, "startDate");
        assert_eq!(query[0].1, month_start(now).timestamp().to_string());
        assert_eq!(query[1].0, "endDate");
        assert_eq!(query[1].1, now.timestamp().to_string());
    }

    #[test]
    fn billing_parser_converts_nano_usd_and_preserves_optional_fields() {
        let parsed = parse_billing(json!({
            "usage": {"inferenceProviders": {
                "usedNanoUsd": 2_450_000_000_u64,
                "includedNanoUsd": 2_000_000_000_u64,
                "limitNanoUsd": 10_000_000_000_u64,
                "numRequests": 7_u64
            }}
        }))
        .unwrap();
        assert!((parsed.used_usd - 2.45).abs() < f64::EPSILON);
        assert!((parsed.included_usd - 2.0).abs() < f64::EPSILON);
        assert!((parsed.billable_usd - 0.45).abs() < f64::EPSILON);
        assert_eq!(parsed.limit_usd, Some(10.0));
        assert_eq!(parsed.requests, Some(7));
    }

    #[test]
    fn billing_parser_clamps_included_amount_above_gross_to_zero() {
        let parsed = parse_billing(json!({
            "usage": {"inferenceProviders": {
                "usedNanoUsd": 1_000_000_u64,
                "includedNanoUsd": 2_000_000_u64
            }}
        }))
        .unwrap();
        assert_eq!(parsed.billable_usd, 0.0);
    }

    #[test]
    fn billing_parser_rejects_missing_wrong_type_and_negative_required_values() {
        for payload in [
            json!({"usage": {"inferenceProviders": {"usedNanoUsd": 1}}}),
            json!({"usage": {"inferenceProviders": {
                "usedNanoUsd": "1", "includedNanoUsd": 1
            }}}),
            json!({"usage": {"inferenceProviders": {
                "usedNanoUsd": -1, "includedNanoUsd": 1
            }}}),
        ] {
            assert!(matches!(
                parse_billing(payload),
                Err(ProviderError::Parse(_))
            ));
        }
    }

    #[test]
    fn invalid_optional_billing_fields_are_omitted() {
        let parsed = parse_billing(json!({
            "usage": {"inferenceProviders": {
                "usedNanoUsd": 1_000_000_u64,
                "includedNanoUsd": 0_u64,
                "limitNanoUsd": "bad"
            }}
        }))
        .unwrap();
        assert_eq!(parsed.limit_usd, None);
        assert_eq!(parsed.requests, None);
    }

    #[test]
    fn invalid_fractional_or_negative_request_counts_are_omitted() {
        for requests in [json!(-1), json!(1.5)] {
            let parsed = parse_billing(json!({
                "usage": {"inferenceProviders": {
                    "usedNanoUsd": 1, "includedNanoUsd": 0, "numRequests": requests
                }}
            }))
            .unwrap();
            assert_eq!(parsed.requests, None);
        }
    }

    #[test]
    fn zerogpu_parser_requires_a_valid_positive_total_and_keeps_reset_optional() {
        let parsed =
            parse_zerogpu(&json!({"base": 1500, "current": 900, "resetsAt": 1_800_000_000}))
                .unwrap();
        assert_eq!(parsed.used_minutes, 600.0);
        assert_eq!(parsed.remaining_minutes, 900.0);
        assert_eq!(parsed.total_minutes, 1500.0);
        assert_eq!(parsed.resets_at.unwrap().timestamp(), 1_800_000_000);
        assert!(parse_zerogpu(&json!({"base": "bad", "current": 900})).is_none());
        assert!(parse_zerogpu(&json!({"base": 0, "current": 0})).is_none());
    }

    #[test]
    fn identity_parser_is_optional_and_sanitized() {
        let identity =
            parse_identity(&json!({"name": "ness", "email": "n@example.test", "isPro": true}))
                .unwrap();
        assert_eq!(identity.name.as_deref(), Some("ness"));
        assert_eq!(identity.email.as_deref(), Some("n@example.test"));
        assert_eq!(identity.plan.as_deref(), Some("Pro"));
        assert!(parse_identity(&json!({"email": "bad\nemail"})).is_none());
    }

    #[test]
    fn status_errors_are_classified_without_response_body_or_token() {
        let token = "hf_secret_fixture";
        for status in [
            StatusCode::FORBIDDEN,
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::BAD_REQUEST,
        ] {
            let error = classify_status(status).to_string();
            assert!(!error.contains(token));
            assert!(!error.contains("response body"));
        }
        assert!(matches!(
            classify_status(StatusCode::UNAUTHORIZED),
            ProviderError::AuthRequired
        ));
        assert!(matches!(
            classify_status(StatusCode::FORBIDDEN),
            ProviderError::AuthRequired
        ));
    }

    #[test]
    fn result_uses_cost_and_transient_details_without_quota_windows() {
        let result = build_result(
            BillingSnapshot {
                used_usd: 2.45,
                included_usd: 2.0,
                billable_usd: 0.45,
                limit_usd: Some(10.0),
                requests: Some(7),
            },
            None,
            Some(ZeroGpuSnapshot {
                used_minutes: 600.0,
                remaining_minutes: 900.0,
                total_minutes: 1500.0,
                resets_at: None,
            }),
        );
        assert_eq!(result.source_label, "api");
        assert_eq!(result.cost.as_ref().and_then(|cost| cost.limit), Some(10.0));
        assert_eq!(result.display_details().len(), 6);
        assert!(result.usage.primary.is_informational);
        assert!(result.usage.secondary.is_none());
        assert!(!result.pace_authoritative);
    }
}
