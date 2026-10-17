//! System notifications for CodexBar
//!
//! Provides Windows toast notifications for usage alerts

#![allow(
    dead_code,
    reason = "notification helpers are reserved for future alert integration"
)]

use crate::core::ProviderId;
use crate::core::{RateWindow, UsagePace};
use crate::locale::{self, LocaleKey};
use crate::settings::Settings;
use crate::sound::{NotificationSoundEvent, play_alert};
use chrono::{DateTime, Utc};

mod credential;
mod identity_gaps;
mod limit_reset;

pub use credential::CredentialAlertPolicy;
use credential::CredentialEpisodes;
pub use identity_gaps::WarningScope;
pub use limit_reset::{
    LimitResetAccount, LimitResetObservation, LimitResetPersistence, LimitResetSample,
    LimitResetWindow, limit_reset_notification_copy, limit_reset_supported,
};

/// Notification types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotificationType {
    /// Usage is approaching limit (high threshold)
    HighUsage,
    /// Usage is critical (critical threshold)
    CriticalUsage,
    /// Usage limit exhausted
    Exhausted,
    /// Provider status issue
    StatusIssue,
    /// Session quota depleted (at 100% usage)
    SessionDepleted,
    /// Session quota restored (back from 100%)
    SessionRestored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PredictiveWarningWindow {
    Session,
    Weekly,
}

