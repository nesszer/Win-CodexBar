//! Opt-in limit reset notifications (upstream CodexBar 0.74.1,
//! `LimitResetNotifications.swift` and the reset detector in
//! `UsageStore+LimitResetCelebration.swift`).
//!
//! Each refresh feeds one session and one weekly observation per provider
//! account into a detector. A detector announces a reset when usage drops from
//! above 1% to at most 1% and the reset boundary evidence allows it. A
//! notification receipt per usage cycle, plus the last announced boundary,
//! keeps one banner per reset across refreshes and restarts. While the setting
//! is on, a session "restored" toast is routed through the same detector so a
//! single banner covers a session reset.
//!
//! State is persisted under hashed account keys (no raw emails on disk) and is
//! cleared when the setting is turned off.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{NotificationManager, SessionTransitionKey, session_transition_key};
use crate::core::{ProviderId, WeeklyResetRule};
use crate::locale::{self, LocaleKey};
use crate::settings::Settings;
use crate::sound::NotificationSoundEvent;
use crate::{atomic_file, secure_file};

const LIMIT_RESET_THRESHOLD: f64 = 1.0;
const RESET_BOUNDARY_TOLERANCE_SECONDS: i64 = 2 * 60;
const DEPLETED_USED_PERCENT: f64 = 99.99;

const STORE_VERSION: u32 = 1;
const STORE_RELATIVE_PATH: &str = "notifications/limit-reset-state-v1.json";
const KEY_NAMESPACE: &str = "com.codexbar.limit-reset.v1";
const MAX_STATES_PER_WINDOW: usize = 256;
const STATE_RETENTION_DAYS: i64 = 30;

/// Whether the provider's usage history can back a reset detector.
pub fn limit_reset_supported(provider: ProviderId) -> bool {
    provider.limit_reset_policy().supported
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitResetWindow {
    Session,
    Weekly,
}

impl LimitResetWindow {
    fn notification_window(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Weekly => "weekly",
        }
    }
}

/// One usage sample of a session or weekly lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LimitResetObservation {
    pub used_percent: f64,
    pub observed_at: DateTime<Utc>,
    pub reset_boundary: Option<DateTime<Utc>>,
}

/// Which account a sample belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitResetAccount {
    /// Stable account discriminator. Hashed before it is kept or persisted.
    pub key: String,
    /// Shown in the banner unless personal info is hidden.
    pub label: Option<String>,
    /// Identity-less samples share one detector and can be transient, so a
    /// second low sample must confirm the reset.
    pub requires_low_confirmation: bool,
}

