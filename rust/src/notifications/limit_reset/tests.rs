use super::*;
use crate::settings::Language;

const EMAIL: &str = "limit-reset-notice@example.com";
const OTHER: &str = "other@example.com";
const START: i64 = 1_784_600_000;
const RESTORED: &str = "Session Restored";
const DEPLETED: &str = "Session Depleted";

fn at(offset_secs: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(START + offset_secs, 0).unwrap()
}

/// Offset in seconds of the `index`-th sample, taken one minute apart.
fn minutes(index: usize) -> i64 {
    i64::try_from(index).unwrap() * 60
}

fn base_settings() -> Settings {
    Settings {
        limit_reset_notifications_enabled: true,
        session_quota_notifications_enabled: false,
        hide_personal_info: false,
        sound_enabled: false,
        ui_language: Language::English,
        ..Settings::default()
    }
}

/// One notifier spy across restarts, like upstream's `NotifierSpy`.
struct Harness {
    root: tempfile::TempDir,
    manager: NotificationManager,
    settings: Settings,
    earlier_toasts: Vec<String>,
}

impl Harness {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("limit-reset-")
            .tempdir()
            .expect("temp dir");
        let manager = NotificationManager::with_config_root(root.path());
        Self {
            root,
            manager,
            settings: base_settings(),
            earlier_toasts: Vec::new(),
        }
    }

    fn restart(&mut self) {
        let toasts = self.manager.toasts.take();
        self.earlier_toasts.extend(toasts);
        self.manager = NotificationManager::with_config_root(self.root.path());
    }

    fn state_path(&self) -> PathBuf {
        self.root.path().join(STORE_RELATIVE_PATH)
    }

    fn toasts(&self) -> Vec<String> {
        let mut all = self.earlier_toasts.clone();
        all.extend(self.manager.toasts.borrow().iter().cloned());
        all
    }

    fn resets(&self) -> Vec<String> {
        self.toasts()
            .into_iter()
            .filter(|toast| toast.contains("limit reset"))
            .collect()
    }

    fn transitions(&self) -> Vec<&'static str> {
        self.toasts()
            .iter()
            .filter_map(|toast| {
                if toast.starts_with(RESTORED) {
                    Some(RESTORED)
                } else if toast.starts_with(DEPLETED) {
                    Some(DEPLETED)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Upstream `handleSessionQuotaTransition`.
    fn session_transition(&mut self, email: &str, session_used: f64) {
        self.manager.check_session_transition(
            ProviderId::Claude,
            email,
            session_used,
            &self.settings,
        );
    }

    fn finish_batch(&mut self, now: DateTime<Utc>) {
        if let Some(persistence) = self.manager.finish_limit_reset_batch(&self.settings, now) {
            persistence.apply();
        }
    }

    fn record_with(
        &mut self,
        email: &str,
        session_used: f64,
        weekly_used: f64,
        offset: i64,
        boundary: Option<DateTime<Utc>>,
    ) {
        let observed_at = at(offset);
        let observation = |used_percent| {
            Some(LimitResetObservation {
                used_percent,
                observed_at,
                reset_boundary: boundary,
            })
        };
        self.manager.observe_limit_reset(
            LimitResetSample {
                provider: ProviderId::Claude,
                account: Some(account(email)),
                session_transition_account: email,
                session: observation(session_used),
                weekly: observation(weekly_used),
            },
            &self.settings,
        );
        self.finish_batch(observed_at);
    }

    /// Upstream `recordPlanUtilizationHistorySample` for the default account.
    fn record(&mut self, session_used: f64, weekly_used: f64, offset: i64) {
        self.record_with(EMAIL, session_used, weekly_used, offset, None);
    }

    /// A transition and a detector sample from the same snapshot.
    fn refresh(&mut self, email: &str, session_used: f64, weekly_used: f64, offset: i64) {
        self.session_transition(email, session_used);
        self.record_with(email, session_used, weekly_used, offset, None);
    }
}

fn account(email: &str) -> LimitResetAccount {
    LimitResetAccount {
        key: email.to_string(),
        label: Some(email.to_string()),
        requires_low_confirmation: false,
    }
}

fn weekly_reset(label: Option<&str>) -> String {
    reset_toast("weekly", label)
}

fn session_reset(label: Option<&str>) -> String {
    reset_toast("session", label)
}

fn reset_toast(window: &str, label: Option<&str>) -> String {
    match label {
        Some(label) => {
            format!("Claude {window} limit reset: Account {label}. Fresh quota is available.")
        }
        None => format!("Claude {window} limit reset: Fresh quota is available."),
    }
}

#[test]
fn weekly_reset_notice_names_provider_window_and_account() {
    let mut h = Harness::new();
    h.record(20.0, 60.0, 0);
    h.record(20.0, 0.0, 3600);
    assert_eq!(h.resets(), [weekly_reset(Some(EMAIL))]);
}

#[test]
fn session_reset_notice_names_the_session_window() {
    let mut h = Harness::new();
    h.record(65.0, 20.0, 0);
    h.record(0.0, 20.0, 3600);
    assert_eq!(h.resets(), [session_reset(Some(EMAIL))]);
}

#[test]
fn reset_notices_stay_silent_when_disabled() {
    let mut h = Harness::new();
    h.settings.limit_reset_notifications_enabled = false;
    h.record(20.0, 60.0, 0);
    h.record(20.0, 0.0, 3600);
    assert!(h.toasts().is_empty());
    assert!(!h.state_path().exists());
}

#[test]
fn hide_personal_info_omits_the_account_from_reset_notices() {
    let mut h = Harness::new();
    h.settings.hide_personal_info = true;
    h.record(20.0, 60.0, 0);
    h.record(20.0, 0.0, 3600);
    assert_eq!(h.resets(), [weekly_reset(None)]);
}

#[test]
fn blank_account_labels_are_omitted() {
    let settings = base_settings();
    let (_, body) = limit_reset_notification_copy(
        &settings,
        ProviderId::Claude,
        LimitResetWindow::Weekly,
        Some("   "),
    );
    assert_eq!(body, "Fresh quota is available.");
}

#[test]
fn session_restored_notice_covers_the_matching_session_reset_notice() {
    let mut h = Harness::new();
    h.settings.session_quota_notifications_enabled = true;
    h.refresh(EMAIL, 100.0, 60.0, 0);
    h.refresh(EMAIL, 0.0, 0.0, 3600);
    assert_eq!(h.transitions(), [DEPLETED, RESTORED]);
    assert_eq!(h.resets(), [weekly_reset(Some(EMAIL))]);
}

#[test]
fn reset_notice_copy_names_provider_and_window() {
    let settings = base_settings();
    let weekly = limit_reset_notification_copy(
        &settings,
        ProviderId::Claude,
        LimitResetWindow::Weekly,
        None,
    );
    assert_eq!(weekly.0, "Claude weekly limit reset");
    assert_eq!(weekly.1, "Fresh quota is available.");
    let session = limit_reset_notification_copy(
        &settings,
        ProviderId::Codex,
        LimitResetWindow::Session,
        Some("work@example.com"),
    );
    assert_eq!(session.0, "Codex session limit reset");
    assert_eq!(
        session.1,
        "Account work@example.com. Fresh quota is available."
    );
}

#[test]
fn reset_notice_copy_is_localized_for_simplified_chinese() {
    let settings = Settings {
        ui_language: Language::Chinese,
        ..base_settings()
    };
    let (title, _) = limit_reset_notification_copy(
        &settings,
        ProviderId::Claude,
        LimitResetWindow::Weekly,
        None,
    );
    assert_eq!(title, "Claude 每周额度已重置");
}

#[test]
fn restored_notice_for_one_account_does_not_suppress_another_account_reset() {
    let mut h = Harness::new();
    h.settings.session_quota_notifications_enabled = true;
    h.session_transition(EMAIL, 100.0);
    h.refresh(EMAIL, 0.0, 20.0, 60);
    assert_eq!(h.transitions(), [DEPLETED, RESTORED]);
    h.record_with(OTHER, 60.0, 20.0, 0, None);
    h.record_with(OTHER, 0.0, 20.0, 60, None);
    assert_eq!(h.resets(), [session_reset(Some(OTHER))]);
}

#[test]
fn weekly_reset_boundary_is_notified_once_across_refreshes_and_restarts() {
    let mut h = Harness::new();
    let boundary = Some(at(7 * 86_400));
    for (index, used) in [60.0, 0.0, 20.0, 30.0, 0.0].into_iter().enumerate() {
        if index == 2 {
            h.restart();
        }
        h.record_with(EMAIL, 20.0, used, minutes(index), boundary);
    }
    assert_eq!(h.resets().len(), 1);
}

#[test]
fn a_restored_confirmation_after_restart_does_not_repeat_a_reset_banner() {
    let mut h = Harness::new();
    h.record(60.0, 20.0, 0);
    h.record(0.0, 20.0, 60);
    h.restart();
    h.settings.session_quota_notifications_enabled = true;
    h.session_transition(EMAIL, 100.0);
    h.refresh(EMAIL, 0.0, 20.0, 120);
    assert_eq!(h.resets().len(), 1);
    assert_eq!(h.transitions(), [DEPLETED]);
}

#[test]
fn reset_notices_recover_when_a_provider_stops_reporting_reset_boundaries() {
    for missing_from in [2usize, 4] {
        let mut h = Harness::new();
        for (index, used) in [60.0, 0.0, 20.0, 30.0, 0.0].into_iter().enumerate() {
            if index == 2 {
                h.restart();
            }
            let boundary = (index < missing_from).then(|| at(7 * 86_400));
            h.record_with(EMAIL, 20.0, used, minutes(index), boundary);
        }
        assert_eq!(h.resets().len(), 2, "missing from sample {missing_from}");
    }
}

#[test]
fn partial_restored_notice_also_covers_the_later_confirmed_reset() {
    let mut h = Harness::new();
    h.settings.session_quota_notifications_enabled = true;
    for (index, used) in [100.0, 50.0].into_iter().enumerate() {
        h.refresh(EMAIL, used, 20.0, minutes(index));
    }
    h.record(0.0, 20.0, 120);
    assert_eq!(h.transitions(), [DEPLETED, RESTORED]);
    assert!(h.resets().is_empty());
    h.record(60.0, 20.0, 180);
    h.record(0.0, 20.0, 240);
    assert_eq!(h.resets().len(), 1);
}

#[test]
fn known_reset_boundaries_remain_deduplicated_after_a_metadata_gap() {
    let mut h = Harness::new();
    let boundary = at(7 * 86_400);
    for (index, used) in [60.0, 0.0, 20.0, 30.0, 0.0, 20.0, 30.0, 0.0]
        .into_iter()
        .enumerate()
    {
        let reset = (!(2..=4).contains(&index)).then_some(boundary);
        h.record_with(EMAIL, 20.0, used, minutes(index), reset);
    }
    assert_eq!(h.resets().len(), 2);
}

#[test]
fn new_depleted_episodes_still_deliver_restored_notices_without_full_resets() {
    for with_reset_metadata in [false, true] {
        let mut h = Harness::new();
        h.settings.session_quota_notifications_enabled = true;
        let boundary = with_reset_metadata.then(|| at(5 * 3600));
        for (index, used) in [100.0, 50.0, 100.0, 50.0].into_iter().enumerate() {
            h.session_transition(EMAIL, used);
            h.record_with(EMAIL, used, 20.0, minutes(index), boundary);
        }
        assert_eq!(
            h.transitions(),
            [DEPLETED, RESTORED, DEPLETED, RESTORED],
            "with reset metadata: {with_reset_metadata}"
        );
        assert!(h.resets().is_empty());
    }
}

#[test]
fn advanced_boundaries_and_independent_windows_notify_separately() {
    let mut h = Harness::new();
    for (index, used) in [60.0, 0.0, 20.0, 30.0, 0.0].into_iter().enumerate() {
        let weeks = if index < 4 { 1 } else { 2 };
        let boundary = Some(at(weeks * 7 * 86_400));
        h.record_with(EMAIL, used, used, minutes(index), boundary);
    }
    let windows: Vec<&str> = h
        .resets()
        .iter()
        .map(|toast| {
            if toast.contains("weekly") {
                "weekly"
            } else {
                "session"
            }
        })
        .collect();
    assert_eq!(windows, ["weekly", "session", "weekly"]);
}

/// Upstream rechecks the opt-in and privacy before an asynchronous delivery.
/// Windows delivers in the same call that detects the reset, so the banner
/// always reflects the settings of that refresh.
#[test]
fn reset_delivery_uses_the_settings_of_the_detecting_refresh() {
    let mut h = Harness::new();
    h.record(20.0, 60.0, 0);
    h.settings.hide_personal_info = true;
    h.record(20.0, 0.0, 60);
    assert_eq!(h.resets(), [weekly_reset(None)]);

    let mut h = Harness::new();
    h.record(20.0, 60.0, 0);
    h.settings.limit_reset_notifications_enabled = false;
    h.record(20.0, 0.0, 60);
    assert!(h.resets().is_empty());
}

#[test]
fn a_later_restored_confirmation_does_not_repeat_an_announced_reset() {
    let mut h = Harness::new();
    h.record(60.0, 20.0, 0);
    h.record(0.0, 20.0, 60);
    h.restart();
    h.settings.session_quota_notifications_enabled = true;
    // Arm a restored transition without feeding the detector the depleted sample.
    h.session_transition(EMAIL, 100.0);
    h.refresh(EMAIL, 0.0, 20.0, 120);
    assert_eq!(h.resets().len(), 1);
    assert!(!h.transitions().contains(&RESTORED));
    h.record(60.0, 20.0, 180);
    h.record(0.0, 20.0, 240);
    assert_eq!(h.resets().len(), 2);
}

#[test]
fn switching_from_a_depleted_account_does_not_cover_another_accounts_reset() {
    let mut h = Harness::new();
    h.settings.session_quota_notifications_enabled = true;
    h.refresh(OTHER, 60.0, 20.0, 0);
    h.refresh(EMAIL, 100.0, 20.0, 60);
    h.refresh(OTHER, 0.0, 20.0, 120);
    assert!(!h.transitions().contains(&RESTORED));
    assert_eq!(h.resets(), [session_reset(Some(OTHER))]);
}

#[test]
fn delayed_restored_confirmation_stays_covered_when_usage_has_resumed() {
    let receipt = Some(NotificationReceipt {
        reset_boundary: None,
    });
    let previous = DetectorState {
        notification_receipt: receipt,
        ..DetectorState::new(false, at(0))
    };
    let mut current = DetectorState {
        notification_receipt: receipt,
        ..DetectorState::new(true, at(60))
    };
    let observation = LimitResetObservation {
        used_percent: 2.0,
        observed_at: at(60),
        reset_boundary: None,
    };
    assert_eq!(
        prepare_notice(&mut current, Some(&previous), &observation, false, true),
        None
    );
    let mut next = DetectorState {
        notification_receipt: current.notification_receipt,
        ..DetectorState::new(false, at(120))
    };
    let observation = LimitResetObservation {
        used_percent: 0.0,
        observed_at: at(120),
        reset_boundary: None,
    };
    assert!(prepare_notice(&mut next, Some(&current), &observation, true, false).is_some());
}

// Windows-specific coverage.

#[test]
fn stale_observations_never_refire_when_the_cache_is_re_evaluated() {
    let mut h = Harness::new();
    h.record(20.0, 60.0, 0);
    h.record(20.0, 0.0, 60);
    // The same cached snapshot is fed every batch; older samples are ignored too.
    for _ in 0..5 {
        h.record(20.0, 0.0, 60);
    }
    h.record(20.0, 60.0, 30);
    h.record(20.0, 0.0, 45);
    assert_eq!(h.resets().len(), 1);
}

#[test]
fn excluded_providers_and_untracked_accounts_skip_detection_but_keep_restored() {
    for (provider, account) in [
        (ProviderId::TinyApi, Some(account(EMAIL))),
        (ProviderId::Xapi, Some(account(EMAIL))),
        (ProviderId::Langdock, Some(account(EMAIL))),
        (ProviderId::Exa, Some(account(EMAIL))),
        // Codex without an owner key / Claude OAuth with an unresolved owner.
        (ProviderId::Codex, None),
    ] {
        assert_eq!(
            limit_reset_supported(provider),
            !matches!(
                provider,
                ProviderId::TinyApi | ProviderId::Xapi | ProviderId::Langdock | ProviderId::Exa
            )
        );
        let mut h = Harness::new();
        h.settings.session_quota_notifications_enabled = true;
        for (offset, used) in [(0, 100.0), (60, 0.0)] {
            h.manager
                .check_session_transition(provider, "acct", used, &h.settings);
            h.manager.observe_limit_reset(
                LimitResetSample {
                    provider,
                    account: account.clone(),
                    session_transition_account: "acct",
                    session: Some(LimitResetObservation {
                        used_percent: used,
                        observed_at: at(offset),
                        reset_boundary: None,
                    }),
                    weekly: None,
                },
                &h.settings,
            );
            h.finish_batch(at(offset));
        }
        assert_eq!(h.transitions(), [DEPLETED, RESTORED], "{provider:?}");
        assert!(h.resets().is_empty(), "{provider:?}");
        assert!(!h.state_path().exists(), "{provider:?}");
    }
}

#[test]
fn restored_toasts_no_detector_claimed_are_posted_when_the_batch_finishes() {
    let mut h = Harness::new();
    h.settings.session_quota_notifications_enabled = true;
    h.session_transition(EMAIL, 100.0);
    h.session_transition(EMAIL, 0.0);
    assert_eq!(h.transitions(), [DEPLETED]);
    h.finish_batch(at(0));
    assert_eq!(h.transitions(), [DEPLETED, RESTORED]);
}

#[test]
fn setting_off_keeps_todays_restored_behaviour() {
    let mut h = Harness::new();
    h.settings.limit_reset_notifications_enabled = false;
    h.settings.session_quota_notifications_enabled = true;
    h.session_transition(EMAIL, 100.0);
    h.session_transition(EMAIL, 0.0);
    // Posted immediately, not deferred to the batch end.
    assert_eq!(h.transitions(), [DEPLETED, RESTORED]);
    assert!(h.manager.deferred_restored.is_empty());
    assert_eq!(h.manager.finish_limit_reset_batch(&h.settings, at(0)), None);
}

#[test]
fn persisted_state_uses_hashed_keys_without_raw_emails() {
    let mut h = Harness::new();
    h.record(60.0, 60.0, 0);
    let on_disk = String::from_utf8_lossy(&std::fs::read(h.state_path()).expect("state file"))
        .to_ascii_lowercase();
    assert!(!on_disk.contains("example.com"));
    // Decoded contents (DPAPI-protected on Windows) carry only hashed keys too.
    let raw = secure_file::read_string(&h.state_path()).expect("read state");
    assert!(raw.contains("\"version\": 1"));
    assert!(!raw.to_ascii_lowercase().contains("example.com"));
    assert!(!raw.contains("limit-reset-notice"));
    let store: LimitResetStore = serde_json::from_str(&raw).expect("decode store");
    let key = hashed_state_key(ProviderId::Claude, EMAIL);
    assert_eq!(key.len(), 64);
    assert!(store.session.contains_key(&key));
    assert!(store.weekly.contains_key(&key));
}

#[test]
fn pruning_drops_old_states_and_caps_each_window() {
    let mut store = LimitResetStore::default();
    let now = at(40 * 86_400);
    store
        .session
        .insert("old".to_string(), DetectorState::new(true, at(0)));
    store.session.insert(
        "recent".to_string(),
        DetectorState::new(true, at(39 * 86_400)),
    );
    for index in 0..300 {
        store.weekly.insert(
            format!("w{index:03}"),
            DetectorState::new(true, at(39 * 86_400 + index)),
        );
    }
    store.prune(now);
    assert_eq!(store.session.keys().collect::<Vec<_>>(), ["recent"]);
    assert_eq!(store.weekly.len(), MAX_STATES_PER_WINDOW);
    assert!(!store.weekly.contains_key("w000"));
    assert!(store.weekly.contains_key("w299"));
}

#[test]
fn unsupported_version_or_corrupt_state_starts_empty() {
    for contents in [
        r#"{"version": 99, "session": {}, "weekly": {}}"#.to_string(),
        "not json".to_string(),
    ] {
        let mut h = Harness::new();
        write_atomically(&h.state_path(), &contents).expect("seed state");
        h.restart();
        // A fresh baseline: the first observation never posts.
        h.record(20.0, 0.0, 0);
        h.record(20.0, 60.0, 60);
        h.record(20.0, 0.0, 120);
        assert_eq!(h.resets().len(), 1);
        let raw = secure_file::read_string(&h.state_path()).expect("rewritten");
        assert!(raw.contains("\"version\": 1"));
    }
}

#[test]
fn claude_identity_less_samples_need_two_consecutive_low_samples() {
    let mut h = Harness::new();
    let unresolved = LimitResetAccount {
        key: "claude:cli:unknown".to_string(),
        label: None,
        requires_low_confirmation: true,
    };
    let observe = |h: &mut Harness, used: f64, offset: i64| {
        let observation = Some(LimitResetObservation {
            used_percent: used,
            observed_at: at(offset),
            reset_boundary: None,
        });
        h.manager.observe_limit_reset(
            LimitResetSample {
                provider: ProviderId::Claude,
                account: Some(unresolved.clone()),
                session_transition_account: "claude:cli:unknown",
                session: observation,
                weekly: None,
            },
            &h.settings,
        );
        h.finish_batch(at(offset));
    };
    observe(&mut h, 60.0, 0);
    observe(&mut h, 0.0, 60);
    assert!(h.resets().is_empty(), "one low sample is not enough");
    // A transient low followed by usage again does not announce anything.
    observe(&mut h, 60.0, 120);
    assert!(h.resets().is_empty());
    observe(&mut h, 0.0, 180);
    observe(&mut h, 0.0, 240);
    assert_eq!(h.resets(), [session_reset(None)]);
}

#[test]
fn a_cached_sample_seen_again_does_not_confirm_a_low_reading() {
    let mut h = Harness::new();
    let unresolved = LimitResetAccount {
        key: "claude:cli:unknown".to_string(),
        label: None,
        requires_low_confirmation: true,
    };
    let observe = |h: &mut Harness, used: f64, offset: i64| {
        h.manager.observe_limit_reset(
            LimitResetSample {
                provider: ProviderId::Claude,
                account: Some(unresolved.clone()),
                session_transition_account: "claude:cli:unknown",
                session: Some(LimitResetObservation {
                    used_percent: used,
                    observed_at: at(offset),
                    reset_boundary: None,
                }),
                weekly: None,
            },
            &h.settings,
        );
        h.finish_batch(at(offset));
    };
    observe(&mut h, 60.0, 0);
    observe(&mut h, 0.0, 60);
    // Every refresh batch re-reads the whole cache, so an unchanged snapshot
    // arrives again with the same timestamp.
    observe(&mut h, 0.0, 60);
    assert!(h.resets().is_empty());
    observe(&mut h, 0.0, 120);
    assert_eq!(h.resets(), [session_reset(None)]);
}

#[test]
fn claude_weekly_rearms_only_after_two_samples_above_one_percent() {
    let mut h = Harness::new();
    h.record(20.0, 60.0, 0);
    h.record(20.0, 0.0, 60);
    assert_eq!(h.resets().len(), 1);
    // One sample above 1% is not a new usage cycle yet.
    h.record(20.0, 30.0, 120);
    h.record(20.0, 0.0, 180);
    assert_eq!(h.resets().len(), 1);
    h.record(20.0, 30.0, 240);
    h.record(20.0, 30.0, 300);
    h.record(20.0, 0.0, 360);
    assert_eq!(h.resets().len(), 2);
}

#[test]
fn codex_weekly_uses_the_advanced_boundary_fast_path_only() {
    let observe = |previous: Option<&DetectorState>, used: f64, boundary: Option<i64>| {
        detector_transition(TransitionInput {
            provider: ProviderId::Codex,
            window: LimitResetWindow::Weekly,
            observation: LimitResetObservation {
                used_percent: used,
                observed_at: at(3600),
                reset_boundary: boundary.map(at),
            },
            previous,
            requires_low_confirmation: false,
        })
    };
    let high = |boundary: Option<i64>| DetectorState {
        reset_boundary: boundary.map(at),
        ..DetectorState::new(true, at(0))
    };
    let week = 7 * 86_400;
    // Advanced boundary and at most 5% used: reset.
    assert!(observe(Some(&high(Some(week))), 4.0, Some(2 * week)).should_post);
    // Unchanged boundary: no reset, baseline kept.
    let guarded = observe(Some(&high(Some(week))), 0.0, Some(week));
    assert!(!guarded.should_post);
    assert!(guarded.state.was_above_threshold);
    // A previous boundary is required.
    let first = observe(Some(&high(None)), 0.0, Some(2 * week));
    assert!(!first.should_post);
    assert_eq!(first.state.reset_boundary, Some(at(2 * week)));
    // Above 5% after the rollover is ordinary usage.
    assert!(!observe(Some(&high(Some(week))), 6.0, Some(2 * week)).should_post);
}

#[test]
fn turning_the_setting_off_clears_saved_state_in_memory_and_on_disk() {
    let mut h = Harness::new();
    h.record(20.0, 60.0, 0);
    assert!(h.state_path().exists());

    h.settings.limit_reset_notifications_enabled = false;
    h.finish_batch(at(30));
    assert!(!h.state_path().exists());
    assert!(h.manager.limit_reset.store.is_empty());

    // Back on, the old high baseline is gone: this low sample is a first sample.
    h.settings.limit_reset_notifications_enabled = true;
    h.record(20.0, 0.0, 60);
    assert!(h.resets().is_empty());

    // A file left by an earlier run is removed even before anything was loaded.
    h.record(20.0, 60.0, 120);
    h.restart();
    h.settings.limit_reset_notifications_enabled = false;
    h.finish_batch(at(180));
    assert!(!h.state_path().exists());
    h.settings.limit_reset_notifications_enabled = true;
    h.record(20.0, 0.0, 240);
    assert!(h.resets().is_empty());
}

#[test]
fn clear_limit_reset_state_returns_the_removal() {
    let mut h = Harness::new();
    h.record(20.0, 60.0, 0);
    let removal = h.manager.clear_limit_reset_state();
    assert_eq!(
        removal,
        Some(LimitResetPersistence::Remove {
            path: h.state_path()
        })
    );
    removal.unwrap().apply();
    assert!(!h.state_path().exists());
}
