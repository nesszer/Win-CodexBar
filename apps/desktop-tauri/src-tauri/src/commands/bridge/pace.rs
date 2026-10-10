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
}

impl WindowPaceSnapshot {
    pub(crate) fn for_window(
        window: &RateWindow,
        default_window_minutes: u32,
        now: DateTime<Utc>,
    ) -> Option<Self> {
        if window.is_informational {
            return None;
        }
        let pace = codexbar::core::UsagePace::weekly(window, Some(now), default_window_minutes)?;
        Some(Self {
            stage: stage_str(pace.stage).to_string(),
            delta_percent: pace.delta_percent,
            expected_used_percent: pace.expected_used_percent,
            actual_used_percent: pace.actual_used_percent,
            eta_seconds: pace.eta_seconds,
            will_last_to_reset: pace.will_last_to_reset,
        })
    }
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
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None)
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
            WindowPaceSnapshot::for_window(&window, WEEKLY_FALLBACK_MINUTES, fixed_now()),
            Some(WindowPaceSnapshot {
                stage: "far_behind".to_string(),
                delta_percent: -20.0,
                expected_used_percent: 50.0,
                actual_used_percent: 30.0,
                eta_seconds: None,
                will_last_to_reset: true,
            })
        );
    }

    #[test]
    fn deficit_is_a_positive_delta_with_an_eta() {
        let window = resetting_in(fixed_now(), 55.0, Some(10_080), 5_040);

        let pace = WindowPaceSnapshot::for_window(&window, WEEKLY_FALLBACK_MINUTES, fixed_now())
            .expect("pace");

        assert_eq!(pace.stage, "slightly_ahead");
        assert_eq!(pace.delta_percent, 5.0);
        assert!(!pace.will_last_to_reset);
        assert!((pace.eta_seconds.expect("eta") - 247_418.18).abs() < 0.01);
    }

    #[test]
    fn informational_window_has_no_pace() {
        let mut window = resetting_in(fixed_now(), 30.0, Some(10_080), 5_040);
        window.is_informational = true;

        assert_eq!(
            WindowPaceSnapshot::for_window(&window, WEEKLY_FALLBACK_MINUTES, fixed_now()),
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
    }

    #[test]
    fn window_pace_serializes_in_camel_case() {
        let window = resetting_in(fixed_now(), 30.0, Some(10_080), 5_040);
        let pace = WindowPaceSnapshot::for_window(&window, WEEKLY_FALLBACK_MINUTES, fixed_now());

        assert_eq!(
            serde_json::to_string(&pace).unwrap(),
            r#"{"stage":"far_behind","deltaPercent":-20.0,"expectedUsedPercent":50.0,"actualUsedPercent":30.0,"etaSeconds":null,"willLastToReset":true}"#
        );
    }
}
