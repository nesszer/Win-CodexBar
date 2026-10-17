//! Live remaining credit estimate for the xAI prepaid balance.
//!
//! The posted prepaid balance runs ahead of the console's "Credits remaining"
//! until xAI posts the current cycle's spend. The postpaid invoice preview
//! reports that not-yet-posted charge as `coreInvoice.amountAfterVat`, a
//! signed USD cents value encoded as a string. Subtracting it from the posted
//! balance estimates the live remaining credit.
//!
//! Ported from steipete/CodexBar 24d5d7059 (#4419). The posted balance stays
//! the primary value; a missing, failed, or malformed preview yields no
//! estimate rather than a zero.

use crate::core::ProviderDisplayDetail;

pub(super) const LIVE_REMAINING_ID: &str = "xai-live-remaining";
pub(super) const LIVE_REMAINING_TITLE: &str = "Live remaining (est.)";

/// Largest magnitude a JavaScript number holds exactly (`Number.isSafeInteger`).
const MAX_SAFE_CENTS: u64 = 9_007_199_254_740_991;

/// Derive the live remaining estimate from one invoice-preview response.
///
/// Returns `None` unless the response is 2xx, the body is JSON, and
/// `coreInvoice.amountAfterVat` is a string holding a safe signed integer of
/// cents. An explicit zero or a negative remaining result is kept.
pub(super) fn live_remaining_usd_from_preview(
    balance_usd: f64,
    status: u16,
    body: &str,
) -> Option<f64> {
    if !(200..300).contains(&status) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let amount = value.get("coreInvoice")?.get("amountAfterVat")?.as_str()?;
    let cents = parse_safe_cents(amount)?;
    Some(balance_usd - (cents as f64) / 100.0)
}

/// Accept only an optional leading `-` followed by ASCII digits, and only
/// values whose magnitude is a safe integer in JavaScript terms.
fn parse_safe_cents(raw: &str) -> Option<i64> {
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let cents: i64 = raw.parse().ok()?;
    (cents.unsigned_abs() <= MAX_SAFE_CENTS).then_some(cents)
}

/// Build the "Live remaining (est.)" display row, or `None` without an estimate.
pub(super) fn live_remaining_detail(
    live_remaining_usd: Option<f64>,
) -> Option<ProviderDisplayDetail> {
    let value = live_remaining_usd.filter(|value| value.is_finite())?;
    ProviderDisplayDetail::new(
        LIVE_REMAINING_ID,
        LIVE_REMAINING_TITLE,
        format!("${value:.2}"),
    )
}