impl PredictiveWarningWindow {
    fn localized_label(self, language: crate::settings::Language) -> String {
        locale::get_text(
            language,
            match self {
                Self::Session => LocaleKey::ProviderSession,
                Self::Weekly => LocaleKey::ProviderWeekly,
            },
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PredictiveResetWindow {
    window_minutes: Option<u32>,
    resets_at: DateTime<Utc>,
}

impl PredictiveResetWindow {
    fn belongs_to_same_cycle(&self, other: &Self) -> bool {
        if self.window_minutes != other.window_minutes {
            return false;
        }
        let tolerance_secs = self
            .window_minutes
            .map(|minutes| i64::from(minutes) * 30)
            .unwrap_or(300)
            .max(300);
        (self.resets_at - other.resets_at).num_seconds().abs() < tolerance_secs
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PredictiveWarningKey {
    provider: ProviderId,
    identity: String,
    window: PredictiveWarningWindow,
    reset: PredictiveResetWindow,
}

impl NotificationType {
    pub fn title(&self, language: crate::settings::Language) -> String {
        locale::get_text(
            language,
            match self {
                NotificationType::HighUsage => LocaleKey::NotificationHighUsageTitle,
                NotificationType::CriticalUsage => LocaleKey::NotificationCriticalUsageTitle,
                NotificationType::Exhausted => LocaleKey::NotificationExhaustedTitle,
                NotificationType::StatusIssue => LocaleKey::NotificationStatusIssueTitle,
                NotificationType::SessionDepleted => LocaleKey::NotificationSessionDepletedTitle,
                NotificationType::SessionRestored => LocaleKey::NotificationSessionRestoredTitle,
            },
        )
    }

    pub fn icon(&self) -> &'static str {
        match self {
            NotificationType::HighUsage => "⚠️",
            NotificationType::CriticalUsage => "🔴",
            NotificationType::Exhausted => "🚫",
            NotificationType::StatusIssue => "⚡",
            NotificationType::SessionDepleted => "🔴",
            NotificationType::SessionRestored => "✅",
        }
    }

    fn is_threshold_toast(self) -> bool {
        matches!(
            self,
            NotificationType::HighUsage
                | NotificationType::CriticalUsage
                | NotificationType::Exhausted
        )
    }
}

/// Dedupe identity for threshold toasts.
/// - `account`: stable per-account discriminator (email, token-account id, …).
///   Empty string is the legacy single-account lane.
/// - `window`: rate window id (`"session"`, `"weekly"`, …) so budgets arm independently.
type ThresholdKey = (
    ProviderId,
    String, /* account */
    String, /* window */
    NotificationType,
);

/// Session-transition tracking key: provider + session owner (see
/// `identity_gaps::session_owner_of`). Threshold keys stay per account.
type SessionTransitionKey = (ProviderId, String /* session owner */);

/// Session-transition key for a sample's account, normalized to its session owner.
fn session_transition_key(provider: ProviderId, account: &str) -> SessionTransitionKey {
    (
        provider,
        identity_gaps::session_owner_of(provider, account).to_string(),
    )
}

/// Depleted marker for a session owner. Shares `sent_notifications` with threshold keys.
fn session_depleted_key(provider: ProviderId, owner: &str) -> ThresholdKey {
    (
        provider,
        owner.to_string(),
        "session".to_string(),
        NotificationType::SessionDepleted,
    )
}

/// Notification manager
pub struct NotificationManager {
    /// Track which notifications have been sent to avoid spam
    sent_notifications: std::collections::HashSet<ThresholdKey>,
    /// Track previous session percent for depleted/restored transitions (per account)
    previous_session_percent: std::collections::HashMap<SessionTransitionKey, f64>,
    /// Account that produced the last session transition sample per provider. A change
    /// (including missing identity `""` versus a named account) starts a fresh baseline.
    session_owner: std::collections::HashMap<ProviderId, String>,
    predictive_warning_keys: std::collections::HashSet<PredictiveWarningKey>,
    deepseek_pricing_period: Option<String>,
    identity_gaps: identity_gaps::IdentityGapState,
    /// Open credential-expiry episodes (in memory; reset on restart).
    credential_episodes: CredentialEpisodes,
    /// Session restored toasts held for the reset detector this batch (only
    /// while limit reset notifications are on).
    deferred_restored: std::collections::HashSet<SessionTransitionKey>,
    limit_reset: limit_reset::LimitResetTracker,
    /// Toasts that would have been shown; tests assert on this instead of popping real toasts.
    #[cfg(test)]
    toasts: std::cell::RefCell<Vec<String>>,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            sent_notifications: std::collections::HashSet::new(),
            previous_session_percent: std::collections::HashMap::new(),
            session_owner: std::collections::HashMap::new(),
            predictive_warning_keys: std::collections::HashSet::new(),
            deepseek_pricing_period: None,
            identity_gaps: identity_gaps::IdentityGapState::default(),
            credential_episodes: CredentialEpisodes::default(),
            deferred_restored: std::collections::HashSet::new(),
            limit_reset: limit_reset::LimitResetTracker::new(
                limit_reset::LimitResetTracker::default_config_root(),
            ),
            #[cfg(test)]
            toasts: std::cell::RefCell::new(Vec::new()),
        }
    }

    /// Observe a DeepSeek pricing period and notify once per observed transition.
    /// Intentionally silent; this advisory must not play a notification sound.
    pub fn notify_pricing_transition(&mut self, period: &str, settings: &Settings) {
        let changed = self
            .deepseek_pricing_period
            .as_deref()
            .is_some_and(|previous| previous != period);
        self.deepseek_pricing_period = Some(period.to_string());
        if !settings.show_notifications || !changed {
            return;
        }
        let label = match period {
            "peak" => "peak",
            "offPeak" => "off-peak",
            _ => "standard/pre-schedule",
        };
        self.show_toast(
            "DeepSeek pricing schedule",
            &format!("DeepSeek is currently in {label} hours."),
        );
    }

    pub fn record_predictive_observation(
        &mut self,
        enabled: bool,
        provider: ProviderId,
        identity: &str,
        window: PredictiveWarningWindow,
        rate_window: &RateWindow,
        pace: &UsagePace,
    ) -> bool {
        if !enabled {
            self.predictive_warning_keys
                .retain(|key| key.provider != provider);
            return false;
        }
        if !matches!(provider, ProviderId::Claude | ProviderId::Codex) || identity.is_empty() {
            return false;
        }
        let Some(resets_at) = rate_window.resets_at else {
            return false;
        };
        let key = PredictiveWarningKey {
            provider,
            identity: identity.to_string(),
            window,
            reset: PredictiveResetWindow {
                window_minutes: rate_window.window_minutes,
                resets_at,
            },
        };

        let warned_this_cycle = self.predictive_warning_keys.iter().any(|existing| {
            existing.provider == key.provider
                && existing.identity == key.identity
                && existing.window == key.window
                && existing.reset.belongs_to_same_cycle(&key.reset)
        });
        self.predictive_warning_keys.retain(|existing| {
            existing.provider != key.provider
                || existing.identity != key.identity
                || existing.window != key.window
        });

        if pace.will_last_to_reset {
            return false;
        }
        if !pace
            .eta_seconds
            .is_some_and(|eta| eta.is_finite() && eta > 0.0)
        {
            return false;
        }

        self.predictive_warning_keys.insert(key);
        !warned_this_cycle
    }

    pub fn set_predictive_warnings_enabled(&mut self, provider: ProviderId, enabled: bool) {
        if !enabled {
            self.predictive_warning_keys
                .retain(|key| key.provider != provider);
        }
    }

    pub fn check_predictive_pace(
        &mut self,
        provider: ProviderId,
        identity: &str,
        window: PredictiveWarningWindow,
        rate_window: &RateWindow,
        pace: &UsagePace,
        settings: &Settings,
    ) {
        if !self.record_predictive_observation(
            settings.show_notifications && settings.predictive_pace_warning_enabled,
            provider,
            identity,
            window,
            rate_window,
            pace,
        ) {
            return;
        }

        let eta = format_duration(pace.eta_seconds.unwrap_or_default());
        let provider_name = provider.display_name();
        let window_label = window.localized_label(settings.ui_language);
        let title = locale::format_locale(
            settings.ui_language,
            LocaleKey::PredictivePaceWarningTitle,
            &[provider_name, &window_label],
        );
        let body = locale::format_locale(
            settings.ui_language,
            LocaleKey::PredictivePaceWarningBody,
            &[&eta],
        );
        self.show_toast(&title, &body);
        Self::play_notification_sound(NotificationSoundEvent::PredictiveWarning, settings);
    }

    /// Check usage and send notifications if thresholds are crossed.
    ///
    /// `account` is a stable account discriminator (email, token-account id, …).
    /// Pass `""` for single-account providers when no identity is available.
    pub fn check_and_notify(
        &mut self,
        provider: ProviderId,
        account: &str,
        window: &str,
        used_percent: f64,
        settings: &Settings,
    ) {
        if !settings.quota_warning_enabled(provider, window) {
            // Quota warnings set to Off for this provider window: forget its
            // sent state so turning them back on starts fresh (Mac parity).
            self.clear_threshold_toasts(provider, account, window);
            return;
        }
        if !settings.show_notifications {
            return;
        }

        let thresholds = settings.usage_thresholds(provider, window);
        let notification_type = if used_percent >= 100.0 {
            Some(NotificationType::Exhausted)
        } else if used_percent >= thresholds.critical {
            Some(NotificationType::CriticalUsage)
        } else if used_percent >= thresholds.high {
            Some(NotificationType::HighUsage)
        } else {
            // Clear only this provider+account+window's threshold toasts so a cool
            // session on one account cannot re-arm another account's weekly (or
            // another window on the same account) on the next poll.
            self.clear_threshold_toasts(provider, account, window);
            None
        };

        if let Some(notif_type) = notification_type {
            let key = (
                provider,
                account.to_string(),
                window.to_string(),
                notif_type,
            );
            if !self.sent_notifications.contains(&key) {
                self.send_notification(provider, window, used_percent, notif_type, settings);
                self.sent_notifications.insert(key);
            }
        }
    }

    fn clear_threshold_toasts(&mut self, provider: ProviderId, account: &str, window: &str) {
        self.sent_notifications.retain(|(p, a, w, t)| {
            *p != provider || a != account || w != window || !t.is_threshold_toast()
        });
    }

    /// Observe a session lane across all session consumers. Informational
    /// placeholders represent missing data and must not clear or re-arm
    /// threshold, transition, or hook state in the caller.
    pub fn check_session_lane(
        &mut self,
        provider: ProviderId,
        account: &str,
        used_percent: f64,
        is_informational: bool,
        settings: &Settings,
    ) -> bool {
        if is_informational {
            return false;
        }

        self.check_and_notify(provider, account, "session", used_percent, settings);
        self.check_session_transition(provider, account, used_percent, settings);
        true
    }

    /// Send a notification for a status issue
    pub fn notify_status_issue(
        &mut self,
        provider: ProviderId,
        description: &str,
        settings: &Settings,
    ) {
        let key = (
            provider,
            String::new(),
            String::new(),
            NotificationType::StatusIssue,
        );
        if !self.sent_notifications.contains(&key) {
            self.send_status_notification(provider, description, settings);
            self.sent_notifications.insert(key);
        }
    }

    /// Clear status issue notification (when resolved)
    pub fn clear_status_issue(&mut self, provider: ProviderId) {
        self.sent_notifications.remove(&(
            provider,
            String::new(),
            String::new(),
            NotificationType::StatusIssue,
        ));
    }

    /// Check session quota transitions (depleted/restored)
    /// Call this with each usage update to detect transitions.
    ///
    /// `account` scopes depleted/restored state so multi-account providers do not
    /// cross-arm session transitions.
    pub fn check_session_transition(
        &mut self,
        provider: ProviderId,
        account: &str,
        current_percent: f64,
        settings: &Settings,
    ) {
        const DEPLETED_THRESHOLD: f64 = 99.99; // Consider depleted at 99.99%+

        // Upstream `ownerChanged`: a different account (or missing identity versus a named
        // one) re-baselines and drops every session edge for this provider. Recovery seen
        // while another account owned the lane must not surface as a restore on return.
        // Owner tracking runs even while session alerts are off, so a switch made while
        // they are off cannot leave a depletion for the next owner to restore.
        let transition_key = session_transition_key(provider, account);
        let owner_changed = self.observe_session_owner(provider, account);
        if owner_changed {
            self.reset_session_transition_state(provider);
            self.previous_session_percent
                .insert(transition_key, current_percent);
            return;
        }
        if !settings.session_quota_notifications_enabled {
            return;
        }

        let previous_percent = self
            .previous_session_percent
            .get(&transition_key)
            .copied()
            .unwrap_or(0.0);

        // Check for depleted transition: was not depleted, now is
        if previous_percent < DEPLETED_THRESHOLD && current_percent >= DEPLETED_THRESHOLD {
            let title = NotificationType::SessionDepleted.title(settings.ui_language);
            let body = locale::format_locale(
                settings.ui_language,
                LocaleKey::NotificationSessionDepletedBody,
                &[provider.display_name()],
            );
            self.show_toast(&title, &body);
            Self::play_notification_sound(NotificationSoundEvent::SessionDepleted, settings);
            self.sent_notifications
                .insert(session_depleted_key(provider, &transition_key.1));
        }
        // Check for restored transition: was depleted, now is not
        else if previous_percent >= DEPLETED_THRESHOLD && current_percent < DEPLETED_THRESHOLD {
            // Only notify restored if we previously sent a depleted notification
            let depleted_key = session_depleted_key(provider, &transition_key.1);
            if self.sent_notifications.contains(&depleted_key) {
                if settings.limit_reset_notifications_enabled {
                    // The reset detector decides whether this restored toast or a
                    // reset banner announces the session reset.
                    self.deferred_restored.insert(transition_key.clone());
                } else {
                    self.post_session_restored(provider, settings);
                }
                self.sent_notifications.remove(&depleted_key);
            }
        }

        // Update the tracked previous percent for this session owner
        self.previous_session_percent
            .insert(transition_key, current_percent);
    }

    /// Record the session owner for `account` and report whether it differs from the
    /// previous sample's owner for this provider.
    fn observe_session_owner(&mut self, provider: ProviderId, account: &str) -> bool {
        let owner = identity_gaps::session_owner_of(provider, account);
        self.session_owner
            .insert(provider, owner.to_string())
            .is_some_and(|previous| previous != owner)
    }

    /// Forget every session edge for `provider` (all accounts): the baseline, pending
    /// depleted flags, and deferred restored confirmations. Threshold toasts are kept.
    fn reset_session_transition_state(&mut self, provider: ProviderId) {
        self.previous_session_percent
            .retain(|(p, _), _| *p != provider);
        self.sent_notifications.retain(|(p, _, w, t)| {
            *p != provider || w != "session" || *t != NotificationType::SessionDepleted
        });
        self.deferred_restored.retain(|(p, _)| *p != provider);
    }

    fn post_session_restored(&self, provider: ProviderId, settings: &Settings) {
        let title = NotificationType::SessionRestored.title(settings.ui_language);
        let body = locale::format_locale(
            settings.ui_language,
            LocaleKey::NotificationSessionRestoredBody,
            &[provider.display_name()],
        );
        self.show_toast(&title, &body);
        Self::play_notification_sound(NotificationSoundEvent::SessionRestored, settings);
    }

    /// Send a Windows toast notification with sound
    fn send_notification(
        &self,
        provider: ProviderId,
        window: &str,
        used_percent: f64,
        notif_type: NotificationType,
        settings: &Settings,
    ) {
        let title = notif_type.title(settings.ui_language);
        let body = Self::notification_body(
            provider,
            window,
            used_percent,
            notif_type,
            settings.ui_language,
        );
        self.show_toast(&title, &body);
        Self::play_notification_sound(Self::sound_event_for(notif_type), settings);
    }

    fn window_label(window: &str, language: crate::settings::Language) -> String {
        let key = match window {
            "session" => LocaleKey::NotificationWindowSession,
            "weekly" => LocaleKey::NotificationWindowWeekly,
            other if !other.is_empty() => return other.to_string(),
            _ => LocaleKey::NotificationWindowUsage,
        };
        locale::get_text(language, key)
    }

    fn notification_body(
        provider: ProviderId,
        window: &str,
        used_percent: f64,
        notif_type: NotificationType,
        language: crate::settings::Language,
    ) -> String {
        let provider_name = provider.display_name();
        let window_label = Self::window_label(window, language);
        let percent = format!("{used_percent:.0}");
        let usage_args = [provider_name, window_label.as_str(), percent.as_str()];
        let provider_args = [provider_name];
        let (key, args): (LocaleKey, &[&str]) = match notif_type {
            NotificationType::HighUsage => (LocaleKey::NotificationHighUsageBody, &usage_args),
            NotificationType::CriticalUsage => {
                (LocaleKey::NotificationCriticalUsageBody, &usage_args)
            }
            NotificationType::Exhausted => (LocaleKey::NotificationExhaustedBody, &usage_args),
            NotificationType::StatusIssue => {
                (LocaleKey::NotificationStatusIssueBody, &provider_args)
            }
            NotificationType::SessionDepleted => {
                (LocaleKey::NotificationSessionDepletedBody, &provider_args)
            }
            NotificationType::SessionRestored => {
                (LocaleKey::NotificationSessionRestoredBody, &provider_args)
            }
        };
        locale::format_locale(language, key, args)
    }

    fn sound_event_for(notif_type: NotificationType) -> NotificationSoundEvent {
        match notif_type {
            NotificationType::HighUsage => NotificationSoundEvent::HighUsage,
            NotificationType::CriticalUsage => NotificationSoundEvent::CriticalUsage,
            NotificationType::Exhausted => NotificationSoundEvent::Exhausted,
            NotificationType::StatusIssue => NotificationSoundEvent::StatusIssue,
            NotificationType::SessionDepleted => NotificationSoundEvent::SessionDepleted,
            NotificationType::SessionRestored => NotificationSoundEvent::SessionRestored,
        }
    }

    fn play_notification_sound(event: NotificationSoundEvent, settings: &Settings) {
        if let Err(error) = play_alert(event, settings) {
            tracing::warn!(?event, %error, "notification sound failed to play");
        }
    }

    fn send_status_notification(
        &self,
        provider: ProviderId,
        description: &str,
        settings: &Settings,
    ) {
        let title = NotificationType::StatusIssue.title(settings.ui_language);
        let body = format!("{}: {}", provider.display_name(), description);
        self.show_toast(&title, &body);
        Self::play_notification_sound(NotificationSoundEvent::StatusIssue, settings);
    }

    /// Tests record the toast and report it as handed to the OS.
    #[cfg(test)]
    fn show_toast(&self, title: &str, body: &str) -> bool {
        self.toasts.borrow_mut().push(format!("{title}: {body}"));
        true
    }

    /// Returns whether the toast was handed to the OS (a spawn failure is the
    /// only failure the fire-and-forget PowerShell dispatch can observe).
    #[cfg(all(target_os = "windows", not(test)))]
    fn show_toast(&self, title: &str, body: &str) -> bool {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        use std::sync::Once;

        // Register our AUMID (App User Model ID) exactly once per process so that
        // CreateToastNotifier("CodexBar") finds a valid registration rather than
        // silently returning a null notifier.
        static AUMID_INIT: Once = Once::new();
        AUMID_INIT.call_once(ensure_aumid_registered);

        // Escape for XML content to prevent injection
        fn xml_escape(s: &str) -> String {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
                .replace('\'', "&apos;")
        }

        let safe_title = xml_escape(title);
        let safe_body = xml_escape(body);

        // Uses ToastGeneric (Win 10+) and wraps in try/catch so PowerShell exits
        // with code 1 on failure rather than swallowing the error silently.
        // Single-quoted here-string (@'...'@) prevents variable expansion of the
        // XML content by PowerShell.
        let script = format!(
            r#"try {{
    [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
    [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null
    $template = @'
<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual><audio silent="true"/></toast>
'@
    $xml = New-Object Windows.Data.Xml.Dom.XmlDocument
    $xml.LoadXml($template)
    $toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
    $notifier = [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier("CodexBar")
    if ($null -eq $notifier) {{ throw "CreateToastNotifier returned null" }}
    $notifier.Show($toast)
}} catch {{
    [System.Console]::Error.WriteLine("CodexBar toast failed: $_")
    exit 1
}}"#,
            safe_title, safe_body
        );

        match Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                &script,
            ])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .spawn()
        {
            Ok(_) => {
                tracing::debug!("Toast notification dispatched: {}", title);
                true
            }
            Err(e) => {
                tracing::warn!("Failed to dispatch toast notification '{}': {}", title, e);
                false
            }
        }
    }

    #[cfg(all(not(target_os = "windows"), not(test)))]
    fn show_toast(&self, title: &str, body: &str) -> bool {
        use std::process::Command;

        // Try notify-send first (works on most Linux distros including WSL with WSLg)
        if let Ok(output) = Command::new("notify-send")
            .args([
                "--app-name=CodexBar",
                "--icon=dialog-information",
                title,
                body,
            ])
            .output()
            && output.status.success()
        {
            tracing::debug!("Sent notification via notify-send: {}", title);
            return true;
        }

        tracing::info!("Notification: {} - {}", title, body);
        false
    }
}

fn format_duration(seconds: f64) -> String {
    // Display-only duration label; the value is ceiled to whole minutes and
    // clamped to at least 1, so only astronomically large inputs could truncate.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "whole-minutes display label; ceil/max bound the value to realistic durations"
    )]
    let total_minutes = (seconds / 60.0).ceil().max(1.0) as i64;
    let days = total_minutes / 1440;
    let hours = (total_minutes % 1440) / 60;
    let minutes = total_minutes % 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Register the CodexBar App User Model ID (AUMID) in the Windows registry so that
