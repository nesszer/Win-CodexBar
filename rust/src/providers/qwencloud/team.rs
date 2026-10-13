//! Qwen Cloud Team token plan, mirroring upstream `qwencloud-team.js` (v0.74).
//!
//! The Team lane resolves its own `secToken` from `info.json`, discovers the
//! console billing selector with `LoadHumanInfo`, then reads the seat
//! subscription summary. `Ok(None)` means "no active Team plan" and lets the
//! Individual lane run; every other outcome is reported verbatim.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use crate::core::{
    ProviderDisplayDetail, ProviderError, ProviderFetchResult, RateWindow, UsageSnapshot,
};

pub(super) const BASE_URL: &str = "https://home.qwencloud.com";
/// Upstream's per-request timeout for the Team plugin.
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

const PRODUCT_CODE: &str = "sfm_tokenplanteams_dp_intl";
const SECTION_TITLE: &str = "Team Token Plan";
const LOGIN_CODES: &[&str] = &[
    "ConsoleNeedLogin",
    "BailianGateway.Login.NotLogined",
    "NO_LOGIN",
];
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

const SESSION_EXPIRED: &str = "Qwen Cloud Team session expired. Sign in again.";
const LOGIN_REQUIRED: &str = "Qwen Cloud Team login required.";
const RATE_LIMITED: &str = "Qwen Cloud Team rate limit reached.";

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TeamUsage {
    pub used: f64,
    pub total: f64,
    pub remaining: f64,
    pub resets_at: Option<DateTime<Utc>>,
    /// `(assigned, total)` seats, present only when upstream would show the row.
    pub seats: Option<(f64, f64)>,
    pub login_method: String,
}

/// Upstream raises both messages with `authenticationExpired`.
pub(super) fn is_auth_expired(error: &ProviderError) -> bool {
    matches!(error, ProviderError::Other(message)
        if message == SESSION_EXPIRED || message == LOGIN_REQUIRED)
}

fn fail(field: &str) -> ProviderError {
    ProviderError::Other(format!(
        "Unsupported Qwen Cloud Team {field}. Check Usage Dashboard."
    ))
}

fn record(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

/// Upstream `number()`: a JSON number, or a string of ASCII digits with an
/// optional fraction, finite and within `0..=MAX_SAFE_INTEGER`.
fn number(value: Option<&Value>) -> Result<f64, ProviderError> {
    let parsed = match value {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) if is_plain_decimal(s) => s.parse::<f64>().ok(),
        _ => None,
    };
    match parsed {
        Some(f) if f.is_finite() && (0.0..=MAX_SAFE_INTEGER).contains(&f) => Ok(f),
        _ => Err(fail("credit count")),
    }
}

fn is_plain_decimal(s: &str) -> bool {
    let (whole, fraction) = match s.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (s, None),
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    digits(whole) && fraction.is_none_or(digits)
}

fn is_safe_integer(f: f64) -> bool {
    f.is_finite() && f.fract() == 0.0 && f.abs() <= MAX_SAFE_INTEGER
}

/// Integral value already checked with [`is_safe_integer`]; `+ 0.0` maps -0 to 0.
fn safe_integer_text(f: f64) -> String {
    format!("{:.0}", f + 0.0)
}

/// Upstream `json()`: status mapping, object root, and login-code detection.
pub(super) fn parse_response(
    status: u16,
    body: &[u8],
) -> Result<Map<String, Value>, ProviderError> {
    match status {
        200 => {}
        401 | 403 => return Err(ProviderError::Other(SESSION_EXPIRED.into())),
        429 => return Err(ProviderError::Other(RATE_LIMITED.into())),
        other => {
            return Err(ProviderError::Other(format!(
                "Qwen Cloud Team returned HTTP {other}."
            )));
        }
    }
    let root = match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(root)) => root,
        _ => return Err(fail("response")),
    };
    if root
        .get("code")
        .and_then(Value::as_str)
        .is_some_and(|code| LOGIN_CODES.contains(&code))
    {
        return Err(ProviderError::Other(LOGIN_REQUIRED.into()));
    }
    Ok(root)
}

pub(super) fn sec_token(info: &Map<String, Value>) -> Result<String, ProviderError> {
    match record(info.get("data")).and_then(|data| data.get("secToken")) {
        Some(Value::String(token)) if !token.trim().is_empty() => Ok(token.clone()),
        _ => Err(ProviderError::Other(LOGIN_REQUIRED.into())),
    }
}

