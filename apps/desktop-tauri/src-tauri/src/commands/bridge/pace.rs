use super::*;
use chrono::{DateTime, Utc};

pub(crate) const SESSION_FALLBACK_MINUTES: u32 = 300;
pub(crate) const WEEKLY_FALLBACK_MINUTES: u32 = 10_080;

pub(crate) fn stage_str(stage: codexbar::core::PaceStage) -> &'static str {
    use codexbar::core::PaceStage;
    match stage {
        PaceStage::OnTrack => "on_track",
        PaceStage::SlightlyAhead => "slightly_ahead",
        PaceStage::Ahead => "ahead",
        PaceStage::FarAhead => "far_ahead",
        PaceStage::SlightlyBehind => "slightly_behind",
        PaceStage::Behind => "behind",
        PaceStage::FarBehind => "far_behind",
    }
}

/// Pace of one rate window against the time elapsed in it.
/// `delta_percent` is actual minus expected used percent, so a positive delta
/// is a deficit and a negative one a reserve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPaceSnapshot {
    pub stage: String,
    pub delta_percent: f64,
    pub expected_used_percent: f64,
    pub actual_used_percent: f64,
    #[serde(default)]
    pub eta_seconds: Option<f64>,
    #[serde(default)]
    pub will_last_to_reset: bool,
    /// Scored against the provider's reset window (upstream
    /// `ProviderPaceCapability.resetWindowPace`): cards show it as weekly
    /// pace on any card lane, whatever the window's length.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reset_window: bool,
}

/// Remaining allowance divided by the local calendar days left, in the
/// allowance's own unit (`%`, `credits` or `requests`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DailyTargetSnapshot {
    pub amount: f64,
    pub unit: String,
}

impl From<codexbar::core::MonthlyDailyTarget> for DailyTargetSnapshot {
    fn from(target: codexbar::core::MonthlyDailyTarget) -> Self {
        Self {
            amount: target.amount,
            unit: target.unit,
        }
    }
}

/// Where a window is rendered. Reset-window pace applies to the card's
/// primary, secondary and tertiary bars only, as on the Mac; extra windows
/// keep plain lane pace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaceLane {
    Card,
    Extra,
}

/// Pace of one window and whether it came from the provider's reset-window
/// capability. The monthly sentinel and Copilot's missing length resolve to
/// the calendar month ending at the reset before scoring.
pub(crate) fn lane_usage_pace(
    id: ProviderId,
    lane: PaceLane,
    window: &RateWindow,
    default_window_minutes: u32,
    now: DateTime<Utc>,
    work_days: Option<u8>,
) -> Option<(codexbar::core::UsagePace, bool)> {
    let capability = id.pace_capability();
    if lane == PaceLane::Card && capability.supports_reset_window_pace(window, now) {
        return capability
            .reset_window_pace(window, now, work_days)
            .map(|pace| (pace, true));
    }
    codexbar::core::UsagePace::weekly(window, Some(now), default_window_minutes, work_days)
        .map(|pace| (pace, false))
}

impl WindowPaceSnapshot {
    pub(crate) fn for_window(
        window: &RateWindow,
        default_window_minutes: u32,
        now: DateTime<Utc>,
        work_days: Option<u8>,
    ) -> Option<Self> {
        if window.is_informational {
            return None;
        }
        let pace = codexbar::core::UsagePace::weekly(
            window,
            Some(now),
            default_window_minutes,
            work_days,
        )?;
        Some(Self::from_pace(&pace, false))
    }

    /// Lane pace with the provider's reset-window capability applied.
    pub(crate) fn for_lane(
        id: ProviderId,
        lane: PaceLane,
        window: &RateWindow,
        default_window_minutes: u32,
        now: DateTime<Utc>,
        work_days: Option<u8>,
    ) -> Option<Self> {
        if window.is_informational {
            return None;
        }
        let (pace, reset_window) =
            lane_usage_pace(id, lane, window, default_window_minutes, now, work_days)?;
        Some(Self::from_pace(&pace, reset_window))
    }

    fn from_pace(pace: &codexbar::core::UsagePace, reset_window: bool) -> Self {
        Self {
            stage: stage_str(pace.stage).to_string(),
            delta_percent: pace.delta_percent,
            expected_used_percent: pace.expected_used_percent,
            actual_used_percent: pace.actual_used_percent,
            eta_seconds: pace.eta_seconds,
            will_last_to_reset: pace.will_last_to_reset,
            reset_window,
        }
    }
}