/// One provider snapshot handed to the reset detector.
#[derive(Debug, Clone)]
pub struct LimitResetSample<'a> {
    pub provider: ProviderId,
    /// `None` when the provider/account cannot be tracked; a deferred restored
    /// toast is then posted directly.
    pub account: Option<LimitResetAccount>,
    /// Account key the session transition lane used for this snapshot.
    pub session_transition_account: &'a str,
    pub session: Option<LimitResetObservation>,
    pub weekly: Option<LimitResetObservation>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct NotificationReceipt {
    #[serde(default)]
    pub(super) reset_boundary: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct DetectorState {
    pub(super) was_above_threshold: bool,
    pub(super) last_observed_at: DateTime<Utc>,
    #[serde(default)]
    pub(super) reset_boundary: Option<DateTime<Utc>>,
    #[serde(default)]
    pub(super) recovery_above_threshold_count: Option<u32>,
    #[serde(default)]
    pub(super) pending_low_confirmation: bool,
    #[serde(default)]
    pub(super) notification_receipt: Option<NotificationReceipt>,
    #[serde(default)]
    pub(super) last_notified_reset_boundary: Option<DateTime<Utc>>,
}

impl DetectorState {
    #[cfg(test)]
    pub(super) fn new(was_above_threshold: bool, last_observed_at: DateTime<Utc>) -> Self {
        Self {
            was_above_threshold,
            last_observed_at,
            reset_boundary: None,
            recovery_above_threshold_count: None,
            pending_low_confirmation: false,
            notification_receipt: None,
            last_notified_reset_boundary: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LimitResetNotice {
    Restored,
    Reset,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct TransitionInput<'a> {
    pub(super) provider: ProviderId,
    pub(super) window: LimitResetWindow,
    pub(super) observation: LimitResetObservation,
    pub(super) previous: Option<&'a DetectorState>,
    pub(super) requires_low_confirmation: bool,
}

#[derive(Debug, Clone)]
pub(super) struct Transition {
    pub(super) state: DetectorState,
    pub(super) should_post: bool,
}

fn equivalent_boundaries(lhs: DateTime<Utc>, rhs: DateTime<Utc>) -> bool {
    (lhs - rhs).num_seconds().abs() < RESET_BOUNDARY_TOLERANCE_SECONDS
}

/// Whether `current` is a later reset boundary than `previous`. A missing
/// previous boundary counts as advanced unless `requires_previous` is set; a
/// missing current boundary never does.
pub(super) fn boundary_advanced(
    previous: Option<DateTime<Utc>>,
    current: Option<DateTime<Utc>>,
    requires_previous: bool,
) -> bool {
    let Some(previous) = previous else {
        return !requires_previous;
    };
    let Some(current) = current else {
        return false;
    };
    !equivalent_boundaries(previous, current) && current > previous
}

/// Reset detector transition. Ports the upstream detector without the
/// advanced-boundary weekly delayed-candidate path and plan tracking (deferred
/// on Windows). Weekly lanes follow the provider's [`WeeklyResetRule`].
pub(super) fn detector_transition(input: TransitionInput<'_>) -> Transition {
    let observation = input.observation;
    let previous = input.previous;
    let current_used = observation.used_percent;
    let was_above = current_used > LIMIT_RESET_THRESHOLD;
    let is_session = input.window == LimitResetWindow::Session;
    let weekly_rule = if input.window == LimitResetWindow::Weekly {
        input.provider.limit_reset_policy().weekly
    } else {
        WeeklyResetRule::Standard
    };
    let recovery_observation_count = match weekly_rule {
        WeeklyResetRule::RecoveryObservations { count } => Some(count),
        _ => None,
    };
    let advanced_boundary_rule = match weekly_rule {
        WeeklyResetRule::AdvancedBoundary {
            reset_threshold_percent,
            requires_previous_boundary,
        } => Some((reset_threshold_percent, requires_previous_boundary)),
        _ => None,
    };
    let recovery_pending = recovery_observation_count.is_some()
        && previous.is_some_and(|state| state.recovery_above_threshold_count.is_some());
    let previous_boundary = previous.and_then(|state| state.reset_boundary);
    let boundary_allows_post = if is_session {
        boundary_advanced(previous_boundary, observation.reset_boundary, false)
    } else if let Some((_, requires_previous_boundary)) = advanced_boundary_rule {
        boundary_advanced(
            previous_boundary,
            observation.reset_boundary,
            requires_previous_boundary,
        )
    } else {
        true
    };
    let previous_above = previous.is_some_and(|state| state.was_above_threshold);
    let crossed_below = previous_above && !was_above;
    let confirming_low_sample =
        previous.is_some_and(|state| state.pending_low_confirmation) && !was_above;

    let should_post = if input.requires_low_confirmation {
        confirming_low_sample && !recovery_pending
    } else if let Some((reset_threshold_percent, _)) = advanced_boundary_rule {
        previous_above && current_used <= reset_threshold_percent && boundary_allows_post
    } else {
        crossed_below && boundary_allows_post && !recovery_pending
    };
    let suppressed_guarded_crossing = crossed_below && !boundary_allows_post;
    let should_await_low_confirmation = input.requires_low_confirmation
        && crossed_below
        && !confirming_low_sample
        && boundary_allows_post
        && !recovery_pending;
    // Sessions keep the last non-regressed boundary on every guarded sample;
    // an advanced-boundary weekly lane adopts a newly appearing boundary so a
    // later advance counts.
    let preserve_boundary = !boundary_allows_post && (is_session || previous_boundary.is_some());

    let previous_recovery_count = previous
        .and_then(|state| state.recovery_above_threshold_count)
        .unwrap_or(0);
    let next_recovery_count = if recovery_pending && was_above {
        previous_recovery_count + 1
    } else {
        0
    };
    let recovery_confirmed = recovery_pending
        && recovery_observation_count.is_some_and(|count| next_recovery_count >= count);

    // A transient zero must not erase the baseline needed to recognize the
    // real reset that follows.
    let next_was_above = if should_post {
        false
    } else if recovery_pending {
        recovery_confirmed
    } else if suppressed_guarded_crossing || should_await_low_confirmation {
        true
    } else {
        was_above
    };
    let recovery_above_threshold_count = if should_post {
        Some(0)
    } else if recovery_pending && !recovery_confirmed {
        Some(next_recovery_count)
    } else {
        None
    };

    Transition {
        state: DetectorState {
            was_above_threshold: next_was_above,
            last_observed_at: observation.observed_at,
            reset_boundary: if preserve_boundary {
                previous_boundary
            } else {
                observation.reset_boundary
            },
            recovery_above_threshold_count,
            pending_low_confirmation: should_await_low_confirmation,
            notification_receipt: previous.and_then(|state| state.notification_receipt),
            last_notified_reset_boundary: previous
                .and_then(|state| state.last_notified_reset_boundary),
        },
        should_post,
    }
}

/// Decide which banner (if any) a detector step produces. The receipt is
/// reserved before delivery so retries and restarts cannot announce the same
/// boundary twice.
pub(super) fn prepare_notice(
    state: &mut DetectorState,
    previous: Option<&DetectorState>,
    observation: &LimitResetObservation,
    reset_confirmed: bool,
    restored: bool,
) -> Option<LimitResetNotice> {
    let new_usage_cycle =
        state.was_above_threshold && previous.is_some_and(|state| !state.was_above_threshold);
    let restored_already_covered = state.notification_receipt.is_some_and(|receipt| {
        !boundary_advanced(receipt.reset_boundary, observation.reset_boundary, true)
    });
    let restored_notice = restored && !restored_already_covered;
    // Re-arm cycle coverage without forgetting known boundaries. Depletion also
    // starts a new restored episode even if the prior recovery never got low.
    if new_usage_cycle || observation.used_percent >= DEPLETED_USED_PERCENT {
        state.notification_receipt = None;
    }
    if let Some(boundary) = observation.reset_boundary {
        let advanced = boundary_advanced(state.last_notified_reset_boundary, Some(boundary), false);
        if state
            .notification_receipt
            .is_some_and(|receipt| receipt.reset_boundary.is_none())
        {
            // Resolve the window of an already announced cycle once its reset
            // metadata returns.
            if advanced {
                state.last_notified_reset_boundary = Some(boundary);
            }
            state.notification_receipt = Some(NotificationReceipt {
                reset_boundary: Some(boundary),
            });
            return None;
        }
        if !(advanced || (restored_notice && state.notification_receipt.is_none())) {
            if reset_confirmed || restored {
                state.notification_receipt = Some(NotificationReceipt {
                    reset_boundary: Some(boundary),
                });
            }
            return None;
        }
    } else if state.notification_receipt.is_some() {
        return None;
    }
    let notice = if restored_notice {
        LimitResetNotice::Restored
    } else if reset_confirmed {
        LimitResetNotice::Reset
    } else {
        return None;
    };
    state.notification_receipt = Some(NotificationReceipt {
        reset_boundary: observation.reset_boundary,
    });
    if let Some(boundary) = observation.reset_boundary {
        state.last_notified_reset_boundary = Some(
            state
                .last_notified_reset_boundary
                .map_or(boundary, |known| known.max(boundary)),
        );
    }
    Some(notice)
}

/// Title and body of a reset banner. The account is omitted when personal
/// info is hidden or the label is blank.
pub fn limit_reset_notification_copy(
    settings: &Settings,
    provider: ProviderId,
    window: LimitResetWindow,
    account_label: Option<&str>,
) -> (String, String) {
    let language = settings.ui_language;
    let window_label = NotificationManager::window_label(window.notification_window(), language);
    let title = locale::format_locale(
        language,
        LocaleKey::LimitResetNotificationTitle,
        &[provider.display_name(), &window_label],
    );
    let label = account_label
        .map(str::trim)
        .filter(|label| !label.is_empty() && !settings.hide_personal_info);
    let body = match label {
        Some(label) => locale::format_locale(
            language,
            LocaleKey::LimitResetNotificationBodyWithAccount,
            &[label],
        ),
        None => locale::get_text(language, LocaleKey::LimitResetNotificationBody),
    };
    (title, body)
}

fn hashed_state_key(provider: ProviderId, account_key: &str) -> String {
    let mut hasher = Sha256::new();
    for part in [KEY_NAMESPACE, provider.cli_name(), account_key] {
        hasher.update(part.as_bytes());
        hasher.update([0u8]);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LimitResetStore {
    version: u32,
    #[serde(default)]
    session: BTreeMap<String, DetectorState>,
    #[serde(default)]
    weekly: BTreeMap<String, DetectorState>,
}

impl Default for LimitResetStore {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            session: BTreeMap::new(),
            weekly: BTreeMap::new(),
        }
    }
}

impl LimitResetStore {
    fn window(&mut self, window: LimitResetWindow) -> &mut BTreeMap<String, DetectorState> {
        match window {
            LimitResetWindow::Session => &mut self.session,
            LimitResetWindow::Weekly => &mut self.weekly,
        }
    }

    fn is_empty(&self) -> bool {
        self.session.is_empty() && self.weekly.is_empty()
    }

    fn prune(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::days(STATE_RETENTION_DAYS);
        for states in [&mut self.session, &mut self.weekly] {
            states.retain(|_, state| state.last_observed_at >= cutoff);
            if states.len() > MAX_STATES_PER_WINDOW {
                let mut observed: Vec<DateTime<Utc>> = states
                    .values()
                    .map(|state| state.last_observed_at)
                    .collect();
                observed.sort_unstable_by(|a, b| b.cmp(a));
                let oldest_kept = observed[MAX_STATES_PER_WINDOW - 1];
                states.retain(|_, state| state.last_observed_at >= oldest_kept);
                // Ties at the cutoff can still exceed the cap; drop by key order.
                while states.len() > MAX_STATES_PER_WINDOW {
                    let Some(key) = states
                        .iter()
                        .find(|(_, state)| state.last_observed_at == oldest_kept)
                        .map(|(key, _)| key.clone())
                    else {
                        break;
                    };
                    states.remove(&key);
                }
            }
        }
    }
}

/// Pending disk work for the reset state, applied outside the app-state lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LimitResetPersistence {
    Write { path: PathBuf, contents: String },
    Remove { path: PathBuf },
}

impl LimitResetPersistence {
    pub fn apply(self) {
        match self {
            Self::Write { path, contents } => {
                if let Err(error) = write_atomically(&path, &contents) {
                    tracing::warn!(%error, "failed to persist limit reset notification state");
                }
            }
            Self::Remove { path } => match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    tracing::warn!(%error, "failed to remove limit reset notification state");
                }
            },
        }
    }
}

fn write_atomically(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("limit-reset-state-v1.json");
    let temp = path.with_file_name(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()));
    let result = secure_file::write_string(&temp, contents)
        .and_then(|()| atomic_file::replace_staged(&temp, path));
    if result.is_err()
        && let Err(cleanup_error) = std::fs::remove_file(&temp)
        && cleanup_error.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(%cleanup_error, "failed to clean staged limit reset state file");
    }
    result
}