/// `CreateToastNotifier("CodexBar")` resolves to a valid notifier instead of returning
/// null.  Must be called at least once before the first toast.  Safe to call multiple
/// times (idempotent registry write).
#[cfg(target_os = "windows")]
fn ensure_aumid_registered() {
    use winreg::RegKey;
    use winreg::enums::*;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    // HKCU\SOFTWARE\Classes\AppUserModelId\<AUMID> is the documented path for
    // registering Win32 desktop app AUMIDs without a COM server or Start Menu shortcut.
    let result = hkcu
        .create_subkey(r"SOFTWARE\Classes\AppUserModelId\CodexBar")
        .and_then(|(key, _)| key.set_value("DisplayName", &"CodexBar"));

    match result {
        Ok(()) => tracing::debug!("CodexBar AUMID registered for Windows toast notifications"),
        Err(e) => tracing::warn!("Failed to register CodexBar AUMID: {}", e),
    }
}

/// Simple notification function for one-off notifications
pub fn show_notification(title: &str, body: &str) {
    let manager = NotificationManager::new();
    manager.show_toast(title, body);
}

#[cfg(test)]
mod tests {
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
    fn quota_warnings_off_suppresses_threshold_toasts_for_that_window_only() {
        use crate::settings::{ProviderQuotaWarnings, QuotaWarningWindow};
        let mut manager = NotificationManager::new();
        let mut settings = Settings::default();
        let account = "";
        let key = |window: &str, kind| {
            (
                ProviderId::Claude,
                account.to_string(),
                window.to_string(),
                kind,
            )
        };

        manager.check_and_notify(ProviderId::Claude, account, "session", 80.0, &settings);
        assert!(
            manager
                .sent_notifications
                .contains(&key("session", NotificationType::HighUsage))
        );

        settings.set_quota_warnings(
            ProviderId::Claude,
            ProviderQuotaWarnings {
                session: QuotaWarningWindow {
                    enabled: Some(false),
                    high: None,
                    critical: None,
                },
                weekly: QuotaWarningWindow::default(),
            },
        );
        manager.check_and_notify(ProviderId::Claude, account, "session", 95.0, &settings);
        manager.check_and_notify(ProviderId::Claude, account, "weekly", 80.0, &settings);
        assert!(
            !manager
                .sent_notifications
                .iter()
                .any(|(_, _, window, _)| window == "session"),
            "Off clears and suppresses the session window"
        );
        assert!(
            manager
                .sent_notifications
                .contains(&key("weekly", NotificationType::HighUsage)),
            "the Global weekly window still warns"
        );
    }

