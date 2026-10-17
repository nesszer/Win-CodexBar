//! Sequences mirror upstream `ClaudeCredentialQuotaWarningTests` (`checkIdentitySamples`).
//! Upstream thresholds are remaining-percent (50 / 20); locally high = 50% used and
//! critical = 80% used, so `remaining` values map to `100 - remaining` used.

use super::*;
use crate::notifications::NotificationType;
use crate::settings::Language;
use chrono::TimeZone;

const CLI_UNKNOWN: &str = "claude:cli:unknown";
const ACCOUNT_A: &str = "account-a@example.com";
const ACCOUNT_B: &str = "account-b@example.com";

#[derive(Clone, Copy)]
enum Lane {
    Session,
    Weekly,
}

impl Lane {
    fn window(self) -> &'static str {
        match self {
            Lane::Session => "session",
            Lane::Weekly => "weekly",
        }
    }
}

struct Sample {
    account: Option<&'static str>,
    remaining: f64,
    reset_offset_secs: Option<i64>,
}

fn sample(account: Option<&'static str>, remaining: f64, reset: Option<i64>) -> Sample {
    Sample {
        account,
        remaining,
        reset_offset_secs: reset,
    }
}

fn settings() -> Settings {
    Settings {
        high_usage_threshold: 50.0,
        critical_usage_threshold: 80.0,
        sound_enabled: false,
        ..Settings::default()
    }
}

fn scope_for(account: Option<&str>) -> WarningScope {
    match account {
        Some(account) => WarningScope::Resolved {
            account: account.to_string(),
            unresolved: CLI_UNKNOWN.to_string(),
        },
        None => WarningScope::Unresolved(CLI_UNKNOWN.to_string()),
    }
}

/// Feed samples the way the shell does and return the toast thresholds fired
/// (`50` for the high toast, `20` for the critical toast).
fn run_samples(samples: &[Sample], lane: Lane) -> Vec<u32> {
    let settings = settings();
    let mut manager = NotificationManager::new();
    let base = Utc.timestamp_opt(1_900_000_000, 0).unwrap();
    for sample in samples {
        let used = 100.0 - sample.remaining;
        let resets_at = sample
            .reset_offset_secs
            .map(|secs| base + chrono::Duration::seconds(secs));
        let account = manager.resolve_warning_account(
            ProviderId::Claude,
            &scope_for(sample.account),
            lane.window(),
            used,
            resets_at,
            &settings,
        );
        manager.check_and_notify(ProviderId::Claude, &account, lane.window(), used, &settings);
    }
    let toasts = manager.toasts.borrow();
    toasts
        .iter()
        .map(|toast| {
            if toast.starts_with(&NotificationType::CriticalUsage.title(Language::English)) {
                20
            } else if toast.starts_with(&NotificationType::HighUsage.title(Language::English)) {
                50
            } else {
                panic!("unexpected toast {toast}")
            }
        })
        .collect()
}

fn alternating(remaining: &[f64], reset: Option<i64>) -> Vec<Sample> {
    remaining
        .iter()
        .enumerate()
        .map(|(index, value)| sample((index % 2 == 0).then_some(ACCOUNT_A), *value, reset))
        .collect()
}

#[test]
fn repeated_identity_gaps_preserve_threshold_history() {
    for lane in [Lane::Session, Lane::Weekly] {
        for has_reset in [true, false] {
            let samples = alternating(
                &[49.0, 48.0, 47.0, 46.0, 45.0, 44.0],
                has_reset.then_some(3600),
            );
            let expected: &[u32] = if has_reset { &[50] } else { &[50, 50] };
            assert_eq!(run_samples(&samples, lane), expected, "reset={has_reset}");
        }
    }
}

#[test]
fn later_thresholds_do_not_repeat_across_either_identity_key() {
    for has_reset in [true, false] {
        for crossing_is_known in [true, false] {
            let remaining: &[f64] = if crossing_is_known {
                &[49.0, 48.0, 19.0, 18.0, 17.0, 16.0]
            } else {
                &[49.0, 48.0, 47.0, 19.0, 18.0, 17.0]
            };
            let expected: &[u32] = if has_reset { &[50, 20] } else { &[50, 50, 20] };
            let samples = alternating(remaining, has_reset.then_some(3600));
            assert_eq!(
                run_samples(&samples, Lane::Session),
                expected,
                "reset={has_reset} crossing_is_known={crossing_is_known}"
            );
        }
    }
}

