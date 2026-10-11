//! System notifications for CodexBar
//!
//! Provides Windows toast notifications for usage alerts

use crate::core::ProviderId;
use crate::core::{RateWindow, UsagePace};
use crate::locale::{self, LocaleKey};
use crate::settings::Settings;
use crate::sound::{NotificationSoundEvent, play_alert};
use chrono::{DateTime, Utc};

mod credential;
mod identity_gaps;

pub use credential::CredentialAlertPolicy;
use credential::CredentialEpisodes;
pub use identity_gaps::WarningScope;

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

/// Session-transition tracking key: provider + account identity.
type SessionTransitionKey = (ProviderId, String /* account */);

/// Notification manager
pub struct NotificationManager {
    /// Track which notifications have been sent to avoid spam
    sent_notifications: std::collections::HashSet<ThresholdKey>,
    /// Track previous session percent for depleted/restored transitions (per account)
    previous_session_percent: std::collections::HashMap<SessionTransitionKey, f64>,
    predictive_warning_keys: std::collections::HashSet<PredictiveWarningKey>,
    deepseek_pricing_period: Option<String>,
    identity_gaps: identity_gaps::IdentityGapState,
    /// Open credential-expiry episodes (in memory; reset on restart).
    credential_episodes: CredentialEpisodes,
    /// Toasts that would have been shown; tests assert on this instead of popping real toasts.
    #[cfg(test)]
    toasts: std::cell::RefCell<Vec<String>>,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            sent_notifications: std::collections::HashSet::new(),
            previous_session_percent: std::collections::HashMap::new(),
            predictive_warning_keys: std::collections::HashSet::new(),
            deepseek_pricing_period: None,
            identity_gaps: identity_gaps::IdentityGapState::default(),
            credential_episodes: CredentialEpisodes::default(),
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
            self.sent_notifications.retain(|(p, a, w, t)| {
                *p != provider || a != account || w != window || !t.is_threshold_toast()
            });
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
        if !settings.show_notifications {
            return;
        }

        const DEPLETED_THRESHOLD: f64 = 99.99; // Consider depleted at 99.99%+

        let transition_key: SessionTransitionKey = (provider, account.to_string());
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
            self.sent_notifications.insert((
                provider,
                account.to_string(),
                "session".to_string(),
                NotificationType::SessionDepleted,
            ));
        }
        // Check for restored transition: was depleted, now is not
        else if previous_percent >= DEPLETED_THRESHOLD && current_percent < DEPLETED_THRESHOLD {
            // Only notify restored if we previously sent a depleted notification
            let depleted_key = (
                provider,
                account.to_string(),
                "session".to_string(),
                NotificationType::SessionDepleted,
            );
            if self.sent_notifications.contains(&depleted_key) {
                let title = NotificationType::SessionRestored.title(settings.ui_language);
                let body = locale::format_locale(
                    settings.ui_language,
                    LocaleKey::NotificationSessionRestoredBody,
                    &[provider.display_name()],
                );
                self.show_toast(&title, &body);
                Self::play_notification_sound(NotificationSoundEvent::SessionRestored, settings);
                self.sent_notifications.remove(&depleted_key);
            }
        }

        // Update the tracked previous percent for this account
        self.previous_session_percent
            .insert(transition_key, current_percent);
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
#[cfg(all(target_os = "windows", not(test)))]
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

#[cfg(test)]
mod tests;