/// Daily target of a monthly window in local calendar days (the Mac uses
/// `Calendar.current`). Upstream `monthlyTargetMetrics` reads the same
/// window, so the target never estimates today's consumption. It does not
/// depend on a lane pace: an exhausted allowance has no pace but still has
/// a target.
pub(crate) fn daily_target(
    id: Option<ProviderId>,
    window: &RateWindow,
    now: DateTime<Utc>,
) -> Option<DailyTargetSnapshot> {
    let capability = id.map_or(codexbar::core::ProviderPaceCapability::UNSUPPORTED, |id| {
        id.pace_capability()
    });
    capability
        .monthly_daily_target(window, now, &chrono::Local)
        .map(DailyTargetSnapshot::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use codexbar::core::UsageSnapshot;

    fn fixed_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 10, 12, 0, 0).unwrap()
    }

    fn resetting_in(
        now: DateTime<Utc>,
        used: f64,
        window: Option<u32>,
        minutes: i64,
    ) -> RateWindow {
        RateWindow::with_details(used, window, Some(now + Duration::minutes(minutes)), None)
    }

    fn snapshot(usage: UsageSnapshot, pace_authoritative: bool) -> ProviderUsageSnapshot {
        let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
        let mut result = ProviderFetchResult::new(usage, "web");
        if !pace_authoritative {
            result = result.with_non_authoritative_pace();
        }
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None, None)
    }

    fn expected_used(window: &RateWindowSnapshot) -> f64 {
        window
            .pace
            .as_ref()
            .expect("lane pace")
            .expected_used_percent
    }

    #[test]
    fn reserve_is_a_negative_delta_that_lasts_to_reset() {
        let window = resetting_in(fixed_now(), 30.0, Some(10_080), 5_040);

        assert_eq!(
            WindowPaceSnapshot::for_window(&window, WEEKLY_FALLBACK_MINUTES, fixed_now(), None),
            Some(WindowPaceSnapshot {
                stage: "far_behind".to_string(),
                delta_percent: -20.0,
                expected_used_percent: 50.0,
                actual_used_percent: 30.0,
                eta_seconds: None,
                will_last_to_reset: true,
                reset_window: false,
            })
        );
    }

    #[test]
    fn deficit_is_a_positive_delta_with_an_eta() {
        let window = resetting_in(fixed_now(), 55.0, Some(10_080), 5_040);

        let pace =
            WindowPaceSnapshot::for_window(&window, WEEKLY_FALLBACK_MINUTES, fixed_now(), None)
                .expect("pace");

        assert_eq!(pace.stage, "slightly_ahead");
        assert_eq!(pace.delta_percent, 5.0);
        assert!(!pace.will_last_to_reset);
        assert!((pace.eta_seconds.expect("eta") - 247_418.18).abs() < 0.01);
    }

    /// Work days reach the weekly lane pace. Friday noon UTC two days before a
    /// Sunday-noon reset: plain elapsed-time pace expects 5/7 = 71.4%; a
    /// Monday-Friday schedule expects about 90% (4.5 of 5 work days in UTC).
    /// The margin holds for every local zone the test machine might use.
    #[test]
    fn weekly_lane_pace_follows_configured_work_days() {
        let now = Utc.with_ymd_and_hms(2026, 6, 12, 12, 0, 0).unwrap();
        let window = resetting_in(now, 50.0, Some(10_080), 2 * 24 * 60);

        let linear = WindowPaceSnapshot::for_window(&window, WEEKLY_FALLBACK_MINUTES, now, None)
            .expect("linear pace");
        let seven = WindowPaceSnapshot::for_window(&window, WEEKLY_FALLBACK_MINUTES, now, Some(7))
            .expect("seven-day pace");
        let five = WindowPaceSnapshot::for_window(&window, WEEKLY_FALLBACK_MINUTES, now, Some(5))
            .expect("five-day pace");

        assert!((linear.expected_used_percent - 500.0 / 7.0).abs() < 0.01);
        assert_eq!(seven, linear);
        assert!(five.expected_used_percent > linear.expected_used_percent + 5.0);
    }

    #[test]
    fn informational_window_has_no_pace() {
        let mut window = resetting_in(fixed_now(), 30.0, Some(10_080), 5_040);
        window.is_informational = true;

        assert_eq!(
            WindowPaceSnapshot::for_window(&window, WEEKLY_FALLBACK_MINUTES, fixed_now(), None),
            None
        );
    }

    #[test]
    fn session_lane_defaults_to_a_300_minute_window() {
        let primary = resetting_in(Utc::now(), 60.0, None, 150);

        let snap = snapshot(UsageSnapshot::new(primary), true);

        assert!((expected_used(&snap.primary) - 50.0).abs() < 0.1);
        assert_eq!(snap.primary.pace.as_ref().unwrap().stage, "ahead");
    }

    #[test]
    fn weekly_lanes_default_to_a_10080_minute_window() {
        let now = Utc::now();
        let mut usage = UsageSnapshot::new(RateWindow::new(10.0))
            .with_secondary(resetting_in(now, 40.0, None, 5_040))
            .with_tertiary(resetting_in(now, 45.0, None, 5_040))
            .with_extra_rate_window("sonnet", "Sonnet", resetting_in(now, 50.0, None, 5_040));
        usage.model_specific = Some(resetting_in(now, 55.0, None, 5_040));

        let snap = snapshot(usage, true);

        assert!((expected_used(snap.secondary.as_ref().unwrap()) - 50.0).abs() < 0.1);
        assert!((expected_used(snap.tertiary.as_ref().unwrap()) - 50.0).abs() < 0.1);
        assert!((expected_used(snap.model_specific.as_ref().unwrap()) - 50.0).abs() < 0.1);
        assert!((expected_used(&snap.extra_rate_windows[0].window) - 50.0).abs() < 0.1);
    }

    #[test]
    fn informational_primary_keeps_the_weekly_lane_pace() {
        let usage = UsageSnapshot::new(RateWindow::informational("No session limit"))
            .with_secondary(resetting_in(Utc::now(), 30.0, Some(10_080), 5_040));

        let snap = snapshot(usage, true);

        assert_eq!(snap.primary.pace, None);
        let weekly = snap.secondary.as_ref().unwrap().pace.as_ref().unwrap();
        assert_eq!(weekly.stage, "far_behind");
        assert!(weekly.will_last_to_reset);
    }

    #[test]
    fn non_authoritative_pace_leaves_every_lane_empty() {
        let now = Utc::now();
        let usage = UsageSnapshot::new(resetting_in(now, 60.0, Some(300), 150))
            .with_secondary(resetting_in(now, 30.0, Some(10_080), 5_040));

        let snap = snapshot(usage, false);

        assert_eq!(snap.primary.pace, None);
        assert_eq!(snap.secondary.as_ref().unwrap().pace, None);
        // The tray scores its own pace, so it needs the flag too.
        assert!(!snap.pace_authoritative);
        assert!(
            snapshot(
                UsageSnapshot::new(resetting_in(now, 60.0, Some(300), 150)),
                true
            )
            .pace_authoritative
        );
    }

    #[test]
    fn window_pace_serializes_in_camel_case() {
        let window = resetting_in(fixed_now(), 30.0, Some(10_080), 5_040);
        let pace =
            WindowPaceSnapshot::for_window(&window, WEEKLY_FALLBACK_MINUTES, fixed_now(), None);

        assert_eq!(
            serde_json::to_string(&pace).unwrap(),
            r#"{"stage":"far_behind","deltaPercent":-20.0,"expectedUsedPercent":50.0,"actualUsedPercent":30.0,"etaSeconds":null,"willLastToReset":true}"#
        );
    }

    /// The refresh path (`from_fetch_result`) threads work days into the lane
    /// pace. The weekly window ends at the next local Monday 00:00, so it is
    /// the current Mon..Sun week and Mon-Fri pace is ahead of linear pace at
    /// any instant inside it.
    #[test]
    fn fetch_result_lane_pace_follows_work_days() {
        use chrono::{Datelike, Local};
        let today = Local::now().date_naive();
        let next_monday =
            today + Duration::days(7 - i64::from(today.weekday().num_days_from_monday()));
        let reset = Local
            .from_local_datetime(&next_monday.and_hms_opt(0, 0, 0).unwrap())
            .single()
            .unwrap()
            .with_timezone(&Utc);
        if reset - Utc::now() >= Duration::days(7) {
            return; // Monday 00:00 exactly or a DST-shortened week: nothing to compare.
        }
        let usage = UsageSnapshot::new(RateWindow::new(10.0)).with_secondary(
            RateWindow::with_details(10.0, Some(10_080), Some(reset), None),
        );
        let metadata = instantiate_provider(ProviderId::Codex).metadata().clone();
        let result = ProviderFetchResult::new(usage, "web");
        let expected = |work_days: Option<u8>| {
            let snapshot = ProviderUsageSnapshot::from_fetch_result(
                ProviderId::Codex,
                &metadata,
                &result,
                None,
                work_days,
            );
            expected_used(snapshot.secondary.as_ref().expect("weekly lane"))
        };

        let linear = expected(None);
        // Each call reads the clock again, so compare within a small tolerance.
        assert!((expected(Some(7)) - linear).abs() < 0.01);
        assert!(expected(Some(5)) > linear);
    }

    // --- Reset-window pace (upstream `ProviderPaceCapability`) ---

    /// Feb 15 00:00 UTC in a billing cycle that resets Mar 1 00:00 UTC:
    /// 14 of the 28 February days have elapsed.
    fn february_now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 2, 15, 0, 0, 0).unwrap()
    }

    fn march_reset() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap()
    }

    fn monthly(used: f64, minutes: Option<u32>) -> RateWindow {
        RateWindow::with_details(used, minutes, Some(march_reset()), None)
    }

    fn snapshot_at(id: ProviderId, usage: UsageSnapshot) -> ProviderUsageSnapshot {
        let metadata = instantiate_provider(id).metadata().clone();
        let result = ProviderFetchResult::new(usage, "web");
        ProviderUsageSnapshot::from_fetch_result_at(
            id,
            &metadata,
            &result,
            None,
            None,
            february_now(),
        )
    }

    fn lane_pace(window: &RateWindowSnapshot) -> &WindowPaceSnapshot {
        window.pace.as_ref().expect("lane pace")
    }

    /// Copilot windows carry a reset date and no length. The Mac resolves
    /// them to the calendar month ending at the reset (28 days here).
    #[test]
    fn copilot_monthly_lanes_get_reset_window_pace() {
        let usage = UsageSnapshot::new(monthly(60.0, None))
            .with_secondary(monthly(20.0, None))
            .with_extra_rate_window("copilot-seat", "Seat credits", monthly(60.0, None));

        let snap = snapshot_at(ProviderId::Copilot, usage);

        let premium = lane_pace(&snap.primary);
        assert_eq!(premium.expected_used_percent, 50.0);
        assert_eq!(premium.delta_percent, 10.0);
        assert_eq!(premium.stage, "ahead");
        // 40% left at 60% per 14 days runs out in 9 days 8 hours.
        assert!((premium.eta_seconds.expect("eta") - 806_400.0).abs() < 1e-6);
        assert!(!premium.will_last_to_reset);
        assert!(premium.reset_window);

        let chat_snap = snap.secondary.as_ref().expect("chat");
        let chat = lane_pace(chat_snap);
        assert_eq!(chat.expected_used_percent, 50.0);
        assert_eq!(chat.delta_percent, -30.0);
        assert!(chat.will_last_to_reset);
        assert!(chat.reset_window);
        // The row's reserve comes from the same resolved pace.
        assert_eq!(chat_snap.reserve_percent, Some(30.0));
        assert!(chat_snap.reserve_will_last_to_reset);

        // Extras keep plain lane pace, as on the Mac: a 14-day reset does not
        // fit the 7-day fallback, so the seat-credit window shows none.
        assert_eq!(snap.extra_rate_windows[0].window.pace, None);
        // The provider-level pace (the Settings detail) scores the length-less
        // window against the 7-day fallback, which a 14-day reset overruns.
        assert!(snap.pace.is_none());
    }

    /// Within the 7-day fallback, a length-less Copilot window still yields
    /// a provider-level pace (the Settings detail shows it). The card lane
    /// keeps its reset-window pace. The tray scores its own pace and skips
    /// the window (`tray_presentation::menu_bar_pace`).
    #[test]
    fn copilot_lengthless_window_keeps_provider_and_lane_pace() {
        let now = february_now();
        let usage = UsageSnapshot::new(RateWindow::with_details(
            60.0,
            None,
            Some(now + Duration::days(3)),
            None,
        ));

        let snap = snapshot_at(ProviderId::Copilot, usage);

        assert!(snap.pace.is_some());
        assert!(lane_pace(&snap.primary).reset_window);
        assert!(snap.pace_authoritative);
        let json = serde_json::to_value(&snap).unwrap();
        assert!(json.get("paceAuthoritative").is_none());
    }

    /// Cursor's plan bars carry the billing-cycle length; the API bar is the
    /// model-specific lane, which the card renders on the tertiary lane.
    #[test]
    fn cursor_plan_lanes_get_reset_window_pace_including_the_api_bar() {
        let mut usage = UsageSnapshot::new(monthly(30.0, Some(40_320)))
            .with_secondary(monthly(70.0, Some(40_320)));
        usage.model_specific = Some(monthly(10.0, Some(40_320)));

        let snap = snapshot_at(ProviderId::Cursor, usage);

        let total = lane_pace(&snap.primary);
        assert_eq!(total.expected_used_percent, 50.0);
        assert_eq!(total.delta_percent, -20.0);
        assert!(total.reset_window);
        let auto = lane_pace(snap.secondary.as_ref().expect("auto"));
        assert_eq!(auto.delta_percent, 20.0);
        assert!(auto.reset_window);
        let api = lane_pace(snap.model_specific.as_ref().expect("api"));
        assert_eq!(api.expected_used_percent, 50.0);
        assert_eq!(api.delta_percent, -40.0);
        assert!(api.reset_window);
        let provider_pace = snap.pace.as_ref().expect("provider pace");
        assert_eq!(provider_pace.expected_used_percent, 50.0);
    }

    /// A 30-day sentinel resolves to February's 28 days: 50% expected, not
    /// the flat-30-day 53.33%. The provider-level pace resolves too.
    #[test]
    fn notion_sentinel_window_is_scored_against_the_real_month() {
        let usage = UsageSnapshot::new(monthly(50.0, Some(43_200)));

        let snap = snapshot_at(ProviderId::Notion, usage);

        let pace = lane_pace(&snap.primary);
        assert_eq!(pace.expected_used_percent, 50.0);
        assert_eq!(pace.delta_percent, 0.0);
        assert_eq!(pace.stage, "on_track");
        assert!(pace.reset_window);
        let provider_pace = snap.pace.as_ref().expect("provider pace");
        assert_eq!(provider_pace.expected_used_percent, 50.0);
    }

    /// Alibaba's monthly quota sits on the tertiary lane, which has no pace
    /// of its own, and the provider module already expands the 30-day
    /// sentinel to the real cycle (31 days ending Mar 31 here). The lane
    /// still gets reset-window pace outside 30-day months.
    #[test]
    fn alibaba_monthly_tertiary_gets_reset_window_pace_in_a_31_day_cycle() {
        let resets_at = Utc.with_ymd_and_hms(2026, 3, 31, 0, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 3, 15, 12, 0, 0).unwrap();
        let monthly = RateWindow::with_details(40.0, Some(44_640), Some(resets_at), None);
        let usage = UsageSnapshot::new(RateWindow::with_details(
            10.0,
            Some(300),
            Some(now + Duration::hours(2)),
            None,
        ))
        .with_tertiary(monthly);
        let metadata = instantiate_provider(ProviderId::Alibaba).metadata().clone();
        let result = ProviderFetchResult::new(usage, "web");

        let snap = ProviderUsageSnapshot::from_fetch_result_at(
            ProviderId::Alibaba,
            &metadata,
            &result,
            None,
            None,
            now,
        );

        let pace = lane_pace(snap.tertiary.as_ref().expect("monthly lane"));
        // 15.5 of 31 days elapsed.
        assert_eq!(pace.expected_used_percent, 50.0);
        assert_eq!(pace.delta_percent, -10.0);
        assert_eq!(pace.stage, "behind");
        assert!(pace.will_last_to_reset);
        assert!(pace.reset_window);
    }

    /// Providers upstream does not opt in keep the existing flat window.
    #[test]
    fn providers_without_reset_window_pace_keep_the_raw_window() {
        let usage =
            UsageSnapshot::new(RateWindow::new(10.0)).with_secondary(monthly(50.0, Some(43_200)));

        let snap = snapshot_at(ProviderId::Gemini, usage);

        let pace = lane_pace(snap.secondary.as_ref().expect("weekly lane"));
        assert!((pace.expected_used_percent - 160.0 / 3.0).abs() < 1e-9);
        assert!(!pace.reset_window);
    }

    /// A spend budget (Vercel's personal budget) opts out of pace in its
    /// fetch result, so the card gets neither a pace marker nor a reserve.
    #[test]
    fn non_authoritative_spend_budget_has_no_pace_or_reserve() {
        let metadata = instantiate_provider(ProviderId::Vercel).metadata().clone();
        let mut budget = monthly(50.0, Some(40_320));
        budget.reset_description = Some("$42.00 / $84.00".into());
        budget.description_is_detail = true;
        let result = ProviderFetchResult::new(UsageSnapshot::new(budget), "api")
            .with_non_authoritative_pace();

        let snap = ProviderUsageSnapshot::from_fetch_result_at(
            ProviderId::Vercel,
            &metadata,
            &result,
            None,
            None,
            february_now(),
        );

        assert_eq!(snap.primary.pace, None);
        assert_eq!(snap.primary.reserve_percent, None);
        assert!(!snap.primary.reserve_will_last_to_reset);
        assert!(snap.pace.is_none());
        // The frontend derives on-pace budgets from window timing, so it
        // needs the flag as well.
        let json = serde_json::to_value(&snap).unwrap();
        assert_eq!(json["paceAuthoritative"], serde_json::Value::Bool(false));
        let cached: ProviderUsageSnapshot = serde_json::from_value(json).unwrap();
        assert!(!cached.pace_authoritative);
    }

    /// A calendar-month credit allowance carries its daily target on the card
    /// lane: 200 remaining credits over the 14 local days to the Mar 1 reset.
    /// Local-day counting varies by zone, so the bound allows one day either way.
    #[test]
    fn monthly_credit_allowance_carries_a_daily_target_on_the_card_lane() {
        let window = monthly(20.0, Some(43_200)).with_quota(250.0, "credits");

        let snap = snapshot_at(ProviderId::Notion, UsageSnapshot::new(window));

        let target = snap.primary.daily_target.expect("daily target");
        assert_eq!(target.unit, "credits");
        let days = 200.0 / target.amount;
        assert!((13.0..=15.0).contains(&days), "{days}");
    }

    /// An exhausted allowance has no lane pace, but the card still shows its
    /// target, as upstream computes the target apart from pace.
    #[test]
    fn exhausted_monthly_allowance_keeps_a_zero_daily_target_without_pace() {
        let window = monthly(100.0, Some(43_200)).with_quota(250.0, "credits");

        let snap = snapshot_at(ProviderId::Notion, UsageSnapshot::new(window));

        assert_eq!(snap.primary.pace, None);
        assert_eq!(
            snap.primary.daily_target,
            Some(DailyTargetSnapshot {
                amount: 0.0,
                unit: "credits".to_string(),
            })
        );
    }

    #[test]
    fn weekly_window_has_no_daily_target() {
        // Reset five days out, inside the seven-day window.
        let reset = Utc.with_ymd_and_hms(2026, 2, 20, 0, 0, 0).unwrap();
        let window = RateWindow::with_details(20.0, Some(10_080), Some(reset), None)
            .with_quota(250.0, "credits");

        let snap = snapshot_at(ProviderId::Notion, UsageSnapshot::new(window));

        assert!(snap.primary.pace.is_some());
        assert_eq!(snap.primary.daily_target, None);
    }

    /// A provider that opts out of pace gets no daily target either, so the
    /// target follows the same pace visibility as the rest of the lane.
    #[test]
    fn non_authoritative_monthly_allowance_has_no_daily_target() {
        let metadata = instantiate_provider(ProviderId::Notion).metadata().clone();
        let window = monthly(20.0, Some(43_200)).with_quota(250.0, "credits");
        let result = ProviderFetchResult::new(UsageSnapshot::new(window), "web")
            .with_non_authoritative_pace();

        let snap = ProviderUsageSnapshot::from_fetch_result_at(
            ProviderId::Notion,
            &metadata,
            &result,
            None,
            None,
            february_now(),
        );

        assert_eq!(snap.primary.pace, None);
        assert_eq!(snap.primary.daily_target, None);
    }

    #[test]
    fn exhausted_reset_window_has_no_pace() {
        let usage = UsageSnapshot::new(monthly(100.0, None));

        let snap = snapshot_at(ProviderId::Copilot, usage);

        assert_eq!(snap.primary.pace, None);
    }

    #[test]
    fn reset_window_flag_serializes_only_when_set() {
        let usage = UsageSnapshot::new(monthly(60.0, None));
        let snap = snapshot_at(ProviderId::Copilot, usage);

        let json = serde_json::to_value(snap.primary.pace.as_ref().unwrap()).unwrap();
        assert_eq!(json["resetWindow"], serde_json::Value::Bool(true));
        let cached: WindowPaceSnapshot = serde_json::from_str(
            r#"{"stage":"on_track","deltaPercent":0.0,"expectedUsedPercent":50.0,"actualUsedPercent":50.0}"#,
        )
        .unwrap();
        assert!(!cached.reset_window);
    }
}