/// Upstream `rpc()` envelope checks, returning `root.data`.
pub(super) fn rpc_result(
    action: &str,
    root: &Map<String, Value>,
) -> Result<Map<String, Value>, ProviderError> {
    let result = record(root.get("data"));
    let success_response = root.get("successResponse");
    if success_response == Some(&Value::Bool(false))
        || result.and_then(|r| r.get("Success")) == Some(&Value::Bool(false))
    {
        return Err(ProviderError::Other(format!(
            "Qwen Cloud Team {action} failed."
        )));
    }
    let code_ok = root.get("code").and_then(Value::as_str) == Some("200");
    match result {
        Some(result) if success_response == Some(&Value::Bool(true)) || code_ok => {
            Ok(result.clone())
        }
        _ => Err(fail("gateway envelope")),
    }
}

/// The console's billing selector from `LoadHumanInfo`. It is not the login
/// UID or a saved workspace id, and it is sent untrimmed.
pub(super) fn billing_selector(
    human: &Map<String, Value>,
) -> Result<Option<String>, ProviderError> {
    let nbid = record(human.get("Data"))
        .and_then(|data| record(data.get("SellerInfoDto")))
        .and_then(|seller| seller.get("Nbid"));
    match nbid {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.is_empty() => Ok(None),
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.clone())),
        Some(_) => Err(fail("billing selector")),
    }
}

/// `JSON.stringify` of upstream's params, keeping its key order (serde_json
/// maps here sort keys).
pub(super) fn summary_params(nbid: Option<&str>) -> String {
    let code = Value::from(PRODUCT_CODE);
    match nbid {
        None => format!(r#"{{"productCode":{code}}}"#),
        Some(nbid) => format!(r#"{{"productCode":{code},"Nbid":{}}}"#, Value::from(nbid)),
    }
}

/// Parse `GetSeatSubscriptionSummary` in upstream's exact check order.
pub(super) fn parse_summary(
    summary: &Map<String, Value>,
    now_ms: i64,
) -> Result<Option<TeamUsage>, ProviderError> {
    if summary.get("Data") == Some(&Value::Null) {
        return Ok(None);
    }
    let data = record(summary.get("Data")).ok_or_else(|| fail("subscription"))?;
    let groups = data
        .get("SubscriptionGroupList")
        .and_then(Value::as_array)
        .ok_or_else(|| fail("subscription groups"))?;
    if groups.is_empty() {
        return Ok(None);
    }
    let start = number(data.get("StartTime"))?;
    let end = number(data.get("EndTime"))?;
    if !is_safe_integer(start) || !is_safe_integer(end) || start <= 0.0 || end <= start {
        return Err(fail("subscription period"));
    }
    // Millisecond timestamps are far below 2^53, so the comparison is exact.
    #[expect(
        clippy::cast_precision_loss,
        reason = "current epoch milliseconds fit an f64 exactly"
    )]
    let now = now_ms as f64;
    if now < start || now >= end {
        return Ok(None);
    }
    if data.get("ProductCode").and_then(Value::as_str) != Some(PRODUCT_CODE) {
        return Err(fail("subscription product"));
    }
    if groups.len() != 1 {
        return Err(fail("multiple credit pools"));
    }
    let group = groups[0]
        .as_object()
        .ok_or_else(|| fail("subscription group"))?;
    let equity_list = group
        .get("EquityList")
        .and_then(Value::as_array)
        .ok_or_else(|| fail("credit equity"))?;
    let credits: Vec<&Map<String, Value>> = equity_list
        .iter()
        .filter_map(Value::as_object)
        .filter(|equity| equity.get("EquityCode").and_then(Value::as_str) == Some("credit_value"))
        .collect();
    let [equity] = credits.as_slice() else {
        return Err(fail("credit equity"));
    };
    let total = number(equity.get("TotalValue"))?;
    let remaining = number(equity.get("SurplusValue"))?;
    if total <= 0.0 || remaining > total {
        return Err(fail("credit count"));
    }

    let seats = match (
        group
            .get("SubscriptionAssignedNumber")
            .and_then(Value::as_f64),
        group.get("SubscriptionTotalNumber").and_then(Value::as_f64),
    ) {
        (Some(assigned), Some(seats))
            if is_safe_integer(assigned)
                && is_safe_integer(seats)
                && assigned >= 0.0
                && seats >= assigned =>
        {
            Some((assigned, seats))
        }
        _ => None,
    };

    let resets_at = match group.get("NextCycleFlushTime") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let millis = number(Some(value))?;
            if !is_safe_integer(millis) || millis <= 0.0 {
                return Err(fail("cycle reset"));
            }
            let millis: i64 = safe_integer_text(millis)
                .parse()
                .map_err(|_| fail("cycle reset"))?;
            Some(DateTime::from_timestamp_millis(millis).ok_or_else(|| fail("cycle reset"))?)
        }
    };

    Ok(Some(TeamUsage {
        used: total - remaining,
        total,
        remaining,
        resets_at,
        seats,
        login_method: login_method(group.get("SpecType")),
    }))
}

