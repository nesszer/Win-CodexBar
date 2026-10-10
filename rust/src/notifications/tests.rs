use super::*;
use crate::core::{PaceStage, RateWindow, UsagePace};
use chrono::{DateTime, Duration, Utc};

#[test]
fn notification_types_map_to_their_sound_events() {
    let mappings = [
        (
            NotificationType::HighUsage,
            NotificationSoundEvent::HighUsage,
        ),
        (
            NotificationType::CriticalUsage,
            NotificationSoundEvent::CriticalUsage,
        ),
        (
            NotificationType::Exhausted,
            NotificationSoundEvent::Exhausted,
        ),
        (
            NotificationType::StatusIssue,
            NotificationSoundEvent::StatusIssue,
        ),
        (
            NotificationType::SessionDepleted,
            NotificationSoundEvent::SessionDepleted,
        ),
        (
            NotificationType::SessionRestored,
            NotificationSoundEvent::SessionRestored,
        ),
    ];

    for (notification_type, sound_event) in mappings {
        assert_eq!(
            NotificationManager::sound_event_for(notification_type),
            sound_event
        );
    }
}

#[test]
fn usage_toasts_follow_ui_language() {
    use crate::settings::Language;

    assert_eq!(
        NotificationType::HighUsage.title(Language::English),
        "High Usage Warning"
    );
    assert_eq!(
        NotificationManager::notification_body(
            ProviderId::Claude,
            "weekly",
            86.0,
            NotificationType::HighUsage,
            Language::English,
        ),
        "Claude weekly usage at 86% - approaching limit"
    );
    assert_eq!(
        NotificationType::HighUsage.title(Language::Russian),
        "Высокое использование"
    );
    assert_eq!(
        NotificationManager::notification_body(
            ProviderId::Claude,
            "weekly",
            86.0,
            NotificationType::HighUsage,
            Language::Russian,
        ),
        "Claude (неделя): использовано 86% — лимит почти исчерпан"
    );
    // Unknown windows pass through untranslated instead of falling back.
    assert_eq!(
        NotificationManager::notification_body(
            ProviderId::Claude,
            "opus",
            100.0,
            NotificationType::Exhausted,
            Language::English,
        ),
        "Claude opus usage limit exhausted (100%)"
    );
}

fn pace(will_last_to_reset: bool, eta_seconds: Option<f64>) -> UsagePace {
    UsagePace {
        stage: PaceStage::Ahead,
        delta_percent: 20.0,
        expected_used_percent: 40.0,
        actual_used_percent: 60.0,
        eta_seconds,
        will_last_to_reset,
    }
}

fn window(now: DateTime<Utc>, offset: Duration, minutes: u32) -> RateWindow {
    RateWindow::with_details(60.0, Some(minutes), Some(now + offset), None)
}

#[test]
fn predictive_warning_notifies_once_until_recovery_then_rearms() {
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let window = window(now, Duration::hours(3), 300);
    let risk = pace(false, Some(3600.0));
    let recovery = pace(true, None);
    let mut manager = NotificationManager::new();

    assert!(manager.record_predictive_observation(
        true,
        ProviderId::Claude,
        "oauth:person@example.com",
        PredictiveWarningWindow::Session,
        &window,
        &risk,
    ));
    assert!(!manager.record_predictive_observation(
        true,
        ProviderId::Claude,
        "oauth:person@example.com",
        PredictiveWarningWindow::Session,
        &window,
        &risk,
    ));
    assert!(!manager.record_predictive_observation(
        true,
        ProviderId::Claude,
        "oauth:person@example.com",
        PredictiveWarningWindow::Session,
        &window,
        &recovery,
    ));
    assert!(manager.record_predictive_observation(
        true,
        ProviderId::Claude,
        "oauth:person@example.com",
        PredictiveWarningWindow::Session,
        &window,
        &risk,
    ));
}

