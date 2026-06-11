// Future-use fields/variants — suppress until vertical slices consume them.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use codexbar::core::ProviderId;
use serde::Serialize;

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
        }
    }

    pub fn mark_blur_dismissed(&mut self, dismissed_at: std::time::Instant) {
        self.last_blur_dismissed_at = Some(dismissed_at);
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
pub type SharedAppState = Mutex<AppState>;

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
}
