use codexbar::core::ProviderId;
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::commands::ProviderUsageSnapshot;
use crate::proof_harness::ProofConfig;
use crate::surface::{SurfaceMode, SurfaceStateMachine, SurfaceTransition};
use crate::surface_target::SurfaceTarget;

/// App-update lifecycle tracking.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    Available(String),
    Downloading(f32),
    Ready,
    Error(String),
}

/// Serializable update-state payload for the frontend bridge.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatePayload {
    pub status: &'static str,
    pub version: Option<String>,
    pub error: Option<String>,
    pub progress: Option<f32>,
    pub release_url: Option<String>,
    pub can_download: bool,
    pub can_apply: bool,
    /// Unix-ms timestamp of the last completed update check (success or failure).
    /// `None` means the app has never checked for updates during this session.
    pub last_checked_at_ms: Option<i64>,
}

impl UpdateState {
    pub fn to_payload(&self) -> UpdateStatePayload {
        match self {
            Self::Idle => UpdateStatePayload {
                status: "idle",
                version: None,
                error: None,
                progress: None,
                release_url: None,
                can_download: false,
                can_apply: false,
                last_checked_at_ms: None,
            },
            Self::Checking => UpdateStatePayload {
                status: "checking",
                version: None,
                error: None,
                progress: None,
                release_url: None,
                can_download: false,
                can_apply: false,
                last_checked_at_ms: None,
            },
            Self::Available(v) => UpdateStatePayload {
                status: "available",
                version: Some(v.clone()),
                error: None,
                progress: None,
                release_url: None,
                can_download: false,
                can_apply: false,
                last_checked_at_ms: None,
            },
            Self::Downloading(p) => UpdateStatePayload {
                status: "downloading",
                version: None,
                error: None,
                progress: Some(*p),
                release_url: None,
                can_download: false,
                can_apply: false,
                last_checked_at_ms: None,
            },
            Self::Ready => UpdateStatePayload {
                status: "ready",
                version: None,
                error: None,
                progress: None,
                release_url: None,
                can_download: false,
                can_apply: false,
                last_checked_at_ms: None,
            },
            Self::Error(e) => UpdateStatePayload {
                status: "error",
                version: None,
                error: Some(e.clone()),
                progress: None,
                release_url: None,
                can_download: false,
                can_apply: false,
                last_checked_at_ms: None,
            },
        }
    }
}

/// Tray icon anchor in physical pixels, used for panel positioning.
#[derive(Debug, Clone, Copy)]
pub struct TrayAnchor {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Central app state behind `Mutex` for Tauri managed state.
///
/// Access in commands via `state: tauri::State<'_, SharedAppState>`.
pub struct AppState {
    pub surface_machine: SurfaceStateMachine,
    pub current_target: SurfaceTarget,
    pub tray_anchor: Option<TrayAnchor>,
    pub provider_cache: Vec<ProviderUsageSnapshot>,
    pub transient_provider_failure_counts: HashMap<ProviderId, u8>,
    pub provider_cache_updated_at: Option<std::time::Instant>,
    pub provider_refresh_started_at: Option<std::time::Instant>,
    pub is_refreshing: bool,
    pub update_state: UpdateState,
    /// Full update metadata from the last successful check.
    pub update_info: Option<codexbar::updater::UpdateInfo>,
    /// Unix-ms timestamp of the last completed update check.
    pub last_update_check_ms: Option<i64>,
    /// Path to a downloaded installer ready to apply.
    pub installer_path: Option<PathBuf>,
    /// Proof-harness configuration (set when `CODEXBAR_PROOF_MODE` is active).
    pub proof_config: Option<ProofConfig>,
    /// Persistent notification manager — tracks which alerts have fired to prevent spam.
    pub notification_manager: codexbar::notifications::NotificationManager,
    /// Instant when the tray panel was last shown — used to suppress
    /// spurious blur-dismiss during the show animation on Windows.
    pub last_shown_at: Option<std::time::Instant>,
    /// Instant when focus loss last dismissed the tray panel. The following
    /// tray click consumes this marker instead of reopening the panel.
    pub last_blur_dismissed_at: Option<std::time::Instant>,
    /// One-shot grace for a blur event caused while revealing the tray panel
    /// during explicit startup.
    pub startup_tray_blur_grace_until: Option<std::time::Instant>,
    /// Whether the explicit startup path may use its delayed shell fallback.
    pub startup_tray_reveal_pending: bool,
    /// Active while a user gesture (resize drag, HTML5 drag-reorder) is
    /// running a Win32 modal loop that transiently steals focus from the
    /// WebView2 child. `(began, until)` — `until` is the hard expiry;
    /// `began` lets a genuine refocus clear the guard early once the
    /// gesture's own focus flicker has settled.
    pub gesture_blur_guard: Option<(std::time::Instant, std::time::Instant)>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    pub fn resolved_target_for_mode(
        mode: SurfaceMode,
        target: Option<SurfaceTarget>,
    ) -> SurfaceTarget {
        match mode {
            SurfaceMode::Hidden => SurfaceTarget::Summary,
            _ => match target {
                Some(target) if target.mode() == mode => target,
                _ => SurfaceTarget::default_for_mode(mode),
            },
        }
    }