/// Detector states for every tracked account, lazily loaded from disk.
#[derive(Debug)]
pub(super) struct LimitResetTracker {
    path: Option<PathBuf>,
    store: LimitResetStore,
    loaded: bool,
    dirty: bool,
}

impl LimitResetTracker {
    pub(super) fn new(config_root: Option<PathBuf>) -> Self {
        Self {
            path: config_root.map(|root| root.join(STORE_RELATIVE_PATH)),
            store: LimitResetStore::default(),
            loaded: false,
            dirty: false,
        }
    }

    #[cfg(not(test))]
    pub(super) fn default_config_root() -> Option<PathBuf> {
        dirs::config_dir().map(|root| root.join("CodexBar"))
    }

    /// Unit tests never touch the real user's state file.
    #[cfg(test)]
    pub(super) fn default_config_root() -> Option<PathBuf> {
        None
    }

    fn ensure_loaded(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let Some(path) = &self.path else {
            return;
        };
        let raw = match secure_file::read_string(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
            Err(error) => {
                tracing::warn!(%error, "failed to read limit reset notification state; starting empty");
                return;
            }
        };
        match serde_json::from_str::<LimitResetStore>(&raw) {
            Ok(store) if store.version == STORE_VERSION => self.store = store,
            Ok(store) => tracing::warn!(
                version = store.version,
                "unsupported limit reset notification state version; starting empty"
            ),
            Err(error) => tracing::warn!(
                line = error.line(),
                column = error.column(),
                "failed to decode limit reset notification state; starting empty"
            ),
        }
    }

