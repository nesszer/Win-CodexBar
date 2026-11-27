//! Bridge shape of a longer exhausted quota that blocks a shorter window
//! (upstream 0.69.0 #4091, Kimi's monthly membership pool).
//!
//! Upstream evaluates `menuCard.blockingQuota` whenever it builds the menu
//! card, so a blocked row recovers as soon as the pool resets. Bridge
//! snapshots outlive their fetch (the provider cache keeps the last good
//! snapshot through failed refreshes), so each blocked window carries the
//! pool's reset and the frontend re-checks it at render time.

use super::*;

/// Set on a window while a longer exhausted pool blocks it. Presentation only:
/// the window's raw percentages stay the provider's data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "../../src/types/generated/")]
#[serde(rename_all = "camelCase")]
pub struct MonthlyLimitBlockSnapshot {
    /// RFC 3339 reset of the exhausted pool; the block lifts then. `None` when
    /// the pool's reset is unknown, so the block holds until a refresh.
    #[serde(default)]
    pub resets_at: Option<String>,
}

impl MonthlyLimitBlockSnapshot {
    pub(super) fn for_window(blocked: bool, blocks: &BlockedWindows) -> Option<Self> {
        blocked.then(|| Self {
            resets_at: blocks.resets_at.map(|reset| reset.to_rfc3339()),
        })
    }

    /// Block of the window the provider-level pace derives from: the weekly
    /// lane when the primary is an informational stand-in, else the primary.
    pub(super) fn for_pace(
        usage: &codexbar::core::UsageSnapshot,
        blocks: &BlockedWindows,
    ) -> Option<Self> {
        let blocked = if usage.primary.is_informational {
            blocks.secondary
        } else {
            blocks.primary
        };
        Self::for_window(blocked, blocks)
    }
}

impl RateWindowSnapshot {
    pub(super) fn with_quota_block(mut self, blocked: bool, blocks: &BlockedWindows) -> Self {
        self.monthly_limit_block = MonthlyLimitBlockSnapshot::for_window(blocked, blocks);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use codexbar::core::UsageSnapshot;

    fn monthly_pool(reset: Option<chrono::DateTime<Utc>>) -> RateWindow {
        RateWindow::with_details(100.0, Some(43_200), reset, None)
    }

    fn bridge_snapshot(id: ProviderId, usage: UsageSnapshot) -> ProviderUsageSnapshot {
        let result = ProviderFetchResult::new(usage, "web");
        let metadata = instantiate_provider(id).metadata().clone();
        ProviderUsageSnapshot::from_fetch_result(id, &metadata, &result, None, None)
    }

    #[test]
    fn exhausted_kimi_monthly_pool_marks_shorter_windows_without_changing_raw_usage() {
        let reset = Utc::now() + Duration::days(20);
        let primary = RateWindow::with_details(
            0.0,
            Some(10_080),
            Some(Utc::now() + Duration::days(3)),
            None,
        );
        let usage = UsageSnapshot::new(primary).with_extra_rate_window(
            "kimi-monthly",
            "Total usage",
            monthly_pool(Some(reset)),
        );

        let snapshot = bridge_snapshot(ProviderId::Kimi, usage.clone());
        let block = MonthlyLimitBlockSnapshot {
            resets_at: Some(reset.to_rfc3339()),
        };
        assert_eq!(snapshot.primary.monthly_limit_block, Some(block.clone()));
        assert_eq!(snapshot.primary.used_percent, 0.0);
        assert_eq!(
            snapshot.extra_rate_windows[0].window.monthly_limit_block,
            None
        );
        // Upstream drops the blocked metric's pace forecast; the provider-level
        // pace comes from the primary here, so it carries the same block.
        let pace = snapshot.pace.as_ref().expect("primary pace");
        assert_eq!(pace.monthly_limit_block, Some(block));

        let json = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(
            json["primary"]["monthlyLimitBlock"]["resetsAt"],
            reset.to_rfc3339()
        );
        assert!(json["extraRateWindows"][0]["window"]["monthlyLimitBlock"].is_null());
        assert!(json["pace"]["monthlyLimitBlock"].is_object());

        // Other providers never derive a blocker from a same-named window.
        let snapshot = bridge_snapshot(ProviderId::Codex, usage);
        assert_eq!(snapshot.primary.monthly_limit_block, None);
        assert_eq!(snapshot.pace.as_ref().unwrap().monthly_limit_block, None);
    }

    #[test]
    fn unknown_monthly_reset_blocks_without_a_lift_time() {
        let usage = UsageSnapshot::new(RateWindow::with_details(0.0, Some(10_080), None, None))
            .with_extra_rate_window("kimi-monthly", "Total usage", monthly_pool(None));

        let snapshot = bridge_snapshot(ProviderId::Kimi, usage);
        assert_eq!(
            snapshot.primary.monthly_limit_block,
            Some(MonthlyLimitBlockSnapshot { resets_at: None })
        );
        let json = serde_json::to_value(&snapshot).unwrap();
        assert!(json["primary"]["monthlyLimitBlock"].is_object());
        assert!(json["primary"]["monthlyLimitBlock"]["resetsAt"].is_null());
    }

    #[test]
    fn informational_primary_takes_the_pace_block_from_the_weekly_lane() {
        let reset = Utc::now() + Duration::days(20);
        let mut usage = UsageSnapshot::new(RateWindow::informational("No active session"))
            .with_extra_rate_window("kimi-monthly", "Total usage", monthly_pool(Some(reset)));
        usage.secondary = Some(RateWindow::with_details(
            10.0,
            Some(10_080),
            Some(Utc::now() + Duration::days(3)),
            None,
        ));

        let snapshot = bridge_snapshot(ProviderId::Kimi, usage);
        assert_eq!(snapshot.primary.monthly_limit_block, None);
        assert!(snapshot.secondary.unwrap().monthly_limit_block.is_some());
        assert!(snapshot.pace.unwrap().monthly_limit_block.is_some());
    }

    #[test]
    fn snapshots_without_the_field_deserialize_unblocked() {
        let window: RateWindowSnapshot =
            serde_json::from_value(serde_json::json!({ "usedPercent": 40.0 })).unwrap();
        assert_eq!(window.monthly_limit_block, None);
        let pace: PaceSnapshot =
            serde_json::from_value(serde_json::json!({ "stage": "on_track", "deltaPercent": 0.0 }))
                .unwrap();
        assert_eq!(pace.monthly_limit_block, None);
    }
}