#[test]
fn predictive_warning_reset_jitter_does_not_retrigger() {
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let mut manager = NotificationManager::new();
    let risk = pace(false, Some(3600.0));

    assert!(manager.record_predictive_observation(
        true,
        ProviderId::Codex,
        "oauth:account-a",
        PredictiveWarningWindow::Weekly,
        &window(now, Duration::days(3), 10080),
        &risk,
    ));
    assert!(!manager.record_predictive_observation(
        true,
        ProviderId::Codex,
        "oauth:account-a",
        PredictiveWarningWindow::Weekly,
        &window(now, Duration::days(3) + Duration::minutes(5), 10080),
        &risk,
    ));
}

#[test]
fn predictive_warning_isolates_provider_identity_source_and_window() {
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let reset = window(now, Duration::hours(3), 300);
    let risk = pace(false, Some(3600.0));
    let mut manager = NotificationManager::new();

    for (provider, identity, warning_window) in [
        (
            ProviderId::Claude,
            "cli:person@example.com",
            PredictiveWarningWindow::Session,
        ),
        (
            ProviderId::Claude,
            "oauth:person@example.com",
            PredictiveWarningWindow::Session,
        ),
        (
            ProviderId::Claude,
            "token-account:1",
            PredictiveWarningWindow::Session,
        ),
        (
            ProviderId::Claude,
            "oauth:person@example.com",
            PredictiveWarningWindow::Weekly,
        ),
        (
            ProviderId::Codex,
            "oauth:person@example.com",
            PredictiveWarningWindow::Session,
        ),
    ] {
        assert!(manager.record_predictive_observation(
            true,
            provider,
            identity,
            warning_window,
            &reset,
            &risk,
        ));
    }
}

#[test]
fn predictive_warning_requires_enabled_confident_positive_risk() {
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let reset = window(now, Duration::hours(3), 300);
    let mut manager = NotificationManager::new();

    for (enabled, observation) in [
        (false, pace(false, Some(3600.0))),
        (true, pace(true, None)),
        (true, pace(false, Some(0.0))),
    ] {
        assert!(!manager.record_predictive_observation(
            enabled,
            ProviderId::Claude,
            "oauth:person@example.com",
            PredictiveWarningWindow::Session,
            &reset,
            &observation,
        ));
    }

    assert!(manager.record_predictive_observation(
        true,
        ProviderId::Claude,
        "oauth:person@example.com",
        PredictiveWarningWindow::Session,
        &reset,
        &pace(false, Some(3600.0)),
    ));
}

#[test]
fn unresolved_and_resolved_warning_histories_remain_separate() {
    let now = DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    let reset = window(now, Duration::hours(3), 300);
    let risk = pace(false, Some(3600.0));
    let mut manager = NotificationManager::new();

    assert!(manager.record_predictive_observation(
        true,
        ProviderId::Claude,
        "claude:oauth:unknown",
        PredictiveWarningWindow::Session,
        &reset,
        &risk,
    ));
    assert!(manager.record_predictive_observation(
        true,
        ProviderId::Claude,
        "oauth:person@example.com",
        PredictiveWarningWindow::Session,
        &reset,
        &risk,
    ));
    assert_eq!(manager.predictive_warning_keys.len(), 2);
}

#[test]
fn session_below_high_does_not_rearm_weekly_high_toast() {
    // Repro for #198: session cool + weekly hot on every refresh used to
    // clear all provider keys on the session call, then re-fire weekly.
    let mut manager = NotificationManager::new();
    let settings = Settings::default();
    assert!(settings.show_notifications);
    assert!((settings.high_usage_threshold - 70.0).abs() < f64::EPSILON);

    let account = "";
    let weekly_key = (
        ProviderId::Claude,
        account.to_string(),
        "weekly".to_string(),
        NotificationType::HighUsage,
    );

    manager.check_and_notify(ProviderId::Claude, account, "session", 20.0, &settings);
    manager.check_and_notify(ProviderId::Claude, account, "weekly", 76.0, &settings);
    assert!(manager.sent_notifications.contains(&weekly_key));

    // Simulate several refresh cycles: session still cool, weekly still hot.
    for _ in 0..5 {
        manager.check_and_notify(ProviderId::Claude, account, "session", 20.0, &settings);
        manager.check_and_notify(ProviderId::Claude, account, "weekly", 76.0, &settings);
    }
    assert_eq!(
        manager
            .sent_notifications
            .iter()
            .filter(|key| key == &&weekly_key)
            .count(),
        1,
        "weekly high toast must arm only once while still above threshold"
    );

    // Drop weekly below high → re-arm allowed on next climb.
    manager.check_and_notify(ProviderId::Claude, account, "weekly", 50.0, &settings);
    assert!(!manager.sent_notifications.contains(&weekly_key));
    manager.check_and_notify(ProviderId::Claude, account, "weekly", 76.0, &settings);
    assert!(manager.sent_notifications.contains(&weekly_key));
}