    pub fn new() -> Self {
        Self {
            surface_machine: SurfaceStateMachine::new(),
            current_target: SurfaceTarget::Summary,
            tray_anchor: None,
            provider_cache: Vec::new(),
            transient_provider_failure_counts: HashMap::new(),
            provider_cache_updated_at: None,
            provider_refresh_started_at: None,
            is_refreshing: false,
            update_state: UpdateState::Idle,
            update_info: None,
            last_update_check_ms: None,
            installer_path: None,
            proof_config: None,
            notification_manager: codexbar::notifications::NotificationManager::new(),
            last_shown_at: None,
            last_blur_dismissed_at: None,
            startup_tray_blur_grace_until: None,
            startup_tray_reveal_pending: false,
            gesture_blur_guard: None,
        }
    }

    pub fn mark_blur_dismissed(&mut self, dismissed_at: std::time::Instant) {
        self.last_blur_dismissed_at = Some(dismissed_at);
    }

    pub fn mark_tray_panel_shown(&mut self, shown_at: std::time::Instant) {
        self.last_shown_at = Some(shown_at);
    }

    pub fn was_tray_panel_recently_shown(
        &self,
        now: std::time::Instant,
        max_age: std::time::Duration,
    ) -> bool {
        self.last_shown_at
            .is_some_and(|shown_at| now.saturating_duration_since(shown_at) < max_age)
    }

    pub fn take_recent_blur_dismissal(
        &mut self,
        now: std::time::Instant,
        max_age: std::time::Duration,
    ) -> bool {
        self.last_blur_dismissed_at
            .take()
            .is_some_and(|dismissed_at| now.duration_since(dismissed_at) <= max_age)
    }

    #[allow(dead_code)]
    // Retained for the legacy startup TrayPanel reveal fallback.
    pub fn arm_startup_tray_reveal(&mut self, grace_until: std::time::Instant) {
        self.startup_tray_blur_grace_until = Some(grace_until);
        self.startup_tray_reveal_pending = true;
    }

    pub fn take_startup_tray_blur_grace(&mut self, now: std::time::Instant) -> bool {
        self.startup_tray_blur_grace_until
            .take()
            .is_some_and(|until| now <= until)
    }

    pub fn take_startup_tray_reveal_fallback(&mut self) -> bool {
        std::mem::take(&mut self.startup_tray_reveal_pending)
    }

    /// Arm the gesture blur guard for 15s. Called when the frontend reports
    /// a resize-grip press or a drag-reorder mousedown is about to start a
    /// Win32/OLE modal loop that will transiently blur the window.
    pub fn begin_gesture_blur_guard(&mut self, now: std::time::Instant) {
        self.gesture_blur_guard = Some((now, now + std::time::Duration::from_secs(15)));
    }

    /// Disarm the gesture blur guard immediately. Called on gesture end
    /// (mouseup / dragend) so a genuine outside click can dismiss again.
    pub fn end_gesture_blur_guard(&mut self) {
        self.gesture_blur_guard = None;
    }