    #[test]
    fn custom_quota_warning_threshold_decides_when_a_toast_fires() {
        use crate::settings::{ProviderQuotaWarnings, QuotaWarningWindow};
        let mut manager = NotificationManager::new();
        let mut settings = Settings::default();
        settings.set_quota_warnings(
            ProviderId::Codex,
            ProviderQuotaWarnings {
                session: QuotaWarningWindow {
                    enabled: None,
                    high: Some(40.0),
                    critical: Some(60.0),
                },
                weekly: QuotaWarningWindow::default(),
            },
        );
        manager.check_and_notify(ProviderId::Codex, "", "session", 45.0, &settings);
        assert!(manager.sent_notifications.contains(&(
            ProviderId::Codex,
            String::new(),
            "session".to_string(),
            NotificationType::HighUsage,
        )));
        // 45% is below the global 70% high threshold, so weekly stays quiet.
        manager.check_and_notify(ProviderId::Codex, "", "weekly", 45.0, &settings);
        assert!(
            !manager
                .sent_notifications
                .iter()
                .any(|(_, _, window, _)| window == "weekly")
        );
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

        // Account B can still fire depleted once it is the active owner again.
        manager.check_session_transition(ProviderId::Claude, "account-b", 40.0, &settings);
        manager.check_session_transition(ProviderId::Claude, "account-b", 100.0, &settings);
        assert!(manager.sent_notifications.contains(&depleted_b));
    }