fn login_method(spec: Option<&Value>) -> String {
    let valid = |s: &str| {
        let bytes = s.as_bytes();
        (1..=40).contains(&bytes.len())
            && bytes[0].is_ascii_lowercase()
            && bytes[1..]
                .iter()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_' || *b == b'-')
    };
    match spec.and_then(Value::as_str) {
        // The check above guarantees ASCII, so slicing at byte 1 is safe.
        Some(s) if valid(s) => format!("{}{} Team", s[..1].to_ascii_uppercase(), &s[1..]),
        _ => SECTION_TITLE.into(),
    }
}

pub(super) fn to_result(team: &TeamUsage) -> ProviderFetchResult {
    let percent = (team.used / team.total * 100.0).clamp(0.0, 100.0);
    let primary = RateWindow::with_details(percent, None, team.resets_at, Some("Team".into()));
    let usage = UsageSnapshot::new(primary)
        .with_primary_label("Team")
        .with_login_method(team.login_method.clone());

    let format = super::format_quota;
    let mut rows = vec![
        ProviderDisplayDetail::new(
            "team-credits",
            "Credits",
            format!(
                "{} / {} credits used",
                format(team.used),
                format(team.total)
            ),
        ),
        ProviderDisplayDetail::new(
            "team-remaining",
            "Remaining",
            format!("{} credits", format(team.remaining)),
        ),
    ];
    if let Some((assigned, seats)) = team.seats {
        rows.push(ProviderDisplayDetail::new(
            "team-seats",
            "Seats",
            format!(
                "{} / {}",
                safe_integer_text(assigned),
                safe_integer_text(seats)
            ),
        ));
    }
    ProviderFetchResult::new(usage, "web").with_display_details(
        rows.into_iter()
            .flatten()
            .filter_map(|row| row.with_section_title(SECTION_TITLE)),
    )
}

/// Run the Team lane against `base` (production: [`BASE_URL`]).
pub(super) async fn fetch(
    client: &reqwest::Client,
    base: &str,
    cookie_header: &str,
    timeout: Duration,
    now_ms: i64,
) -> Result<Option<TeamUsage>, ProviderError> {
    let referer = format!("{base}/analytics/token-plan/team");
    let response = client
        .get(format!("{base}/tool/user/info.json"))
        .timeout(timeout)
        .header("Cookie", cookie_header)
        .header("Accept", "application/json")
        .header("Referer", &referer)
        .send()
        .await?;
    let status = response.status().as_u16();
    let info = parse_response(status, &response.bytes().await?)?;
    let token = sec_token(&info)?;

    let rpc = async |product: &str, action: &str, params: String, region: &str| {
        let mut form = vec![
            ("product", product.to_string()),
            ("action", action.to_string()),
            ("sec_token", token.clone()),
            ("region", region.to_string()),
            ("params", params),
        ];
        if action == "GetSeatSubscriptionSummary" {
            form.push(("language", "zh-CN".into()));
        }
        let response = client
            .post(format!(
                "{base}/data/api.json?product={product}&action={action}"
            ))
            .timeout(timeout)
            .header("Cookie", cookie_header)
            .header("Accept", "application/json")
            .header("Referer", &referer)
            .header("Origin", base)
            .form(&form)
            .send()
            .await?;
        let status = response.status().as_u16();
        let root = parse_response(status, &response.bytes().await?)?;
        rpc_result(action, &root)
    };

    let human = rpc("ea-service", "LoadHumanInfo", "{}".into(), "ap-southeast-1").await?;
    let params = summary_params(billing_selector(&human)?.as_deref());
    let summary = rpc(
        "BssOpenAPI-V3",
        "GetSeatSubscriptionSummary",
        params,
        "cn-hangzhou",
    )
    .await?;
    parse_summary(&summary, now_ms)
}

/// Upstream's fallback error: when Team failed for a real reason and the
/// Individual lane also fails, the Team error is what the user sees.
pub(super) fn choose_result<T>(
    team_error: Option<ProviderError>,
    individual: Result<T, ProviderError>,
) -> Result<T, ProviderError> {
    match (team_error, individual) {
        (Some(team_error), Err(_)) => Err(team_error),
        (Some(team_error), Ok(value)) => {
            tracing::warn!("Qwen Cloud Team usage unavailable: {team_error}");
            Ok(value)
        }
        (None, individual) => individual,
    }
}