#[test]
fn discontinuous_identity_gaps_start_one_independent_episode() {
    for discontinuity in ["reset", "increase", "missing"] {
        let reset = match discontinuity {
            "missing" => None,
            "reset" => Some(7200),
            _ => Some(3600),
        };
        let unresolved = |remaining: f64| {
            sample(
                None,
                if discontinuity == "increase" {
                    49.0
                } else {
                    remaining
                },
                reset,
            )
        };
        let samples = [
            sample(Some(ACCOUNT_A), 40.0, Some(3600)),
            unresolved(39.0),
            sample(Some(ACCOUNT_A), 38.0, Some(3600)),
            unresolved(37.0),
            sample(Some(ACCOUNT_A), 36.0, Some(3600)),
            unresolved(35.0),
        ];
        assert_eq!(
            run_samples(&samples, Lane::Session),
            [50, 50],
            "{discontinuity}"
        );
    }
}

#[test]
fn identity_gaps_follow_the_most_recent_account_without_merging_known_accounts() {
    let samples = [
        sample(Some(ACCOUNT_A), 49.0, Some(3600)),
        sample(Some(ACCOUNT_B), 48.0, Some(3600)),
        sample(None, 47.0, Some(3600)),
        sample(Some(ACCOUNT_A), 46.0, Some(3600)),
        sample(None, 45.0, Some(3600)),
        sample(Some(ACCOUNT_B), 44.0, Some(3600)),
        sample(None, 43.0, Some(3600)),
    ];
    assert_eq!(run_samples(&samples, Lane::Session), [50, 50]);
}

#[test]
fn initial_unresolved_history_survives_repeated_resolution_and_recovery() {
    for has_reset in [true, false] {
        let reset = has_reset.then_some(3600);
        let samples = [
            sample(None, 49.0, reset),
            sample(Some(ACCOUNT_A), 48.0, reset),
            sample(None, 47.0, reset),
            sample(Some(ACCOUNT_A), 19.0, reset),
            sample(None, 18.0, reset),
            sample(Some(ACCOUNT_A), 60.0, reset),
            sample(None, 49.0, reset),
            sample(Some(ACCOUNT_A), 48.0, reset),
            sample(None, 47.0, reset),
        ];
        assert_eq!(
            run_samples(&samples, Lane::Session),
            [50, 20, 50],
            "reset={has_reset}"
        );
    }
}

#[test]
fn independent_scopes_are_never_merged() {
    let settings = settings();
    let mut manager = NotificationManager::new();
    let scope = WarningScope::Independent("token-account:1".to_string());
    assert_eq!(
        manager.resolve_warning_account(
            ProviderId::Claude,
            &scope,
            "session",
            60.0,
            None,
            &settings
        ),
        "token-account:1"
    );
    assert!(manager.identity_gaps.last_known.is_empty());
    assert!(manager.identity_gaps.lanes.is_empty());
}

#[test]
fn disabled_notifications_leave_scope_untouched() {
    let settings = Settings {
        show_notifications: false,
        ..settings()
    };
    let mut manager = NotificationManager::new();
    let key = manager.resolve_warning_account(
        ProviderId::Claude,
        &scope_for(None),
        "session",
        60.0,
        None,
        &settings,
    );
    assert_eq!(key, CLI_UNKNOWN);
    assert!(manager.identity_gaps.lanes.is_empty());
}

#[test]
fn session_depleted_state_follows_the_merged_account() {
    let settings = settings();
    let mut manager = NotificationManager::new();
    let base = Utc.timestamp_opt(1_900_000_000, 0).unwrap();
    let reset = Some(base + chrono::Duration::hours(1));

    let unresolved = manager.resolve_warning_account(
        ProviderId::Claude,
        &scope_for(None),
        "session",
        100.0,
        reset,
        &settings,
    );
    manager.check_session_lane(ProviderId::Claude, &unresolved, 100.0, false, &settings);
    let account = manager.resolve_warning_account(
        ProviderId::Claude,
        &scope_for(Some(ACCOUNT_A)),
        "session",
        100.0,
        reset,
        &settings,
    );
    manager.check_session_lane(ProviderId::Claude, &account, 100.0, false, &settings);

    let depleted = manager
        .toasts
        .borrow()
        .iter()
        .filter(|toast| {
            toast.starts_with(&NotificationType::SessionDepleted.title(Language::English))
        })
        .count();
    assert_eq!(depleted, 1, "depleted toast must not repeat after merge");
}

#[test]
fn session_baseline_bridges_an_identity_gap_into_the_merged_account() {
    let settings = settings();
    let mut manager = NotificationManager::new();
    let base = Utc.timestamp_opt(1_900_000_000, 0).unwrap();
    let reset = Some(base + chrono::Duration::hours(1));

    // The unresolved sample is a normal (non-depleted) baseline.
    let unresolved = manager.resolve_warning_account(
        ProviderId::Claude,
        &scope_for(None),
        "session",
        50.0,
        reset,
        &settings,
    );
    manager.check_session_lane(ProviderId::Claude, &unresolved, 50.0, false, &settings);
    // Once the identity resolves, the bridged baseline must still produce the depletion edge.
    let account = manager.resolve_warning_account(
        ProviderId::Claude,
        &scope_for(Some(ACCOUNT_A)),
        "session",
        100.0,
        reset,
        &settings,
    );
    manager.check_session_lane(ProviderId::Claude, &account, 100.0, false, &settings);

    let depleted = manager
        .toasts
        .borrow()
        .iter()
        .filter(|toast| {
            toast.starts_with(&NotificationType::SessionDepleted.title(Language::English))
        })
        .count();
    assert_eq!(
        depleted, 1,
        "bridged baseline must still announce depletion"
    );
}