    fn states(&mut self, window: LimitResetWindow) -> &mut BTreeMap<String, DetectorState> {
        self.ensure_loaded();
        self.store.window(window)
    }

    fn persistence_for_enabled(&mut self, now: DateTime<Utc>) -> Option<LimitResetPersistence> {
        if !self.dirty {
            return None;
        }
        self.dirty = false;
        self.store.prune(now);
        let path = self.path.clone()?;
        match serde_json::to_string_pretty(&self.store) {
            Ok(contents) => Some(LimitResetPersistence::Write { path, contents }),
            Err(error) => {
                tracing::warn!(%error, "failed to encode limit reset notification state");
                None
            }
        }
    }

    /// Forget every detector state; returns the file removal when one may exist.
    fn clear(&mut self) -> Option<LimitResetPersistence> {
        let had_state = !self.store.is_empty() || self.dirty;
        self.store = LimitResetStore::default();
        // Loaded stays true so a cleared tracker never resurrects the old file.
        self.loaded = true;
        self.dirty = false;
        let path = self.path.clone()?;
        (had_state || path.exists()).then_some(LimitResetPersistence::Remove { path })
    }
}

impl NotificationManager {
    /// A manager whose reset notification state lives under `config_root`
    /// (tests and proof homes); [`NotificationManager::new`] uses `%APPDATA%\CodexBar`.
    pub fn with_config_root(config_root: impl Into<PathBuf>) -> Self {
        let mut manager = Self::new();
        manager.limit_reset = LimitResetTracker::new(Some(config_root.into()));
        manager
    }