#[test]
fn missing_session_lane_preserves_warning_history_and_weekly_eligibility() {
    let settings = Settings {
        high_usage_threshold: 50.0,
        ..Settings::default()
    };
    let mut manager = NotificationManager::new();
    let account = "";

    let session_high = (
        ProviderId::Claude,
        account.to_string(),
        "session".to_string(),
        NotificationType::HighUsage,
    );
    let weekly_high = (
        ProviderId::Claude,
        account.to_string(),
        "weekly".to_string(),
        NotificationType::HighUsage,
    );

    assert!(manager.check_session_lane(ProviderId::Claude, account, 51.0, false, &settings,));
    assert!(manager.sent_notifications.contains(&session_high));

    assert!(!manager.check_session_lane(ProviderId::Claude, account, 0.0, true, &settings,));
    manager.check_and_notify(ProviderId::Claude, account, "weekly", 76.0, &settings);
    assert!(manager.sent_notifications.contains(&weekly_high));

    assert!(manager.check_session_lane(ProviderId::Claude, account, 52.0, false, &settings,));
    assert!(manager.check_session_lane(ProviderId::Claude, account, 81.0, false, &settings,));
    assert_eq!(
        manager
            .sent_notifications
            .iter()
            .filter(|key| **key == session_high)
            .count(),
        1,
        "missing session data must not re-arm the session warning"
    );
    assert!(manager.sent_notifications.contains(&weekly_high));
}

#[test]
fn threshold_keys_isolate_session_and_weekly() {
    let mut manager = NotificationManager::new();
    let settings = Settings::default();
    let account = "";

    manager.check_and_notify(ProviderId::Claude, account, "session", 75.0, &settings);
    manager.check_and_notify(ProviderId::Claude, account, "weekly", 75.0, &settings);

    assert!(manager.sent_notifications.contains(&(
        ProviderId::Claude,
        account.to_string(),
        "session".to_string(),
        NotificationType::HighUsage,
    )));
    assert!(manager.sent_notifications.contains(&(
        ProviderId::Claude,
        account.to_string(),
        "weekly".to_string(),
        NotificationType::HighUsage,
    )));

    // Cool only session; weekly stays armed.
    manager.check_and_notify(ProviderId::Claude, account, "session", 10.0, &settings);
    assert!(!manager.sent_notifications.contains(&(
        ProviderId::Claude,
        account.to_string(),
        "session".to_string(),
        NotificationType::HighUsage,
    )));
    assert!(manager.sent_notifications.contains(&(
        ProviderId::Claude,
        account.to_string(),
        "weekly".to_string(),
        NotificationType::HighUsage,
    )));
}

/// Mirrors `notify_usage_thresholds` in the Tauri shell: each refresh
/// calls session then weekly. Confidence pass for #198 over many cycles.
#[test]
fn refresh_loop_session_cool_weekly_hot_toasts_once() {
    let mut manager = NotificationManager::new();
    let settings = Settings::default();
    let account = "";
    let weekly_high = (
        ProviderId::Claude,
        account.to_string(),
        "weekly".to_string(),
        NotificationType::HighUsage,
    );

    let mut weekly_fires = 0usize;
    for _ in 0..30 {
        let before = manager.sent_notifications.contains(&weekly_high);
        // Same order as apps/desktop-tauri/.../providers.rs
        manager.check_and_notify(ProviderId::Claude, account, "session", 20.0, &settings);
        manager.check_and_notify(ProviderId::Claude, account, "weekly", 76.0, &settings);
        let after = manager.sent_notifications.contains(&weekly_high);
        if after && !before {
            weekly_fires += 1;
        }
    }

    assert_eq!(
        weekly_fires, 1,
        "weekly high must fire exactly once across 30 refresh cycles"
    );
    assert!(manager.sent_notifications.contains(&weekly_high));
    // Session never armed high while cool.
    assert!(!manager.sent_notifications.contains(&(
        ProviderId::Claude,
        account.to_string(),
        "session".to_string(),
        NotificationType::HighUsage,
    )));
}

