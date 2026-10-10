//! Best-effort pay-as-you-go credit lookup (upstream v0.67.0 `sakana.js`).
//!
//! The billing page only server-renders the "Pay as you go" tab when the
//! request URL carries `?tab=payAsYouGo`, so the balance needs a second GET
//! next to the required quota request. Nothing here may fail or delay the
//! primary result: every failure collapses to `None` and the caller simply
//! omits the extra rows.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex_lite::Regex;
use reqwest::{Client, Url};
use tokio::task::JoinHandle;

use crate::core::{ProviderDisplayDetail, is_same_origin};
use crate::providers::{format, read_bounded_response};

/// Per-request bound for the optional GET (the primary keeps the client's 15 s).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// Shared collection budget, measured from the primary request start.
const COLLECTION_BUDGET: Duration = Duration::from_millis(200);
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_PERIOD_CHARS: usize = 120;

static BALANCE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)<h2[^>]*>\s*Credit balance\s*</h2>[\s\S]{0,900}?<p[^>]*tabular-nums[^"]*"[^>]*>\$?([0-9][0-9,]*(?:\.[0-9]+)?)</p>"#,
    )
    .expect("balance regex is valid")
});
static USAGE_TOTAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?i)<h2[^>]*>\s*Usage\s*</h2>\s*<span[^>]*>\s*Total(?:<!--\s*-->)?:\s*(?:<!--\s*-->)?\$?([0-9][0-9,]*(?:\.[0-9]+)?)\s*</span>"#,
    )
    .expect("usage total regex is valid")
});
static PERIOD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)aria-label="Usage date range"[^>]*>([\s\S]*?)</button>"#)
        .expect("period regex is valid")
});
static HTML_COMMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<!--[\s\S]*?-->").expect("comment regex is valid"));

#[derive(Debug, Clone, PartialEq)]
pub(super) struct PaygBalance {
    balance: f64,
    usage_total: Option<f64>,
    period: Option<String>,
}

impl PaygBalance {
    /// `Balance` plus, when the page shows a total, `Usage` with the date
    /// range as its secondary value. Menu detail only; never a tray metric.
    pub(super) fn display_details(&self) -> Vec<ProviderDisplayDetail> {
        let mut rows = Vec::new();
        rows.extend(ProviderDisplayDetail::new(
            "payg-balance",
            "Balance",
            format::usd_plain(self.balance),
        ));
        if let Some(total) = self.usage_total {
            let row = ProviderDisplayDetail::new("payg-usage", "Usage", format::usd_plain(total));
            rows.extend(match self.period.as_deref() {
                Some(period) => row.and_then(|row| row.with_secondary_value(period)),
                None => row,
            });
        }
        rows
    }
}

/// Parse the pay-as-you-go tab. `None` when the balance card is absent; an
/// account without purchased credit still renders a `$0.00` balance, so
/// absence means the markup was not there.
pub(super) fn parse(html: &str) -> Option<PaygBalance> {
    let balance = capture_amount(&BALANCE_RE, html)?;
    Some(PaygBalance {
        balance,
        usage_total: capture_amount(&USAGE_TOTAL_RE, html),
        period: capture_period(html),
    })
}

fn capture_amount(pattern: &Regex, html: &str) -> Option<f64> {
    pattern
        .captures(html)?
        .get(1)?
        .as_str()
        .replace(',', "")
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

fn capture_period(html: &str) -> Option<String> {
    let raw = PERIOD_RE.captures(html)?.get(1)?.as_str();
    let text = HTML_COMMENT_RE.replace_all(raw, "");
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!collapsed.is_empty()).then(|| collapsed.chars().take(MAX_PERIOD_CHARS).collect())
}

/// One optional GET: never errors, never retries. The final URL must stay on
/// the billing origin so a redirect cannot feed foreign markup into the parser.
async fn fetch(
    client: Client,
    url: Url,
    cookie: String,
    billing_origin: Url,
) -> Option<PaygBalance> {
    let response = super::billing_request(&client, url, &cookie)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .ok()?;
    if response.status() != reqwest::StatusCode::OK
        || !is_same_origin(response.url(), &billing_origin)
    {
        return None;
    }
    let body = read_bounded_response(response, MAX_BODY_BYTES).await.ok()?;
    parse(&String::from_utf8_lossy(&body))
}

/// Handle to the in-flight optional GET. Dropping it (primary failure, caller
/// cancellation, budget expiry) aborts the request instead of leaking it.
pub(super) struct PaygLookup {
    task: JoinHandle<Option<PaygBalance>>,
    started_at: Instant,
}

impl PaygLookup {
    pub(super) fn spawn(client: Client, url: Url, cookie: String, billing_origin: Url) -> Self {
        let started_at = Instant::now();
        Self {
            task: tokio::spawn(fetch(client, url, cookie, billing_origin)),
            started_at,
        }
    }

    /// Collect the result within the policy budget measured from `spawn`:
    /// background reads keep the shared 200 ms (a slow primary only takes an
    /// already-finished result); foreground reads that require completeness
    /// wait out the full optional-request timeout.
    pub(super) async fn join(
        mut self,
        requires_optional_usage_completeness: bool,
    ) -> Option<PaygBalance> {
        let budget = join_budget(self.started_at, requires_optional_usage_completeness);
        match tokio::time::timeout(budget, &mut self.task).await {
            Ok(Ok(balance)) => balance,
            Ok(Err(_)) | Err(_) => None,
        }
    }
}

impl Drop for PaygLookup {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn join_budget(started_at: Instant, requires_optional_usage_completeness: bool) -> Duration {
    let total = if requires_optional_usage_completeness {
        REQUEST_TIMEOUT
    } else {
        COLLECTION_BUDGET
    };
    total.saturating_sub(started_at.elapsed())
}