    fn toast_count(manager: &NotificationManager, notification: NotificationType) -> usize {
        let prefix = notification.title(crate::settings::Language::English);
        manager
            .toasts
            .borrow()
            .iter()
            .filter(|toast| toast.starts_with(&prefix))
            .count()
    }

    #[test]
    fn session_owner_change_starts_a_fresh_baseline() {
        let mut manager = NotificationManager::new();
        let settings = Settings::default();

        manager.check_session_transition(ProviderId::Claude, "account-a", 50.0, &settings);
        // Switching to an account already depleted is a new baseline, not a depleted edge.
        manager.check_session_transition(ProviderId::Claude, "account-b", 100.0, &settings);
        assert_eq!(
            toast_count(&manager, NotificationType::SessionDepleted),
            0,
            "owner change must not fire depleted"
        );

        // The new owner's later samples produce normal edges again.
        manager.check_session_transition(ProviderId::Claude, "account-b", 40.0, &settings);
        manager.check_session_transition(ProviderId::Claude, "account-b", 100.0, &settings);
        assert_eq!(toast_count(&manager, NotificationType::SessionDepleted), 1);
    }

    #[test]
    fn session_switching_back_does_not_restore_a_stale_depletion() {
        let mut manager = NotificationManager::new();
        let settings = Settings::default();
        let depleted_a = (
            ProviderId::Claude,
            "account-a".to_string(),
            "session".to_string(),
            NotificationType::SessionDepleted,
        );

        manager.check_session_transition(ProviderId::Claude, "account-a", 50.0, &settings);
        manager.check_session_transition(ProviderId::Claude, "account-a", 100.0, &settings);
        assert_eq!(toast_count(&manager, NotificationType::SessionDepleted), 1);

        // A -> B -> A: the recovery happened while B was the owner, so A's
        // return must rebaseline instead of announcing a restore.
        manager.check_session_transition(ProviderId::Claude, "account-b", 40.0, &settings);
        manager.check_session_transition(ProviderId::Claude, "account-a", 20.0, &settings);
        assert_eq!(toast_count(&manager, NotificationType::SessionRestored), 0);
        assert!(!manager.sent_notifications.contains(&depleted_a));
    }