#[test]
fn threshold_keys_isolate_accounts_on_same_provider() {
    // Two accounts on the same provider can each fire High once for weekly.
    let mut manager = NotificationManager::new();
    let settings = Settings::default();

    let key_a = (
        ProviderId::Claude,
        "account-a".to_string(),
        "weekly".to_string(),
        NotificationType::HighUsage,
    );
    let key_b = (
        ProviderId::Claude,
        "account-b".to_string(),
        "weekly".to_string(),
        NotificationType::HighUsage,
    );

    manager.check_and_notify(ProviderId::Claude, "account-a", "weekly", 80.0, &settings);
    manager.check_and_notify(ProviderId::Claude, "account-b", "weekly", 80.0, &settings);

    assert!(manager.sent_notifications.contains(&key_a));
    assert!(manager.sent_notifications.contains(&key_b));

    // Re-poll both still hot: neither re-fires (still armed, no second insert).
    manager.check_and_notify(ProviderId::Claude, "account-a", "weekly", 80.0, &settings);
    manager.check_and_notify(ProviderId::Claude, "account-b", "weekly", 80.0, &settings);
    assert_eq!(
        manager
            .sent_notifications
            .iter()
            .filter(|k| k.0 == ProviderId::Claude
                && k.2 == "weekly"
                && k.3 == NotificationType::HighUsage)
            .count(),
        2
    );
}

#[test]
fn account_a_session_cool_does_not_clear_account_b_weekly() {
    let mut manager = NotificationManager::new();
    let settings = Settings::default();

    let key_b_weekly = (
        ProviderId::Claude,
        "account-b".to_string(),
        "weekly".to_string(),
        NotificationType::HighUsage,
    );

    manager.check_and_notify(ProviderId::Claude, "account-b", "weekly", 80.0, &settings);
    assert!(manager.sent_notifications.contains(&key_b_weekly));

    // Account A session cool must not clear account B weekly armed state.
    manager.check_and_notify(ProviderId::Claude, "account-a", "session", 10.0, &settings);
    assert!(
        manager.sent_notifications.contains(&key_b_weekly),
        "account A session cool must not clear account B weekly threshold key"
    );

    // Account A weekly cool must also not clear account B.
    manager.check_and_notify(ProviderId::Claude, "account-a", "weekly", 10.0, &settings);
    assert!(manager.sent_notifications.contains(&key_b_weekly));
}

#[test]
fn session_transition_isolates_accounts() {
    let mut manager = NotificationManager::new();
    let settings = Settings::default();

    let depleted_a = (
        ProviderId::Claude,
        "account-a".to_string(),
        "session".to_string(),
        NotificationType::SessionDepleted,
    );
    let depleted_b = (
        ProviderId::Claude,
        "account-b".to_string(),
        "session".to_string(),
        NotificationType::SessionDepleted,
    );

    manager.check_session_transition(ProviderId::Claude, "account-a", 50.0, &settings);
    manager.check_session_transition(ProviderId::Claude, "account-a", 100.0, &settings);
    assert!(manager.sent_notifications.contains(&depleted_a));
    assert!(!manager.sent_notifications.contains(&depleted_b));

    // Account B still has quota; restoring A must not affect B's lane.
    manager.check_session_transition(ProviderId::Claude, "account-b", 40.0, &settings);
    manager.check_session_transition(ProviderId::Claude, "account-a", 20.0, &settings);
    assert!(!manager.sent_notifications.contains(&depleted_a));
    assert!(!manager.sent_notifications.contains(&depleted_b));

    // Account B can still fire depleted independently.
    manager.check_session_transition(ProviderId::Claude, "account-b", 100.0, &settings);
    assert!(manager.sent_notifications.contains(&depleted_b));
}