#[test]
fn claude_unresolved_source_fallback_keeps_the_session_owner() {
    let settings = settings();
    let mut manager = NotificationManager::new();

    manager.check_session_transition(ProviderId::Claude, "claude:oauth:unknown", 50.0, &settings);
    manager.check_session_transition(ProviderId::Claude, "claude:oauth:unknown", 100.0, &settings);
    // One CLI fallback sample keeps the same unresolved Claude owner (upstream
    // `claude-account:unknown`), so OAuth's pending depletion is not dropped.
    manager.check_session_transition(ProviderId::Claude, "claude:cli:unknown", 40.0, &settings);
    manager.check_session_transition(ProviderId::Claude, "claude:oauth:unknown", 0.0, &settings);

    let restored = manager
        .toasts
        .borrow()
        .iter()
        .filter(|toast| {
            toast.starts_with(&NotificationType::SessionRestored.title(Language::English))
        })
        .count();
    assert_eq!(
        restored, 1,
        "source fallback must not reset the session owner"
    );
}

fn toast_count(manager: &NotificationManager, notification: NotificationType) -> usize {
    let prefix = notification.title(Language::English);
    manager
        .toasts
        .borrow()
        .iter()
        .filter(|toast| toast.starts_with(&prefix))
        .count()
}

#[test]
fn claude_unresolved_sources_share_one_session_depletion() {
    let settings = settings();
    let mut manager = NotificationManager::new();

    manager.check_session_transition(ProviderId::Claude, "claude:oauth:unknown", 50.0, &settings);
    manager.check_session_transition(ProviderId::Claude, "claude:oauth:unknown", 100.0, &settings);
    assert_eq!(toast_count(&manager, NotificationType::SessionDepleted), 1);

    // The CLI fallback shares the unresolved owner, so its 100% sample is not a new edge.
    manager.check_session_transition(ProviderId::Claude, "claude:cli:unknown", 100.0, &settings);
    assert_eq!(
        toast_count(&manager, NotificationType::SessionDepleted),
        1,
        "CLI fallback must not repeat the depleted toast"
    );

    // Recovery observed on the other source still announces the restore.
    manager.check_session_transition(ProviderId::Claude, "claude:cli:unknown", 20.0, &settings);
    assert_eq!(toast_count(&manager, NotificationType::SessionRestored), 1);
}

#[test]
fn reverse_identity_gap_copy_does_not_carry_the_account_depletion() {
    let settings = settings();
    let mut manager = NotificationManager::new();
    let base = Utc.timestamp_opt(1_900_000_000, 0).unwrap();
    let reset = Some(base + chrono::Duration::hours(1));
    let next_reset = Some(base + chrono::Duration::hours(3));

    // The unresolved sample establishes a baseline, then account A resolves and absorbs it.
    let unresolved = manager.resolve_warning_account(
        ProviderId::Claude,
        &scope_for(None),
        "session",
        50.0,
        reset,
        &settings,
    );
    manager.check_session_lane(ProviderId::Claude, &unresolved, 50.0, false, &settings);
    let account = manager.resolve_warning_account(
        ProviderId::Claude,
        &scope_for(Some(ACCOUNT_A)),
        "session",
        50.0,
        reset,
        &settings,
    );
    manager.check_session_lane(ProviderId::Claude, &account, 50.0, false, &settings);
    let account = manager.resolve_warning_account(
        ProviderId::Claude,
        &scope_for(Some(ACCOUNT_A)),
        "session",
        100.0,
        reset,
        &settings,
    );
    manager.check_session_lane(ProviderId::Claude, &account, 100.0, false, &settings);
    assert_eq!(toast_count(&manager, NotificationType::SessionDepleted), 1);

    // A discontinuous unresolved sample (new reset, usage dropped) is a new owner. A's
    // depletion stays with A; the unresolved owner starts a fresh baseline.
    let unresolved = manager.resolve_warning_account(
        ProviderId::Claude,
        &scope_for(None),
        "session",
        20.0,
        next_reset,
        &settings,
    );
    assert_eq!(unresolved, CLI_UNKNOWN);
    manager.check_session_lane(ProviderId::Claude, &unresolved, 20.0, false, &settings);
    assert_eq!(
        toast_count(&manager, NotificationType::SessionRestored),
        0,
        "reverse copy must not announce a restore from A's depletion"
    );
}