    /// Whether a gesture-scoped blur guard is currently suppressing
    /// blur-dismiss.
    pub fn is_gesture_blur_guard_active(&self, now: std::time::Instant) -> bool {
        self.gesture_blur_guard
            .is_some_and(|(_, until)| now < until)
    }

    /// Clear the gesture guard on a genuine refocus. A 750ms grace from the
    /// gesture's start keeps a focus flicker at gesture kickoff from
    /// disarming the guard prematurely; a refocus after that settles the
    /// guard so a real outside-click dismiss works immediately again.
    pub fn clear_gesture_guard_on_refocus(&mut self, now: std::time::Instant) {
        if let Some((began, until)) = self.gesture_blur_guard
            && now < until
            && now.saturating_duration_since(began) >= std::time::Duration::from_millis(750)
        {
            self.gesture_blur_guard = None;
        }
    }

    pub fn transition_surface(
        &mut self,
        mode: SurfaceMode,
        target: SurfaceTarget,
    ) -> Option<SurfaceTransition> {
        self.transition_surface_internal(mode, Some(target))
    }

    pub fn hide_surface(&mut self) -> Option<SurfaceTransition> {
        self.transition_surface_internal(SurfaceMode::Hidden, Some(SurfaceTarget::Summary))
    }

    fn transition_surface_internal(
        &mut self,
        mode: SurfaceMode,
        target: Option<SurfaceTarget>,
    ) -> Option<SurfaceTransition> {
        let next_target = Self::resolved_target_for_mode(mode, target);
        let transition = self.surface_machine.transition(mode);
        self.current_target = next_target;

        transition
    }

    /// Build an enriched update payload using the stored update info.
    pub fn update_payload(&self) -> UpdateStatePayload {
        let mut p = self.update_state.to_payload();
        if let Some(ref info) = self.update_info {
            if p.version.is_none() {
                p.version = Some(info.version.clone());
            }
            p.release_url = Some(info.release_url.clone());
            p.can_download = info.supports_auto_download();
            p.can_apply = info.supports_auto_apply();
        }
        p.last_checked_at_ms = self.last_update_check_ms;
        p
    }
}

/// The type registered as Tauri managed state.
#[cfg(test)]
mod tests {
    use super::AppState;
    use crate::surface::SurfaceMode;
    use crate::surface_target::SurfaceTarget;

    #[test]
    fn transition_applies_explicit_target_on_mode_change() {
        let mut state = AppState::new();

        let transition = state.transition_surface(
            SurfaceMode::Settings,
            SurfaceTarget::Settings {
                tab: "apiKeys".into(),
            },
        );

        assert!(transition.is_some());
        assert_eq!(
            state.current_target,
            SurfaceTarget::Settings {
                tab: "apiKeys".into()
            }
        );
    }

    #[test]
    fn transition_applies_summary_target_for_tray_panel() {
        let mut state = AppState::new();

        let transition = state.transition_surface(SurfaceMode::TrayPanel, SurfaceTarget::Summary);

        assert!(transition.is_some());
        assert_eq!(state.current_target, SurfaceTarget::Summary);
    }

    #[test]
    fn transition_applies_dashboard_target_for_pop_out() {
        let mut state = AppState::new();

        let transition = state.transition_surface(SurfaceMode::PopOut, SurfaceTarget::Dashboard);

        assert!(transition.is_some());
        assert_eq!(state.current_target, SurfaceTarget::Dashboard);
    }

    #[test]
    fn same_mode_settings_retarget_updates_target() {
        let mut state = AppState::new();
        state.transition_surface(
            SurfaceMode::Settings,
            SurfaceTarget::Settings {
                tab: "apiKeys".into(),
            },
        );

        let transition = state.transition_surface(
            SurfaceMode::Settings,
            SurfaceTarget::Settings {
                tab: "about".into(),
            },
        );

        assert!(transition.is_none());
        assert_eq!(
            state.current_target,
            SurfaceTarget::Settings {
                tab: "about".into()
            }
        );
    }

