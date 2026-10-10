//! Groq console usage: spend, requests and tokens from the console platform
//! API (`/platform/v1/organizations/{org}/activity`), authenticated with the
//! console's Stytch session.
//!
//! Mirrors upstream 0.70.0 `GroqConsoleFetcher`, `GroqConsoleSession`,
//! `GroqConsoleStytch` and `GroqConsoleUsageSnapshot`.

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use chrono::{DateTime, Days, NaiveDate, NaiveTime, TimeZone, Utc};
use reqwest::{Client, StatusCode, Url};
use serde::Deserialize;

use crate::core::{
    CostSnapshot, OpenAiApiDailyUsage, OpenAiApiModelUsage, OpenAiApiUsageHistory,
    ProviderDisplayDetail, ProviderError, ProviderFetchResult, RateWindow, UsageSnapshot,
};

pub(super) const SESSION_COOKIE: &str = "stytch_session";
pub(super) const JWT_COOKIE: &str = "stytch_session_jwt";
/// Browser cookie matching includes subdomains, so this covers console.groq.com.
pub(super) const COOKIE_DOMAIN: &str = "groq.com";
const SESSION_JWT_ENV: &str = "GROQ_SESSION_JWT";
const SESSION_TOKEN_ENV: &str = "GROQ_SESSION_TOKEN";
const STYTCH_URL_ENV: &str = "GROQ_STYTCH_URL";
const STYTCH_PUBLIC_TOKEN_ENV: &str = "GROQ_STYTCH_PUBLIC_TOKEN";
const DEFAULT_STYTCH_URL: &str = "https://api.stytchb2b.groq.com";
/// Publishable token of Groq's Stytch B2B project; it only authorizes SDK
/// calls from the `console.groq.com` origin.
const DEFAULT_STYTCH_PUBLIC_TOKEN: &str = "public-token-live-58df57a9-a1f5-4066-bc0c-2ff942db684f";
const CONSOLE_ORIGIN: &str = "https://console.groq.com";
const STYTCH_SDK_CLIENT: &str = r#"{"app":{"identifier":"console.groq.com"},"sdk":{"identifier":"Stytch.js Javascript SDK","version":"5.43.0"}}"#;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
pub(super) const HISTORY_DAYS: u32 = 30;
const MAX_MODEL_ROWS: usize = 20;
const LOGIN_METHOD: &str = "Console";
const SUMMARY_SECTION: &str = "Usage summary";
const MODELS_SECTION: &str = "Models";

/// Why the console path produced no usage. The first three mean "no usable
/// session" and let Auto fall back to the Prometheus API-key path.
#[derive(Debug)]
pub(super) enum ConsoleError {
    MissingSession,
    InvalidSession(String),
    AccessDenied(String),
    Api(String),
    Parse(String),
    Transport(ProviderError),
}

impl ConsoleError {
    pub(super) fn allows_metrics_fallback(&self) -> bool {
        matches!(
            self,
            Self::MissingSession | Self::InvalidSession(_) | Self::AccessDenied(_)
        )
    }
}

impl From<ConsoleError> for ProviderError {
    fn from(error: ConsoleError) -> Self {
        match error {
            ConsoleError::MissingSession => ProviderError::Other(
                "No Groq console session found. Sign in at console.groq.com in your browser."
                    .to_string(),
            ),
            ConsoleError::InvalidSession(message) => {
                ProviderError::Other(format!("Groq console session is invalid: {message}"))
            }
            ConsoleError::AccessDenied(message) => {
                ProviderError::Other(format!("Groq console access denied: {message}"))
            }
            ConsoleError::Api(message) => {
                ProviderError::Other(format!("Groq console API error: {message}"))
            }
            ConsoleError::Parse(message) => {
                ProviderError::Parse(format!("Groq console response: {message}"))
            }
            ConsoleError::Transport(error) => error,
        }
    }
}

impl From<reqwest::Error> for ConsoleError {
    fn from(error: reqwest::Error) -> Self {
        Self::Transport(error.into())
    }
}

/// A console session: the long-lived opaque `stytch_session` token (exchanged
/// for a fresh JWT) and/or the short-lived `stytch_session_jwt`.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct ConsoleSession {
    pub(super) session_token: Option<String>,
    pub(super) direct_jwt: Option<String>,
}

