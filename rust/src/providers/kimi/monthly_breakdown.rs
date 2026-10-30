//! Kimi Code / other-features breakdown of the monthly `Total usage` pool
//! (upstream #4759). Both shares draw on one limit, so they are detail rows
//! rather than quota windows. The Code share always comes from the source
//! that supplied `Total usage`: `limit_month_code` beside the Code API
//! `limit_month_total`, or `kimiCodeUsedRatio` beside the web membership pool.

use serde::Deserialize;

use super::{KimiCodeUsagePools, KimiRatioPool, KimiSubscriptionStatsResponse, subscription_pool};
use crate::core::ProviderDisplayDetail;
use crate::providers::json;

const SECTION_TITLE: &str = "Total usage";

/// Used ratio of the shared monthly pool and the Kimi Code share reported by
/// the same source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct MonthlyShares {
    used: f64,
    code: Option<f64>,
}

impl KimiCodeUsagePools {
    /// Shares from the Code API, present only when its monthly pool is the
    /// `Total usage` lane. Malformed share data yields no Code share.
    pub(super) fn monthly_shares(&self) -> Option<MonthlyShares> {
        let used = self.monthly.as_ref()?.valid_ratio()?;
        let code = self
            .monthly_code
            .as_ref()
            // A derived struct also decodes from a JSON array; only an object is a pool.
            .filter(|value| value.is_object())
            .and_then(|value| KimiRatioPool::deserialize(value).ok())
            .and_then(|pool| pool.valid_ratio());
        Some(MonthlyShares { used, code })
    }
}

/// Shares from the web membership pool that backs `Total usage`.
pub(super) fn subscription_monthly_shares(
    subscription: &KimiSubscriptionStatsResponse,
) -> Option<MonthlyShares> {
    let (balance, used) = subscription_pool(subscription)?;
    Some(MonthlyShares {
        used,
        code: json::lenient_f64(balance.kimi_code_used_ratio.as_ref()),
    })
}

/// One "Total usage" section with a "Kimi Code" and an "Other features" row,
/// or nothing when either ratio is missing or invalid. Values stay canonical
/// English, like the CLI.
pub(super) fn monthly_breakdown_details(
    shares: Option<MonthlyShares>,
) -> Vec<ProviderDisplayDetail> {
    let Some(MonthlyShares {
        used,
        code: Some(code),
    }) = shares
    else {
        return Vec::new();
    };
    if !(used.is_finite() && used >= 0.0 && code.is_finite() && code >= 0.0) {
        return Vec::new();
    }
    [
        ("kimi-monthly-code", "Kimi Code", code),
        (
            "kimi-monthly-other",
            "Other features",
            (used - code).max(0.0),
        ),
    ]
    .into_iter()
    .filter_map(|(id, label, ratio)| {
        ProviderDisplayDetail::new(id, label, format!("{} used", percent_text(ratio * 100.0)))?
            .with_section_title(SECTION_TITLE)?
            .with_progress(ratio, 1.0)
    })
    .collect()
}

/// Upstream `UsageFormatter.percentString`: clamped to 0-100, with "<1%" for
/// any share above 0 and below 1, including one that would round to 1%.
fn percent_text(percent: f64) -> String {
    let clamped = percent.clamp(0.0, 100.0);
    if clamped > 0.0 && clamped < 1.0 {
        "<1%".to_string()
    } else {
        format!("{clamped:.0}%")
    }
}

#[cfg(test)]
#[path = "monthly_breakdown_tests.rs"]
mod tests;