    #[test]
    fn same_mode_provider_retarget_updates_target() {
        let mut state = AppState::new();
        state.transition_surface(SurfaceMode::PopOut, SurfaceTarget::Dashboard);

        let transition = state.transition_surface(
            SurfaceMode::PopOut,
            SurfaceTarget::Provider {
                provider_id: "claude".into(),
            },
        );

        assert!(transition.is_none());
        assert_eq!(
            state.current_target,
            SurfaceTarget::Provider {
                provider_id: "claude".into()
            }
        );
    }

    #[test]
    fn hidden_transition_resets_target_to_summary() {
        let mut state = AppState::new();
        state.transition_surface(
            SurfaceMode::Settings,
            SurfaceTarget::Settings {
                tab: "apiKeys".into(),
            },
        );

        let transition = state.hide_surface();

        assert!(transition.is_some());
        assert_eq!(state.current_target, SurfaceTarget::Summary);
    }

    #[test]
    fn incompatible_target_falls_back_to_mode_default() {
        let mut state = AppState::new();

        state.transition_surface(
            SurfaceMode::PopOut,
            SurfaceTarget::Settings {
                tab: "general".into(),
            },
        );

        assert_eq!(state.current_target, SurfaceTarget::Dashboard);
    }

    #[test]
    fn startup_tray_blur_grace_is_consumed_once() {
        let mut state = AppState::new();
        let now = std::time::Instant::now();

        state.arm_startup_tray_reveal(now + std::time::Duration::from_secs(1));

        assert!(state.take_startup_tray_blur_grace(now));
        assert!(!state.take_startup_tray_blur_grace(now));
    }

    #[test]
    fn startup_tray_reveal_fallback_is_consumed_once() {
        let mut state = AppState::new();
        let now = std::time::Instant::now();

        state.arm_startup_tray_reveal(now + std::time::Duration::from_secs(1));

        assert!(state.take_startup_tray_reveal_fallback());
        assert!(!state.take_startup_tray_reveal_fallback());
    }

    #[test]
    fn expired_startup_tray_blur_grace_is_consumed_without_suppressing() {
        let mut state = AppState::new();
        let now = std::time::Instant::now();

        state.arm_startup_tray_reveal(now - std::time::Duration::from_secs(1));

        assert!(!state.take_startup_tray_blur_grace(now));
        assert!(!state.take_startup_tray_blur_grace(now));
    }

    #[test]
    fn gesture_blur_guard_is_active_immediately_after_begin() {
        let mut state = AppState::new();
        let now = std::time::Instant::now();

        state.begin_gesture_blur_guard(now);

        assert!(state.is_gesture_blur_guard_active(now));
    }

    #[test]
    fn gesture_blur_guard_is_inactive_after_15_seconds() {
        let mut state = AppState::new();
        let now = std::time::Instant::now();

        state.begin_gesture_blur_guard(now);

        assert!(!state.is_gesture_blur_guard_active(
            now + std::time::Duration::from_secs(15)
        ));
        assert!(!state.is_gesture_blur_guard_active(
            now + std::time::Duration::from_secs(16)
        ));
    }

    #[test]
    fn end_gesture_blur_guard_clears_it() {
        let mut state = AppState::new();
        let now = std::time::Instant::now();

        state.begin_gesture_blur_guard(now);
        state.end_gesture_blur_guard();

        assert!(!state.is_gesture_blur_guard_active(now));
    }

    #[test]
    fn refocus_before_750ms_does_not_clear_gesture_guard() {
        let mut state = AppState::new();
        let now = std::time::Instant::now();

        state.begin_gesture_blur_guard(now);
        state.clear_gesture_guard_on_refocus(now + std::time::Duration::from_millis(200));

        assert!(state.is_gesture_blur_guard_active(now + std::time::Duration::from_millis(200)));
    }

    #[test]
    fn refocus_at_or_after_750ms_clears_gesture_guard() {
        let mut state = AppState::new();
        let now = std::time::Instant::now();

        state.begin_gesture_blur_guard(now);
        state.clear_gesture_guard_on_refocus(now + std::time::Duration::from_millis(750));

        assert!(!state.is_gesture_blur_guard_active(now + std::time::Duration::from_millis(750)));
    }
}