impl std::fmt::Debug for ConsoleSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let redacted = |value: &Option<String>| value.as_ref().map(|_| "<redacted>");
        f.debug_struct("ConsoleSession")
            .field("session_token", &redacted(&self.session_token))
            .field("direct_jwt", &redacted(&self.direct_jwt))
            .finish()
    }
}

impl ConsoleSession {
    /// `GROQ_SESSION_TOKEN` / `GROQ_SESSION_JWT` override, checked before any
    /// cookie source.
    pub(super) fn from_env(env: impl Fn(&str) -> Option<String>) -> Option<Self> {
        Self::new(env(SESSION_TOKEN_ENV), env(SESSION_JWT_ENV))
    }

    pub(super) fn from_cookie_header(header: &str) -> Option<Self> {
        let header = crate::providers::normalize_cookie_header(header)?;
        let last = |name| {
            crate::providers::cookie_values(&header, name)
                .into_iter()
                .rfind(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        };
        Self::new(last(SESSION_COOKIE), last(JWT_COOKIE))
    }

    fn new(session_token: Option<String>, direct_jwt: Option<String>) -> Option<Self> {
        let clean = |value: Option<String>| {
            value
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };
        let session = Self {
            session_token: clean(session_token),
            direct_jwt: clean(direct_jwt),
        };
        (session.session_token.is_some() || session.direct_jwt.is_some()).then_some(session)
    }
}

/// Where the console path sends its requests.
pub(super) struct ConsoleEndpoints {
    /// Only the scheme, host and port are used; the activity path replaces
    /// whatever path the base has.
    pub(super) api_base: Url,
    pub(super) stytch_base: Url,
    pub(super) stytch_public_token: String,
}

impl ConsoleEndpoints {
    pub(super) fn from_env(api_base: Url) -> Self {
        let stytch_base = std::env::var(STYTCH_URL_ENV)
            .ok()
            .and_then(|raw| crate::providers::validated_https_url(&raw, "Groq Stytch").ok())
            .unwrap_or_else(|| Url::parse(DEFAULT_STYTCH_URL).expect("static Stytch URL is valid"));
        let stytch_public_token = std::env::var(STYTCH_PUBLIC_TOKEN_ENV)
            .ok()
            .map(|token| token.trim().to_owned())
            .filter(|token| !token.is_empty())
            .unwrap_or_else(|| DEFAULT_STYTCH_PUBLIC_TOKEN.to_owned());
        Self {
            api_base,
            stytch_base,
            stytch_public_token,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ActivityResponse {
    data: Vec<ActivityRow>,
}

/// One per-model, per-day row of the activity listing.
#[derive(Debug, Deserialize)]
pub(super) struct ActivityRow {
    organization_name: Option<String>,
    model: Option<String>,
    timestamp: f64,
    num_requests: Option<u64>,
    n_context_tokens_total: Option<u64>,
    n_non_cached_context_tokens_total: Option<u64>,
    n_generated_tokens_total: Option<u64>,
    cost: Option<f64>,
}

pub(super) fn parse_activity(body: &[u8]) -> Result<Vec<ActivityRow>, ConsoleError> {
    serde_json::from_slice::<ActivityResponse>(body)
        .map(|response| response.data)
        .map_err(|error| ConsoleError::Parse(error.to_string()))
}

/// Try the sessions in order and move on when one is rejected, like upstream's
/// `shouldRetryNextSession`; any other error stops the search.
pub(super) async fn fetch_first_usable(
    client: &Client,
    endpoints: &ConsoleEndpoints,
    sessions: &[ConsoleSession],
    now: DateTime<Utc>,
) -> Result<ProviderFetchResult, ConsoleError> {
    let mut last_error = ConsoleError::MissingSession;
    for session in sessions {
        match fetch_usage(client, endpoints, session, now).await {
            Err(error @ (ConsoleError::AccessDenied(_) | ConsoleError::InvalidSession(_))) => {
                last_error = error;
            }
            result => return result,
        }
    }
    Err(last_error)
}

async fn fetch_usage(
    client: &Client,
    endpoints: &ConsoleEndpoints,
    session: &ConsoleSession,
    now: DateTime<Utc>,
) -> Result<ProviderFetchResult, ConsoleError> {
    let jwt = resolve_jwt(client, endpoints, session).await?;
    let org = organization_id(&jwt).ok_or_else(|| {
        ConsoleError::InvalidSession("session token is missing the organization claim".into())
    })?;
    let local = chrono::Local;
    let (start, end) = history_window(&now.with_timezone(&local), HISTORY_DAYS);
    let url = activity_url(&endpoints.api_base, &org, start, end)?;
    let rows = fetch_activity(client, url, &jwt).await?;
    let organization = organization_name(&rows);
    let daily = daily_usage(&rows, &local);
    Ok(build_result(daily, organization, now))
}

/// Refresh the opaque session token when there is one, keeping a direct JWT
/// as the fallback if the refresh fails.
async fn resolve_jwt(
    client: &Client,
    endpoints: &ConsoleEndpoints,
    session: &ConsoleSession,
) -> Result<String, ConsoleError> {
    if let Some(token) = &session.session_token {
        match refresh_session_jwt(client, endpoints, token).await {
            Ok(jwt) => return Ok(jwt),
            Err(error) => {
                if let Some(jwt) = &session.direct_jwt {
                    tracing::debug!("Groq console session refresh failed; using the session JWT");
                    return Ok(jwt.clone());
                }
                return Err(error);
            }
        }
    }
    session
        .direct_jwt
        .clone()
        .ok_or(ConsoleError::MissingSession)
}

#[derive(Debug, Deserialize)]
struct StytchResponse {
    data: Option<StytchPayload>,
}

#[derive(Debug, Deserialize)]
struct StytchPayload {
    session_jwt: Option<String>,
}

async fn refresh_session_jwt(
    client: &Client,
    endpoints: &ConsoleEndpoints,
    session_token: &str,
) -> Result<String, ConsoleError> {
    let mut url = endpoints.stytch_base.clone();
    url.set_query(None);
    url.set_fragment(None);
    url.path_segments_mut()
        .map_err(|()| ConsoleError::InvalidSession("invalid Stytch URL".into()))?
        .pop_if_empty()
        .extend(["sdk", "v1", "b2b", "sessions", "authenticate"]);
    let credential = STANDARD.encode(format!("{}:{session_token}", endpoints.stytch_public_token));
    let response = client
        .post(url)
        .timeout(REQUEST_TIMEOUT)
        .header("Authorization", format!("Basic {credential}"))
        .header("Origin", CONSOLE_ORIGIN)
        .header("X-SDK-Parent-Host", CONSOLE_ORIGIN)
        .header("X-SDK-Client", STANDARD.encode(STYTCH_SDK_CLIENT))
        .json(&serde_json::json!({
            "session_token": session_token,
            "session_duration_minutes": 30,
        }))
        .send()
        .await?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(ConsoleError::AccessDenied(format!(
            "Stytch HTTP {}",
            status.as_u16()
        )));
    }
    if !status.is_success() {
        return Err(ConsoleError::Api(format!(
            "Stytch HTTP {}",
            status.as_u16()
        )));
    }
    let body = read_body(response).await?;
    serde_json::from_slice::<StytchResponse>(&body)
        .ok()
        .and_then(|response| response.data)
        .and_then(|data| data.session_jwt)
        .map(|jwt| jwt.trim().to_owned())
        .filter(|jwt| !jwt.is_empty())
        .ok_or_else(|| ConsoleError::Parse("Stytch response missing session_jwt".into()))
}

async fn fetch_activity(
    client: &Client,
    url: Url,
    jwt: &str,
) -> Result<Vec<ActivityRow>, ConsoleError> {
    let response = client
        .get(url)
        .timeout(REQUEST_TIMEOUT)
        .bearer_auth(jwt)
        .header("Accept", "application/json")
        .send()
        .await?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(ConsoleError::AccessDenied(format!(
            "HTTP {}",
            status.as_u16()
        )));
    }
    if !status.is_success() {
        return Err(ConsoleError::Api(format!("HTTP {}", status.as_u16())));
    }
    parse_activity(&read_body(response).await?)
}

async fn read_body(response: reqwest::Response) -> Result<Vec<u8>, ConsoleError> {
    crate::providers::read_bounded_response(response, MAX_BODY_BYTES)
        .await
        .map_err(|error| match error {
            crate::providers::BoundedBodyError::TooLarge => {
                ConsoleError::Parse("response is too large".into())
            }
            crate::providers::BoundedBodyError::Read(error) => error.into(),
        })
}

/// The org id from the JWT's `https://groq.com/organization` claim (Stytch's
/// organization slug as a fallback). The signature is not checked: the API
/// authenticates the token, this only reads the routing claim.
pub(super) fn organization_id(jwt: &str) -> Option<String> {
    let payload = jwt.trim().split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let claim = |claim: &str, field: &str| {
        claims
            .get(claim)?
            .get(field)?
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    };
    claim("https://groq.com/organization", "id")
        .or_else(|| claim("https://stytch.com/organization", "slug"))
}

/// The API base's origin with the activity path; the console platform API is
/// rooted at the host, not under the public `/v1` base.
pub(super) fn activity_url(
    base: &Url,
    org: &str,
    start: i64,
    end: i64,
) -> Result<Url, ConsoleError> {
    let mut url = base.clone();
    url.set_query(None);
    url.set_fragment(None);
    url.path_segments_mut()
        .map_err(|()| ConsoleError::InvalidSession("could not build activity URL".into()))?
        .clear()
        .extend(["platform", "v1", "organizations", org, "activity"]);
    url.query_pairs_mut()
        .append_pair("start_date", &start.to_string())
        .append_pair("end_date", &end.to_string());
    Ok(url)
}

/// Unix seconds bounding the last `days` local calendar days, today included.
pub(super) fn history_window<Tz: TimeZone>(now: &DateTime<Tz>, days: u32) -> (i64, i64) {
    let tz = now.timezone();
    let today = now.date_naive();
    let first = today
        .checked_sub_days(Days::new(u64::from(days.clamp(1, 365) - 1)))
        .unwrap_or(today);
    let tomorrow = today.checked_add_days(Days::new(1)).unwrap_or(today);
    (
        start_of_day(&tz, first).timestamp(),
        start_of_day(&tz, tomorrow).timestamp(),
    )
}

fn start_of_day<Tz: TimeZone>(tz: &Tz, day: NaiveDate) -> DateTime<Tz> {
    let midnight = day.and_time(NaiveTime::MIN);
    tz.from_local_datetime(&midnight)
        .earliest()
        .unwrap_or_else(|| tz.from_utc_datetime(&midnight))
}

pub(super) fn organization_name(rows: &[ActivityRow]) -> Option<String> {
    rows.iter()
        .filter_map(|row| row.organization_name.as_deref())
        .find(|name| !name.is_empty())
        .map(ToOwned::to_owned)
}

/// Bucket the rows by local day in `tz`, then by model. Cached input is the
/// context tokens the API did not count as non-cached. Each day is stamped at
/// UTC midnight of its local date because the shared daily chart labels bars
/// by the UTC date of `start_time`.
pub(super) fn daily_usage<Tz: TimeZone>(rows: &[ActivityRow], tz: &Tz) -> Vec<OpenAiApiDailyUsage> {
    let mut days: BTreeMap<NaiveDate, HashMap<String, (OpenAiApiModelUsage, f64)>> =
        BTreeMap::new();
    for row in rows {
        if !row.timestamp.is_finite() {
            continue;
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "the cast saturates and timestamp_opt rejects out-of-range seconds"
        )]
        let seconds = row.timestamp.floor() as i64;
        let Some(time) = tz.timestamp_opt(seconds, 0).single() else {
            continue;
        };
        let name = row
            .model
            .as_deref()
            .filter(|model| !model.is_empty())
            .unwrap_or("unknown");
        let context = row.n_context_tokens_total.unwrap_or(0);
        let non_cached = row.n_non_cached_context_tokens_total.unwrap_or(context);
        let generated = row.n_generated_tokens_total.unwrap_or(0);
        let (model, cost) = days
            .entry(time.date_naive())
            .or_default()
            .entry(name.to_owned())
            .or_insert_with(|| (empty_model(name), 0.0));
        model.requests = model.requests.saturating_add(row.num_requests.unwrap_or(0));
        model.input_tokens = model.input_tokens.saturating_add(non_cached);
        model.cached_input_tokens = model
            .cached_input_tokens
            .saturating_add(context.saturating_sub(non_cached));
        model.output_tokens = model.output_tokens.saturating_add(generated);
        model.total_tokens = model
            .total_tokens
            .saturating_add(context.saturating_add(generated));
        *cost += row.cost.unwrap_or(0.0);
    }
    days.into_iter()
        .map(|(day, models)| {
            let cost_usd = models.values().map(|(_, cost)| cost).sum();
            let mut models: Vec<_> = models.into_values().map(|(model, _)| model).collect();
            models.sort_by(|a, b| {
                b.total_tokens
                    .cmp(&a.total_tokens)
                    .then_with(|| a.name.cmp(&b.name))
            });
            let sum = |field: fn(&OpenAiApiModelUsage) -> u64| {
                models
                    .iter()
                    .map(field)
                    .fold(0u64, |total, value| total.saturating_add(value))
            };
            let start_time = day.and_time(NaiveTime::MIN).and_utc().timestamp();
            OpenAiApiDailyUsage {
                start_time,
                end_time: start_time + 86_400,
                cost_usd,
                requests: sum(|model| model.requests),
                input_tokens: sum(|model| model.input_tokens),
                cached_input_tokens: sum(|model| model.cached_input_tokens),
                output_tokens: sum(|model| model.output_tokens),
                total_tokens: sum(|model| model.total_tokens),
                line_items: Vec::new(),
                models,
            }
        })
        .collect()
}

fn empty_model(name: &str) -> OpenAiApiModelUsage {
    OpenAiApiModelUsage {
        name: name.to_owned(),
        requests: 0,
        input_tokens: 0,
        cached_input_tokens: 0,
        output_tokens: 0,
        total_tokens: 0,
    }
}

/// The Mac card: a "Usage summary" section (spend, requests, tokens, cached
/// input), a "Models" section, the daily chart and the window's spend.
pub(super) fn build_result(
    daily: Vec<OpenAiApiDailyUsage>,
    organization: Option<String>,
    now: DateTime<Utc>,
) -> ProviderFetchResult {
    let period = format!("Last {HISTORY_DAYS} days");
    let spend: f64 = daily.iter().map(|day| day.cost_usd).sum();
    let total = |field: fn(&OpenAiApiDailyUsage) -> u64| {
        daily
            .iter()
            .map(field)
            .fold(0u64, |sum, value| sum.saturating_add(value))
    };
    let requests = total(|day| day.requests);
    let tokens = total(|day| day.total_tokens);
    let cached = total(|day| day.cached_input_tokens);

    let mut usage = UsageSnapshot::new(RateWindow::informational(format!(
        "{} · {period}",
        usd(spend)
    )))
    .with_primary_label("Spend")
    .with_login_method(LOGIN_METHOD);
    if let Some(organization) = organization {
        usage = usage.with_organization(organization);
    }
    usage.updated_at = now;

    let mut details = vec![
        summary_row("spend", "Spend", usd(spend))
            .and_then(|row| row.with_secondary_value(period.as_str())),
        summary_row("requests", "Requests", count(requests)),
        summary_row("tokens", "Tokens", count(tokens)),
    ];
    if cached > 0 {
        details.push(summary_row("cached-input", "Cached input", count(cached)));
    }
    details.extend(
        top_models(&daily)
            .into_iter()
            .take(MAX_MODEL_ROWS)
            .enumerate()
            .map(|(index, (name, requests, tokens))| {
                ProviderDisplayDetail::new(
                    format!("model-{index}"),
                    name,
                    format!("{} tokens", count(tokens)),
                )
                .and_then(|row| row.with_secondary_value(format!("{} requests", count(requests))))
                .and_then(|row| row.with_section_title(MODELS_SECTION))
            }),
    );

    let history = OpenAiApiUsageHistory {
        history_days: HISTORY_DAYS,
        project_id: None,
        daily,
    };
    ProviderFetchResult::new(usage, "console")
        .with_cost(CostSnapshot::new(spend, "USD", period))
        .with_open_ai_api_usage(history)
        .with_display_details(details.into_iter().flatten())
}

fn summary_row(id: &str, title: &str, value: String) -> Option<ProviderDisplayDetail> {
    ProviderDisplayDetail::new(id, title, value)
        .and_then(|row| row.with_section_title(SUMMARY_SECTION))
}

/// Per-model totals across the window: (name, requests, tokens), most tokens
/// first, then by name.
fn top_models(daily: &[OpenAiApiDailyUsage]) -> Vec<(String, u64, u64)> {
    let mut totals: HashMap<&str, (u64, u64)> = HashMap::new();
    for model in daily.iter().flat_map(|day| &day.models) {
        let (requests, tokens) = totals.entry(&model.name).or_default();
        *requests = requests.saturating_add(model.requests);
        *tokens = tokens.saturating_add(model.total_tokens);
    }
    let mut models: Vec<_> = totals
        .into_iter()
        .map(|(name, (requests, tokens))| (name.to_owned(), requests, tokens))
        .collect();
    models.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    models
}

fn usd(value: f64) -> String {
    format!("${value:.2}")
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
