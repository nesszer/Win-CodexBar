// Placeholder emitters for vertical slices — suppress dead-code until wired.
#![allow(dead_code)]

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::commands::ProviderUsageSnapshot;
use crate::proof_harness::ProofStatePayload;
use crate::state::UpdateStatePayload;
use crate::surface::SurfaceMode;
use crate::surface_target::SurfaceTarget;

// ── Event name constants ─────────────────────────────────────────────

pub const SURFACE_MODE_CHANGED: &str = "surface-mode-changed";
pub const PROVIDER_UPDATED: &str = "provider-updated";
pub const REFRESH_STARTED: &str = "refresh-started";
pub const REFRESH_COMPLETE: &str = "refresh-complete";
pub const UPDATE_STATE_CHANGED: &str = "update-state-changed";
pub const LOGIN_PHASE_CHANGED: &str = "login-phase-changed";
pub const PROOF_STATE_CHANGED: &str = "proof-state-changed";
pub const LOCALE_CHANGED: &str = "locale-changed";
pub const SETTINGS_CHANGED: &str = "settings-changed";

// ── Payloads ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceModePayload {
    pub mode: &'static str,
    pub previous: &'static str,
    pub target: SurfaceTarget,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshCompletePayload {
    pub provider_count: usize,
    pub error_count: usize,
}

// ── Emit helpers ─────────────────────────────────────────────────────

pub fn emit_surface_mode_changed(
    app: &AppHandle,
    from: SurfaceMode,
    to: SurfaceMode,
    target: SurfaceTarget,
) {
    let _ = app.emit(
        SURFACE_MODE_CHANGED,
        SurfaceModePayload {
            mode: to.as_str(),
            previous: from.as_str(),
            target,
        },
    );
}

pub fn emit_provider_updated(app: &AppHandle, snapshot: &ProviderUsageSnapshot) {
    let _ = app.emit(PROVIDER_UPDATED, snapshot);
}

pub fn emit_refresh_started(app: &AppHandle) {
    let _ = app.emit(REFRESH_STARTED, ());
}

pub fn emit_refresh_complete(app: &AppHandle, provider_count: usize, error_count: usize) {
    let _ = app.emit(
        REFRESH_COMPLETE,
        RefreshCompletePayload {
            provider_count,
            error_count,
        },
    );
}

pub fn emit_update_state_changed(app: &AppHandle, payload: &UpdateStatePayload) {
    let _ = app.emit(UPDATE_STATE_CHANGED, payload);
}

pub fn emit_login_phase_changed(app: &AppHandle) {
    let _ = app.emit(LOGIN_PHASE_CHANGED, ());
}

pub fn emit_proof_state_changed(app: &AppHandle, payload: &ProofStatePayload) {
    let _ = app.emit(PROOF_STATE_CHANGED, payload);
}

/// Broadcast to every window that persisted settings changed, so surfaces in
/// other windows (e.g. the PopOut dashboard) re-read settings and re-render —
/// the detached Settings window and the main window are separate webviews and
/// do not share React state. Payload-less; listeners re-fetch the snapshot.
pub fn emit_settings_changed(app: &AppHandle) {
    let _ = app.emit(SETTINGS_CHANGED, ());
}