    #[test]
    fn session_owner_switch_while_alerts_are_off_still_rebaselines() {
        let mut manager = NotificationManager::new();
        let on = Settings::default();
        let off = Settings {
            session_quota_notifications_enabled: false,
            ..Settings::default()
        };

        manager.check_session_transition(ProviderId::Claude, "account-a", 50.0, &on);
        manager.check_session_transition(ProviderId::Claude, "account-a", 100.0, &on);
        assert_eq!(toast_count(&manager, NotificationType::SessionDepleted), 1);

        // Owner B arrives while alerts are off: silent, but it still re-baselines.
        manager.check_session_transition(ProviderId::Claude, "account-b", 100.0, &off);
        assert_eq!(toast_count(&manager, NotificationType::SessionDepleted), 1);

        // A returns with alerts on at 20%. A's recovery happened while B owned the
        // lane, so A's stale depletion must not surface as a restore.
        manager.check_session_transition(ProviderId::Claude, "account-a", 20.0, &on);
        assert_eq!(toast_count(&manager, NotificationType::SessionRestored), 0);
        assert!(!manager.sent_notifications.contains(&(
            ProviderId::Claude,
            "account-a".to_string(),
            "session".to_string(),
            NotificationType::SessionDepleted,
        )));
    }

    #[test]
    fn session_missing_identity_is_its_own_owner() {
        let mut manager = NotificationManager::new();
        let settings = Settings::default();

        // Missing identity ("") repeats like a nil discriminator: same owner, normal edges.
        manager.check_session_transition(ProviderId::Claude, "", 50.0, &settings);
        manager.check_session_transition(ProviderId::Claude, "", 100.0, &settings);
        assert_eq!(toast_count(&manager, NotificationType::SessionDepleted), 1);

        // Missing -> named and named -> missing are owner changes (nil != value).
        manager.check_session_transition(ProviderId::Claude, "account-a", 40.0, &settings);
        manager.check_session_transition(ProviderId::Claude, "", 0.0, &settings);
        assert_eq!(
            toast_count(&manager, NotificationType::SessionRestored),
            0,
            "missing identity must not restore a depletion from before the switch"
        );
    }

    #[test]
    fn session_transition_follows_its_own_setting_not_threshold_warnings() {
        let depleted = (
            ProviderId::Codex,
            "account".to_string(),
            "session".to_string(),
            NotificationType::SessionDepleted,
        );

        // Threshold warnings off must not silence depleted/restored alerts.
        let mut manager = NotificationManager::new();
        let settings = Settings {
            show_notifications: false,
            session_quota_notifications_enabled: true,
            ..Settings::default()
        };
        manager.check_session_transition(ProviderId::Codex, "account", 40.0, &settings);
        manager.check_session_transition(ProviderId::Codex, "account", 100.0, &settings);
        assert!(manager.sent_notifications.contains(&depleted));

        // Turning the session-quota alerts off silences them even with warnings on.
        let mut manager = NotificationManager::new();
        let settings = Settings {
            show_notifications: true,
            session_quota_notifications_enabled: false,
            ..Settings::default()
        };
        manager.check_session_transition(ProviderId::Codex, "account", 40.0, &settings);
        manager.check_session_transition(ProviderId::Codex, "account", 100.0, &settings);
        assert!(!manager.sent_notifications.contains(&depleted));
    }
}