    /// Feed one provider snapshot into the reset detectors. Call after the
    /// session lane for the same snapshot, then finish the refresh batch with
    /// [`NotificationManager::finish_limit_reset_batch`].
    pub fn observe_limit_reset(&mut self, sample: LimitResetSample<'_>, settings: &Settings) {
        let restored_key =
            session_transition_key(sample.provider, sample.session_transition_account);
        let restored = self.deferred_restored.remove(&restored_key);
        let account = sample
            .account
            .filter(|_| settings.limit_reset_notifications_enabled)
            .filter(|_| limit_reset_supported(sample.provider));
        let Some(account) = account else {
            if restored {
                self.post_session_restored(sample.provider, settings);
            }
            return;
        };
        let state_key = hashed_state_key(sample.provider, &account.key);
        self.observe_limit_reset_window(
            sample.provider,
            LimitResetWindow::Session,
            &state_key,
            &account,
            sample.session,
            restored,
            settings,
        );
        self.observe_limit_reset_window(
            sample.provider,
            LimitResetWindow::Weekly,
            &state_key,
            &account,
            sample.weekly,
            false,
            settings,
        );
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one detector step needs the sample, account and restored arbitration"
    )]
    fn observe_limit_reset_window(
        &mut self,
        provider: ProviderId,
        window: LimitResetWindow,
        state_key: &str,
        account: &LimitResetAccount,
        observation: Option<LimitResetObservation>,
        restored: bool,
        settings: &Settings,
    ) {
        let Some(observation) = observation else {
            if restored {
                self.post_session_restored(provider, settings);
            }
            return;
        };
        let states = self.limit_reset.states(window);
        let previous = states.get(state_key).cloned();
        // The whole provider cache is re-evaluated every batch; a sample that is
        // not newer than the last one seen must not advance the detector.
        if previous
            .as_ref()
            .is_some_and(|state| observation.observed_at <= state.last_observed_at)
        {
            if restored {
                self.post_session_restored(provider, settings);
            }
            return;
        }
        let transition = detector_transition(TransitionInput {
            provider,
            window,
            observation,
            previous: previous.as_ref(),
            requires_low_confirmation: account.requires_low_confirmation,
        });
        let mut state = transition.state;
        let notice = prepare_notice(
            &mut state,
            previous.as_ref(),
            &observation,
            transition.should_post,
            restored,
        );
        states.insert(state_key.to_string(), state);
        self.limit_reset.dirty = true;
        match notice {
            Some(LimitResetNotice::Restored) => self.post_session_restored(provider, settings),
            Some(LimitResetNotice::Reset) => {
                tracing::info!(
                    provider = provider.cli_name(),
                    window = window.notification_window(),
                    "limit reset detected"
                );
                let (title, body) = limit_reset_notification_copy(
                    settings,
                    provider,
                    window,
                    account.label.as_deref(),
                );
                self.show_toast(&title, &body);
                Self::play_notification_sound(NotificationSoundEvent::SessionRestored, settings);
            }
            None => {}
        }
    }

    /// Close a refresh batch: post restored toasts no detector claimed, and
    /// return the disk work for the reset state (apply it outside any lock).
    /// With the setting off, all saved reset state is cleared.
    pub fn finish_limit_reset_batch(
        &mut self,
        settings: &Settings,
        now: DateTime<Utc>,
    ) -> Option<LimitResetPersistence> {
        let unclaimed: Vec<SessionTransitionKey> = self.deferred_restored.drain().collect();
        for (provider, _) in unclaimed {
            self.post_session_restored(provider, settings);
        }
        if settings.limit_reset_notifications_enabled {
            self.limit_reset.persistence_for_enabled(now)
        } else {
            self.limit_reset.clear()
        }
    }

    /// Forget all reset state in memory; returns the file removal to apply.
    pub fn clear_limit_reset_state(&mut self) -> Option<LimitResetPersistence> {
        self.limit_reset.clear()
    }
}

#[cfg(test)]
#[path = "limit_reset/tests.rs"]
mod tests;
