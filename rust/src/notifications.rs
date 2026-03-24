//! System notifications for CodexBar
//!
//! Provides Windows toast notifications for usage alerts

#![allow(dead_code)]

use crate::core::ProviderId;
use crate::settings::Settings;
use crate::sound::{play_alert, AlertSound};

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

impl NotificationType {
    pub fn title(&self) -> &'static str {
        match self {
            NotificationType::HighUsage => "High Usage Warning",
            NotificationType::CriticalUsage => "Critical Usage Alert",
            NotificationType::Exhausted => "Usage Limit Reached",
            NotificationType::StatusIssue => "Provider Status Issue",
            NotificationType::SessionDepleted => "Session Depleted",
            NotificationType::SessionRestored => "Session Restored",
        }
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
}

/// Notification manager
pub struct NotificationManager {
    /// Track which notifications have been sent to avoid spam
    sent_notifications: std::collections::HashSet<(ProviderId, NotificationType)>,
    /// Track previous session percent for depleted/restored transitions
    previous_session_percent: std::collections::HashMap<ProviderId, f64>,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            sent_notifications: std::collections::HashSet::new(),
            previous_session_percent: std::collections::HashMap::new(),
        }
    }

    /// Check usage and send notifications if thresholds are crossed
    pub fn check_and_notify(
        &mut self,
        provider: ProviderId,
        used_percent: f64,
        settings: &Settings,
    ) {
        if !settings.show_notifications {
            return;
        }

        let notification_type = if used_percent >= 100.0 {
            Some(NotificationType::Exhausted)
        } else if used_percent >= settings.critical_usage_threshold {
            Some(NotificationType::CriticalUsage)
        } else if used_percent >= settings.high_usage_threshold {
            Some(NotificationType::HighUsage)
        } else {
            // Reset notifications if usage dropped
            self.sent_notifications.retain(|(p, _)| *p != provider);
            None
        };

        if let Some(notif_type) = notification_type {
            let key = (provider, notif_type);
            if !self.sent_notifications.contains(&key) {
                self.send_notification(provider, used_percent, notif_type, settings);
                self.sent_notifications.insert(key);
            }
        }
    }

    /// Send a notification for a status issue
    pub fn notify_status_issue(
        &mut self,
        provider: ProviderId,
        description: &str,
        settings: &Settings,
    ) {
        let key = (provider, NotificationType::StatusIssue);
        if !self.sent_notifications.contains(&key) {
            self.send_status_notification(provider, description, settings);
            self.sent_notifications.insert(key);
        }
    }

    /// Clear status issue notification (when resolved)
    pub fn clear_status_issue(&mut self, provider: ProviderId) {
        self.sent_notifications
            .remove(&(provider, NotificationType::StatusIssue));
    }

    /// Check session quota transitions (depleted/restored)
    /// Call this with each usage update to detect transitions
    pub fn check_session_transition(
        &mut self,
        provider: ProviderId,
        current_percent: f64,
        settings: &Settings,
    ) {
        if !settings.show_notifications {
            return;
        }

        const DEPLETED_THRESHOLD: f64 = 99.99; // Consider depleted at 99.99%+

        let previous_percent = self
            .previous_session_percent
            .get(&provider)
            .copied()
            .unwrap_or(0.0);

        // Check for depleted transition: was not depleted, now is
        if previous_percent < DEPLETED_THRESHOLD && current_percent >= DEPLETED_THRESHOLD {
            let title = NotificationType::SessionDepleted.title();
            let body = format!(
                "{} session depleted. 0% left. Will notify when available again.",
                provider.display_name()
            );
            self.show_toast(title, &body);
            play_alert(AlertSound::Error, settings);
            self.sent_notifications
                .insert((provider, NotificationType::SessionDepleted));
        }
        // Check for restored transition: was depleted, now is not
        else if previous_percent >= DEPLETED_THRESHOLD && current_percent < DEPLETED_THRESHOLD {
            // Only notify restored if we previously sent a depleted notification
            if self
                .sent_notifications
                .contains(&(provider, NotificationType::SessionDepleted))
            {
                let title = NotificationType::SessionRestored.title();
                let body = format!(
                    "{} session restored. Session quota is available again.",
                    provider.display_name()
                );
                self.show_toast(title, &body);
                play_alert(AlertSound::Success, settings);
                self.sent_notifications
                    .remove(&(provider, NotificationType::SessionDepleted));
            }
        }

        // Update the tracked previous percent
        self.previous_session_percent
            .insert(provider, current_percent);
    }

    /// Send a Windows toast notification with sound
    fn send_notification(
        &self,
        provider: ProviderId,
        used_percent: f64,
        notif_type: NotificationType,
        settings: &Settings,
    ) {
        let title = notif_type.title();
        let body = match notif_type {
            NotificationType::HighUsage => {
                format!(
                    "{} usage at {:.0}% - approaching limit",
                    provider.display_name(),
                    used_percent
                )
            }
            NotificationType::CriticalUsage => {
                format!(
                    "{} usage at {:.0}% - critically high!",
                    provider.display_name(),
                    used_percent
                )
            }
            NotificationType::Exhausted => {
                format!(
                    "{} usage limit exhausted ({:.0}%)",
                    provider.display_name(),
                    used_percent
                )
            }
            NotificationType::StatusIssue => {
                format!("{} is experiencing issues", provider.display_name())
            }
            NotificationType::SessionDepleted => {
                format!("{} session depleted. 0% left.", provider.display_name())
            }
            NotificationType::SessionRestored => {
                format!(
                    "{} session restored. Quota available again.",
                    provider.display_name()
                )
            }
        };

        self.show_toast(title, &body);

        // Play appropriate sound based on notification type
        let alert_sound = match notif_type {
            NotificationType::HighUsage => AlertSound::Warning,
            NotificationType::CriticalUsage => AlertSound::Critical,
            NotificationType::Exhausted => AlertSound::Error,
            NotificationType::StatusIssue => AlertSound::Error,
            NotificationType::SessionDepleted => AlertSound::Error,
            NotificationType::SessionRestored => AlertSound::Success,
        };
        play_alert(alert_sound, settings);
    }

    fn send_status_notification(
        &self,
        provider: ProviderId,
        description: &str,
        settings: &Settings,
    ) {
        let title = NotificationType::StatusIssue.title();
        let body = format!("{}: {}", provider.display_name(), description);
        self.show_toast(title, &body);
        play_alert(AlertSound::Error, settings);
    }

    #[cfg(target_os = "windows")]
    fn show_toast(&self, title: &str, body: &str) {
        use std::os::windows::process::CommandExt;
        use std::process::Command;

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

        // Use single-quoted here-string (@'...'@) to prevent PowerShell variable expansion
        let script = format!(
            r#"
            [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null
            [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom.XmlDocument, ContentType = WindowsRuntime] | Out-Null

            $template = @'
            <toast>
                <visual>
                    <binding template="ToastText02">
                        <text id="1">{}</text>
                        <text id="2">{}</text>
                    </binding>
                </visual>
            </toast>
'@

            $xml = New-Object Windows.Data.Xml.Dom.XmlDocument
            $xml.LoadXml($template)
            $toast = [Windows.UI.Notifications.ToastNotification]::new($xml)
            [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier("CodexBar").Show($toast)
            "#,
            safe_title, safe_body
        );

        let _ = Command::new("powershell")
            .args(["-ExecutionPolicy", "Bypass", "-Command", &script])
            .creation_flags(0x08000000) // CREATE_NO_WINDOW
            .spawn();
    }

    #[cfg(not(target_os = "windows"))]
    fn show_toast(&self, title: &str, body: &str) {
        use std::process::Command;

        // Try notify-send first (works on most Linux distros including WSL with WSLg)
        if let Ok(output) = Command::new("notify-send")
            .args(["--app-name=CodexBar", "--icon=dialog-information", title, body])
            .output()
        {
            if output.status.success() {
                tracing::debug!("Sent notification via notify-send: {}", title);
                return;
            }
        }

        // Fallback: log the notification
        tracing::info!("Notification: {} - {}", title, body);

        // In WSL, also try to use Windows toast notifications via powershell.exe
        if crate::wsl::is_wsl() {
            let script = format!(
                r#"Add-Type -AssemblyName System.Windows.Forms; \
                   [System.Windows.Forms.MessageBox]::Show('{}','{}','OK','Information')"#,
                body.replace('\'', "''"),
                title.replace('\'', "''")
            );
            let _ = Command::new("powershell.exe")
                .args(["-Command", &script])
                .spawn();
        }
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple notification function for one-off notifications
pub fn show_notification(title: &str, body: &str) {
    let manager = NotificationManager::new();
    manager.show_toast(title, body);
}
