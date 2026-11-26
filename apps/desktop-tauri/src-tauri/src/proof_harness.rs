//! Proof/debug harness for the Tauri desktop shell.
//!
//! Activated by the `CODEXBAR_PROOF_MODE` environment variable.  The value
//! specifies a target surface and optional settings tab to display on
//! startup, e.g.:
//!
//!   - `trayPanel`          — show the tray panel on the `main` window
//!   - `popOut`             — open the tray-panel flyout window, the same
//!     window tray left-click and "Pop Out Dashboard" open (the legacy
//!     PopOut layout is retired, so `popOut:<target>` payloads are rejected)
//!   - `settings`           — show settings (General tab)
//!   - `settings:menuBar`   — show settings on the Menu Bar tab
//!   - `settings:usageSpend` — show settings on the Usage & Spend tab
//!   - `settings:hooks`     — show settings on the Hooks tab
//!   - `settings:about`     — show settings on the About tab
//!   - `settings:providers:codex` — show settings on the Codex provider pane
//!
//! In proof mode the shell immediately transitions to the requested surface
//! and suppresses blur-dismiss so the window stays visible for automated
//! screenshot capture. Surfaces are shown without being activated: no
//! `set_focus`, no foreground change (see `shell::activation::suppress_all`),
//! so a proof run never takes focus from the app the user is working in.
//!
//! `CODEXBAR_SEED_USAGE_JSON=<abs-path>` additionally seeds either the legacy
//! synthetic Codex [`ProviderUsageSnapshot`] object or, in valid proof mode,
//! a nonempty array of unique supported provider snapshots. The validated set
//! is installed before the first event/WebView read and pinned against refresh
//! eviction. A seed object without `errorState` is Ready when it has no `error`
//! and Unknown otherwise; an explicit `errorState` is always kept. In proof
//! mode (`CODEXBAR_PROOF_MODE` set), an unreadable or malformed seed logs an
//! error and exits with code 2 so a broken fixture never runs as live data.
//! Outside proof mode, a malformed seed logs a warning and is skipped.
//!
//! `CODEXBAR_PROOF_WINDOW_ORIGIN=<x>,<y>` (physical pixels, proof mode only)
//! opens the flyout, Settings and float bar windows at that point instead of
//! their normal placement, so proof runs can keep every window on one chosen
//! monitor.
//!
//! `CODEXBAR_PROOF_SYSTEM_THEME=light|dark` (proof mode only) makes
//! `get_system_theme` report that Windows app theme, so the `auto` theme can be
//! proven in both directions without changing the real OS setting. Any other
//! value is ignored.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use codexbar::core::ProviderId;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::commands::{CostSnapshotBridge, ProviderUsageSnapshot, RateWindowSnapshot};
use crate::shell;
use crate::shell::activation::Activation;
use crate::state::AppState;
use crate::surface::SurfaceMode;
use crate::surface_target::{SurfaceTarget, is_supported_settings_tab};

/// Proof configuration parsed from `CODEXBAR_PROOF_MODE`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProofConfig {
    /// The surface to show on startup (serialized as the camelCase id).
    pub target_surface: String,
    /// Optional settings tab id (e.g. `"menuBar"`, `"usageSpend"`).
    pub settings_tab: Option<String>,
    /// Optional target payload for richer proof routing (currently only the
    /// settings tab id).
    pub target_payload: Option<String>,
}

impl ProofConfig {
    /// Read proof configuration from the environment.
    ///
    /// Returns `None` when `CODEXBAR_PROOF_MODE` is unset or empty.
    pub fn from_env() -> Option<Self> {
        let raw = std::env::var("CODEXBAR_PROOF_MODE").ok()?;
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }

        let (surface_str, payload) = if let Some((s, t)) = raw.split_once(':') {
            (s, Some(t.to_string()))
        } else {
            (raw, None)
        };

        let Some(surface_mode) = SurfaceMode::parse(surface_str) else {
            tracing::warn!("CODEXBAR_PROOF_MODE: unknown surface '{surface_str}', ignoring");
            return None;
        };

        if !proof_payload_is_supported(surface_mode, payload.as_deref()) {
            tracing::warn!("CODEXBAR_PROOF_MODE: unsupported target '{raw}', ignoring");
            return None;
        }

        Some(ProofConfig {
            target_surface: surface_str.to_string(),
            settings_tab: (surface_str == SurfaceMode::Settings.as_str())
                .then_some(payload.clone())
                .flatten(),
            target_payload: payload,
        })
    }

    /// Resolve the target `SurfaceMode` enum value.
    pub fn surface_mode(&self) -> SurfaceMode {
        SurfaceMode::parse(&self.target_surface).unwrap_or(SurfaceMode::TrayPanel)
    }

    pub fn surface_target(&self) -> SurfaceTarget {
        match self.surface_mode() {
            SurfaceMode::Hidden | SurfaceMode::TrayPanel => SurfaceTarget::Summary,
            SurfaceMode::PopOut => SurfaceTarget::Dashboard,
            SurfaceMode::Settings => SurfaceTarget::Settings {
                tab: self
                    .settings_tab
                    .clone()
                    .unwrap_or_else(|| "general".into()),
            },
        }
    }
}

/// Immediately transition to the proof-mode target surface.
///
/// Called from the Tauri `setup` closure when proof mode is active.
pub fn activate(app: &AppHandle) {
    let config = {
        let st = app.state::<Mutex<AppState>>();
        st.lock().unwrap().proof_config.clone()
    };

    let Some(config) = config else { return };
    let target = config.surface_mode();
    let monitors = app
        .get_webview_window("main")
        .and_then(|window| window.available_monitors().ok())
        .unwrap_or_default();
    let origin = window_origin_position(&monitors, origin_logical_size(target));
    if target == SurfaceMode::PopOut {
        tracing::info!("proof-harness: opening the tray-panel flyout window");
        // Called from the async setup task, so building the window is safe.
        // Proof automation must never take focus from the user's app.
        if let Err(err) = shell::flyout_window::open_or_focus(app, origin, Activation::Never) {
            tracing::error!("proof-harness: flyout open FAILED: {err}");
        }
        return;
    }
    let position = origin.or_else(|| match target {
        // Detached surfaces are larger than tray panels. Let their normal
        // positioning paths center/clamp them instead of reusing tray coords.
        SurfaceMode::Settings | SurfaceMode::PopOut => None,
        _ => proof_window_position(app),
    });
    tracing::info!(
        "proof-harness: activating surface={} tab={:?} position={:?}",
        config.target_surface,
        config.settings_tab,
        position,
    );

    match shell::transition_to_target(
        app,
        target,
        config.surface_target(),
        position,
        Activation::Never,
    ) {
        Ok(mode) => tracing::info!("proof-harness: transition succeeded → {mode:?}"),
        Err(err) => tracing::error!("proof-harness: transition FAILED: {err}"),
    }
}

/// Seeded snapshots must use an exact bridge provider id, not a CLI alias.
fn is_seedable_provider_id(provider_id: &str) -> bool {
    ProviderId::all()
        .iter()
        .any(|provider| provider.cli_name() == provider_id)
}

/// Bottom inset (physical px) kept between the proof panel's bottom edge and
/// the monitor work-area bottom (#265).
const PROOF_BOTTOM_INSET_PX: i32 = 8;

/// Mirror of the frontend auto-fit ceiling (`TRAY_MAX_MEASURE_HEIGHT`,
/// logical px in `useTrayPanelLayout.ts`): the proof panel can settle
/// anywhere up to this height.
const PROOF_MAX_SETTLE_HEIGHT_LOGICAL: f64 = 920.0;

/// Justified proof-panel anchor (#265). The harness anchors `main` once and
/// never re-anchors (the flyout-only `reanchor_tray_panel` path is a no-op
/// here, and Win32 ignores `set_max_size` for programmatic resizes), so an
/// anchor computed from the DEFAULT initial size lets a tall auto-fit settle
/// (up to the frontend's 920px cap) push the bottom edge under the taskbar.
/// When `anchor_y + max settle` would exceed `work_bottom - inset`, move the
/// anchor up just enough that even the tallest settle stays fully on screen;
/// otherwise keep the historical anchor unchanged.
fn proof_anchor_y(anchor_y: i32, work_bottom: i32, max_settle_height_px: i32) -> i32 {
    let justified = work_bottom - PROOF_BOTTOM_INSET_PX - max_settle_height_px;
    anchor_y.min(justified)
}

/// Logical size to keep on screen when placing `target` at the proof origin.
/// The flyout reanchors after its auto-fit settle and Settings keeps its
/// size, but the `main` tray panel is placed once and can settle up to the
/// frontend cap, so it reserves that height (#265).
fn origin_logical_size(target: SurfaceMode) -> (f64, f64) {
    match target {
        SurfaceMode::Settings => {
            let props = SurfaceMode::Settings.window_properties();
            (props.width, props.height)
        }
        SurfaceMode::PopOut => {
            let props = SurfaceMode::TrayPanel.window_properties();
            (props.width, props.height)
        }
        SurfaceMode::Hidden | SurfaceMode::TrayPanel => {
            let props = SurfaceMode::TrayPanel.window_properties();
            (
                props.width,
                props.height.max(PROOF_MAX_SETTLE_HEIGHT_LOGICAL),
            )
        }
    }
}

/// Bottom edge of a settled panel anchored via [`proof_anchor_y`].
#[cfg(test)]
fn proof_settled_bottom(
    anchor_y: i32,
    settled_height: i32,
    work_bottom: i32,
    max_settle_height_px: i32,
) -> i32 {
    proof_anchor_y(anchor_y, work_bottom, max_settle_height_px) + settled_height
}

/// Calculate a predictable window position for proof captures.
///
/// Proof mode needs a reliable on-screen position. We skip
/// `inferred_tray_panel_position` because its DPI-scaled maths can
/// produce off-screen coords on high-DPI setups.
fn proof_window_position(app: &AppHandle) -> Option<(i32, i32)> {
    let (x, y) = proof_window_position_unjustified(app)?;

    // #265: justify above the taskbar when the tallest legitimate auto-fit
    // settle (the frontend's 920px cap) would otherwise push the bottom edge
    // under it — the harness anchors `main` once and never re-anchors, so
    // the anchor itself must reserve the settle room.
    let Some(window) = app.get_webview_window("main") else {
        return Some((x, y));
    };
    let Some(m) = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.available_monitors().ok()?.into_iter().next())
    else {
        return Some((x, y));
    };
    let work_area = m.work_area();
    // Monitor physical dimensions are bounded well below i32::MAX on Windows.
    let work_bottom = work_area.position.y + work_area.size.height as i32;
    let scale = m.scale_factor().max(1.0);
    // 920px logical * scale is a whole pixel count by design.
    let max_settle = (PROOF_MAX_SETTLE_HEIGHT_LOGICAL * scale) as i32;
    let justified_y = proof_anchor_y(y, work_bottom, max_settle);
    if justified_y != y {
        tracing::info!(
            "proof-pos: #265 justified anchor y={y} → {justified_y} \
             (work_bottom={work_bottom} max_settle={max_settle})"
        );
    }
    Some((x, justified_y))
}

/// Raw proof-capture anchor, before the #265 above-taskbar justification.
fn proof_window_position_unjustified(app: &AppHandle) -> Option<(i32, i32)> {
    if let Some(pos) = shell::tray_panel_position(app) {
        return Some(pos);
    }
    let window = app.get_webview_window("main")?;
    let monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.available_monitors().ok()?.into_iter().next());
    if let Some(m) = monitor {
        // Return coordinates in Tauri physical space (same as
        // monitor.size/work_area). transition.rs divides by scale before
        // calling set_position.
        // Monitor physical dimensions are bounded well below i32::MAX.
        let screen_w = m.size().width as i32;
        let work_area = m.work_area();
        let work_bottom = work_area.position.y + work_area.size.height as i32;
        let scale = m.scale_factor().max(1.0);
        let props = SurfaceMode::TrayPanel.window_properties();
        // Layout dimensions in whole physical px by design.
        let panel_w = (props.width * scale) as i32;
        let panel_h = (props.height * scale) as i32;
        let margin = (12.0 * scale) as i32;
        let x = screen_w - panel_w - margin;
        let y = work_bottom - panel_h - margin;
        tracing::info!(
            "proof-pos: screen_w={screen_w} work_bottom={work_bottom} \
             panel={}x{} scale={scale} → ({x},{y})",
            panel_w,
            panel_h,
        );
        return Some((x, y));
    }
    Some((800, 25))
}

/// Returns `true` when proof mode is active in the shared state.
pub fn is_proof_mode(app: &AppHandle) -> bool {
    app.try_state::<Mutex<AppState>>()
        .map(|st| st.lock().unwrap().proof_config.is_some())
        .unwrap_or(false)
}

// ── Window origin (CODEXBAR_PROOF_WINDOW_ORIGIN) ─────────────────────

// ── System theme (CODEXBAR_PROOF_SYSTEM_THEME) ───────────────────────

/// Environment variable naming the app theme (`light` or `dark`) that
/// `get_system_theme` reports. Honored only in proof mode.
pub const SYSTEM_THEME_ENV_VAR: &str = "CODEXBAR_PROOF_SYSTEM_THEME";

static SYSTEM_THEME: OnceLock<&'static str> = OnceLock::new();

/// Read `CODEXBAR_PROOF_SYSTEM_THEME` once at startup. Outside proof mode,
/// or when the value is unset or invalid, nothing is installed and
/// `get_system_theme` reads the OS setting.
pub fn install_system_theme_from_env(proof_mode: bool) {
    let raw = std::env::var(SYSTEM_THEME_ENV_VAR).ok();
    if let Some(theme) = proof_system_theme(proof_mode, raw.as_deref()) {
        tracing::info!("proof-harness: system theme {theme}");
        let _ = SYSTEM_THEME.set(theme);
    }
}

/// The proof theme override, when one was installed at startup.
pub fn system_theme_override() -> Option<&'static str> {
    SYSTEM_THEME.get().copied()
}

/// Gate for `CODEXBAR_PROOF_SYSTEM_THEME`. Returns `None` outside proof mode,
/// when the value is unset or blank, and when it is not `light` or `dark`.
fn proof_system_theme(proof_mode: bool, raw: Option<&str>) -> Option<&'static str> {
    let raw = raw.map(str::trim).filter(|raw| !raw.is_empty())?;
    if !proof_mode {
        tracing::warn!("{SYSTEM_THEME_ENV_VAR} is ignored outside proof mode");
        return None;
    }
    match raw.to_ascii_lowercase().as_str() {
        "light" => Some("light"),
        "dark" => Some("dark"),
        _ => {
            tracing::warn!("{SYSTEM_THEME_ENV_VAR}: expected 'light' or 'dark', ignoring");
            None
        }
    }
}

/// Environment variable with a `<x>,<y>` physical-pixel origin for proof
/// windows. Honored only in proof mode.
pub const WINDOW_ORIGIN_ENV_VAR: &str = "CODEXBAR_PROOF_WINDOW_ORIGIN";

static WINDOW_ORIGIN: OnceLock<(i32, i32)> = OnceLock::new();

/// Read `CODEXBAR_PROOF_WINDOW_ORIGIN` once at startup. Outside proof mode,
/// or when the value is unset or malformed, no origin is installed and every
/// window keeps its normal placement.
pub fn install_window_origin_from_env(proof_mode: bool) {
    if let Some(origin) = window_origin_from_env(proof_mode) {
        tracing::info!("proof-harness: window origin {origin:?}");
        let _ = WINDOW_ORIGIN.set(origin);
    }
}

fn window_origin_from_env(proof_mode: bool) -> Option<(i32, i32)> {
    let raw = std::env::var(WINDOW_ORIGIN_ENV_VAR).ok()?;
    if raw.trim().is_empty() {
        return None;
    }
    if !proof_mode {
        tracing::warn!("{WINDOW_ORIGIN_ENV_VAR} is ignored outside proof mode");
        return None;
    }
    match parse_window_origin(&raw) {
        Ok(origin) => Some(origin),
        Err(msg) => {
            tracing::warn!("{WINDOW_ORIGIN_ENV_VAR}: {msg}, ignoring");
            None
        }
    }
}

/// Parse `<x>,<y>` as signed physical pixels, e.g. `-3600,-1188`.
fn parse_window_origin(raw: &str) -> Result<(i32, i32), String> {
    let (x, y) = raw
        .split_once(',')
        .ok_or_else(|| format!("expected '<x>,<y>', got '{raw}'"))?;
    let parse = |part: &str| {
        part.trim()
            .parse::<i32>()
            .map_err(|_| format!("'{}' is not a whole pixel coordinate", part.trim()))
    };
    Ok((parse(x)?, parse(y)?))
}

/// The proof window origin, when one was installed at startup.
pub fn window_origin() -> Option<(i32, i32)> {
    WINDOW_ORIGIN.get().copied()
}

/// Top-left (physical px) for a proof window of `logical_size` at the
/// installed origin: on the monitor whose full bounds contain the origin,
/// clamped so the window stays inside that monitor's work area. `None` when
/// no origin is installed or no monitor contains it.
pub fn window_origin_position(
    monitors: &[tauri::Monitor],
    logical_size: (f64, f64),
) -> Option<(i32, i32)> {
    let origin = window_origin()?;
    let monitors = monitors
        .iter()
        .map(|monitor| {
            let work = monitor.work_area();
            OriginMonitor {
                bounds: crate::window_positioner::Rect::new(
                    monitor.position().x,
                    monitor.position().y,
                    monitor.size().width,
                    monitor.size().height,
                ),
                work_area: crate::window_positioner::Rect::new(
                    work.position.x,
                    work.position.y,
                    work.size.width,
                    work.size.height,
                ),
                scale_factor: monitor.scale_factor(),
            }
        })
        .collect::<Vec<_>>();
    let position = origin_on_monitor(origin, &monitors, logical_size);
    if position.is_none() {
        tracing::warn!(
            "{WINDOW_ORIGIN_ENV_VAR}: no monitor contains {origin:?}; using normal placement"
        );
    }
    position
}

#[derive(Debug, Clone, Copy)]
struct OriginMonitor {
    bounds: crate::window_positioner::Rect,
    work_area: crate::window_positioner::Rect,
    scale_factor: f64,
}

fn origin_on_monitor(
    origin: (i32, i32),
    monitors: &[OriginMonitor],
    logical_size: (f64, f64),
) -> Option<(i32, i32)> {
    let (x, y) = origin;
    let monitor = monitors.iter().find(|m| {
        x >= m.bounds.x && x < m.bounds.right() && y >= m.bounds.y && y < m.bounds.bottom()
    })?;
    // Window sizes are small positive logical constants or measured sizes.
    let size = crate::window_positioner::PanelSize {
        width: logical_size.0.round().max(1.0) as u32,
        height: logical_size.1.round().max(1.0) as u32,
    };
    Some(crate::window_positioner::clamp_position_to_work_area(
        x,
        y,
        &monitor.work_area,
        &size,
        monitor.scale_factor,
    ))
}

/// Synthetic account rows are available only through a proof-mode command.
pub fn seed_claude_accounts_from_env() -> Option<Vec<crate::commands::ClaudeAccountState>> {
    let path = std::env::var_os("CODEXBAR_SEED_CLAUDE_ACCOUNTS_JSON")?;
    let result = std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok());
    if result.is_none() {
        tracing::warn!("Invalid Claude account proof seed; using real account metadata");
    }
    result
}

// ── Provider-usage seed (CODEXBAR_SEED_USAGE_JSON) ───────────────────

/// Environment variable pointing at a JSON file with one synthetic,
/// bridge-shaped Codex snapshot or a proof-only array of provider snapshots.
pub const SEED_USAGE_ENV_VAR: &str = "CODEXBAR_SEED_USAGE_JSON";

/// Whether proof mode is requested, from the raw `CODEXBAR_PROOF_MODE` value.
/// A set but invalid value still counts, so a broken proof run cannot quietly
/// fall back to live data.
fn proof_mode_requested() -> bool {
    std::env::var("CODEXBAR_PROOF_MODE").is_ok_and(|raw| !raw.trim().is_empty())
}

/// Read and validate the seed file referenced by `CODEXBAR_SEED_USAGE_JSON`.
///
/// Outside proof mode, an unreadable or malformed seed logs a warning and
/// returns `None`. In proof mode (`CODEXBAR_PROOF_MODE` set) it logs an error
/// and exits the process with code 2, so a broken fixture never runs as live
/// data.
///
/// Seed lanes that carry `resetsAt` but no `pace` get the pace the bridge
/// would compute at launch, using `pace_work_days`
/// (`Settings::weekly_progress_work_days`), so proof runs exercise the live
/// pace math instead of a hand-written result.
pub fn seed_usage_snapshots_from_env(
    proof_config: Option<&ProofConfig>,
    pace_work_days: Option<u8>,
) -> Option<Vec<ProviderUsageSnapshot>> {
    let path = std::env::var_os(SEED_USAGE_ENV_VAR)?;
    let path = std::path::PathBuf::from(path);
    let source = std::fs::read_to_string(&path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()));
    match seed_usage_from_source(
        source,
        &path.display().to_string(),
        proof_config,
        proof_mode_requested(),
        pace_work_days,
        chrono::Utc::now(),
    ) {
        Ok(snapshots) => snapshots,
        Err(msg) => {
            let line =
                format!("{SEED_USAGE_ENV_VAR}: {msg}; proof mode cannot run without a valid seed");
            tracing::error!("{line}");
            eprintln!("{line}");
            std::process::exit(2);
        }
    }
}

/// Validate a seed source and return its snapshots, or the failure reason.
///
/// `Ok(None)` means a malformed seed was skipped with a warning (outside proof
/// mode). `Err` means proof mode is active, so the seed must stop the run.
/// The caller supplies the source text, the clock and the proof state.
fn seed_usage_from_source(
    source: Result<String, String>,
    label: &str,
    proof_config: Option<&ProofConfig>,
    proof_active: bool,
    pace_work_days: Option<u8>,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<Option<Vec<ProviderUsageSnapshot>>, String> {
    let parsed = source.and_then(|raw| {
        parse_seed_usage_snapshots(&raw, proof_config).map_err(|msg| format!("{msg} in {label}"))
    });
    match parsed {
        Ok(mut snapshots) => {
            for snapshot in &mut snapshots {
                fill_seed_pace(snapshot, now, pace_work_days);
            }
            Ok(Some(snapshots))
        }
        Err(msg) if proof_active => Err(msg),
        Err(msg) => {
            tracing::warn!("{SEED_USAGE_ENV_VAR}: {msg}");
            Ok(None)
        }
    }
}

/// Parse a seed-usage JSON string directly into a canonical
/// [`ProviderUsageSnapshot`].
///
/// The canonical bridge types carry `#[serde(default)]` on optional/derived
/// fields so the seed JSON can omit them. This function fills in the
/// computed defaults that serde cannot express from sibling fields
/// (`remainingPercent` from `usedPercent`, `formattedUsed` from `used`,
/// `updatedAt` from the current time) and validates that the snapshot is
/// for the `codex` provider.
///
/// Pure: no env vars, no files, no global state.
pub fn parse_seed_usage_snapshot(json: &str) -> Result<ProviderUsageSnapshot, String> {
    let mut snapshot: ProviderUsageSnapshot =
        serde_json::from_str(json).map_err(|e| format!("malformed JSON: {e}"))?;
    let raw: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("malformed JSON: {e}"))?;

    if snapshot.provider_id != "codex" {
        return Err(format!(
            "snapshot providerId '{}' is not 'codex', ignoring",
            snapshot.provider_id
        ));
    }

    normalize_seed_snapshot(&mut snapshot, error_state_omitted(&raw));

    Ok(snapshot)
}

/// Parse legacy Codex objects unchanged, or a provider array when proof mode
/// has already been validated. Arrays are all-or-nothing and use canonical
/// provider IDs emitted by the bridge.
pub fn parse_seed_usage_snapshots(
    json: &str,
    proof_config: Option<&ProofConfig>,
) -> Result<Vec<ProviderUsageSnapshot>, String> {
    if !json.trim_start().starts_with('[') {
        return parse_seed_usage_snapshot(json).map(|snapshot| vec![snapshot]);
    }
    if !proof_config.is_some_and(is_valid_proof_config) {
        return Err("provider snapshot arrays require valid proof mode".into());
    }

    let raw_values: Vec<serde_json::Value> =
        serde_json::from_str(json).map_err(|e| format!("malformed JSON: {e}"))?;
    let mut snapshots: Vec<ProviderUsageSnapshot> =
        serde_json::from_str(json).map_err(|e| format!("malformed JSON: {e}"))?;
    if snapshots.is_empty() {
        return Err("provider snapshot array must not be empty".into());
    }

    let mut providers = HashSet::with_capacity(snapshots.len());
    for (snapshot, raw) in snapshots.iter_mut().zip(&raw_values) {
        if !is_seedable_provider_id(&snapshot.provider_id) {
            return Err(format!(
                "unsupported snapshot providerId '{}', ignoring",
                snapshot.provider_id
            ));
        }
        if !providers.insert(snapshot.provider_id.clone()) {
            return Err(format!(
                "duplicate snapshot providerId '{}', ignoring",
                snapshot.provider_id
            ));
        }
        normalize_seed_snapshot(snapshot, error_state_omitted(raw));
    }
    Ok(snapshots)
}

/// True when the raw seed object omits `errorState`. The bridge default
/// (`Unknown`) would then mark every pill as an error, so the seed infers it.
fn error_state_omitted(raw: &serde_json::Value) -> bool {
    raw.get("errorState").is_none()
}

fn normalize_seed_snapshot(snapshot: &mut ProviderUsageSnapshot, error_state_omitted: bool) {
    if error_state_omitted {
        snapshot.error_state = if snapshot.error.is_some() {
            codexbar::core::ProviderStateKind::Unknown
        } else {
            codexbar::core::ProviderStateKind::Ready
        };
    }
    normalize_rate_window(&mut snapshot.primary);
    snapshot.secondary.as_mut().map(normalize_rate_window);
    snapshot.model_specific.as_mut().map(normalize_rate_window);
    snapshot.tertiary.as_mut().map(normalize_rate_window);
    for extra in &mut snapshot.extra_rate_windows {
        normalize_rate_window(&mut extra.window);
    }
    snapshot.cost.as_mut().map(normalize_cost);

    if snapshot.updated_at.is_empty() {
        snapshot.updated_at = chrono::Utc::now().to_rfc3339();
    }
}

/// Compute lane pace for seeded windows that omit it, with the same fallback
/// window lengths and reset-window capability as
/// `ProviderUsageSnapshot::from_fetch_result`.
fn fill_seed_pace(
    snapshot: &mut ProviderUsageSnapshot,
    now: chrono::DateTime<chrono::Utc>,
    work_days: Option<u8>,
) {
    use crate::commands::pace::{PaceLane, SESSION_FALLBACK_MINUTES, WEEKLY_FALLBACK_MINUTES};
    let id = ProviderId::from_cli_name(&snapshot.provider_id);
    fill_window_pace(
        &mut snapshot.primary,
        (id, PaceLane::Card),
        SESSION_FALLBACK_MINUTES,
        now,
        work_days,
    );
    for window in [
        snapshot.secondary.as_mut(),
        snapshot.model_specific.as_mut(),
        snapshot.tertiary.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        fill_window_pace(
            window,
            (id, PaceLane::Card),
            WEEKLY_FALLBACK_MINUTES,
            now,
            work_days,
        );
    }
    for extra in &mut snapshot.extra_rate_windows {
        fill_window_pace(
            &mut extra.window,
            (id, PaceLane::Extra),
            WEEKLY_FALLBACK_MINUTES,
            now,
            work_days,
        );
    }
}

fn fill_window_pace(
    w: &mut RateWindowSnapshot,
    (id, pace_lane): (Option<ProviderId>, crate::commands::pace::PaceLane),
    default_minutes: u32,
    now: chrono::DateTime<chrono::Utc>,
    work_days: Option<u8>,
) {
    if (w.pace.is_some() && w.daily_target.is_some()) || w.is_informational {
        return;
    }
    let Some(resets_at) = w
        .resets_at
        .as_deref()
        .and_then(|raw| chrono::DateTime::parse_from_rfc3339(raw).ok())
    else {
        return;
    };
    let mut window = codexbar::core::RateWindow::with_details(
        w.used_percent,
        w.window_minutes,
        Some(resets_at.with_timezone(&chrono::Utc)),
        w.reset_description.clone(),
    );
    window.reset_regenerates = w.reset_regenerates;
    if w.daily_target.is_none() {
        w.daily_target = crate::commands::pace::daily_target(id, &window, now);
    }
    if w.pace.is_some() {
        return;
    }
    w.pace = match id {
        Some(id) => crate::commands::pace::WindowPaceSnapshot::for_lane(
            id,
            pace_lane,
            &window,
            default_minutes,
            now,
            work_days,
        ),
        None => crate::commands::pace::WindowPaceSnapshot::for_window(
            &window,
            default_minutes,
            now,
            work_days,
        ),
    };
}

fn is_valid_proof_config(config: &ProofConfig) -> bool {
    SurfaceMode::parse(&config.target_surface)
        .is_some_and(|mode| proof_payload_is_supported(mode, config.target_payload.as_deref()))
}

/// Recompute `remaining_percent` from `used_percent` (matching the canonical
/// [`RateWindowSnapshot::from_rate_window`] behaviour) so the seed JSON can
/// omit it.
fn normalize_rate_window(w: &mut RateWindowSnapshot) {
    w.remaining_percent = 100.0 - w.used_percent.clamp(0.0, 100.0);
}

/// Fill `formatted_used` from `used` when the seed JSON omits it.
fn normalize_cost(c: &mut CostSnapshotBridge) {
    if c.formatted_used.is_empty() {
        c.formatted_used = format!("${:.2}", c.used);
    }
}

fn proof_payload_is_supported(surface_mode: SurfaceMode, payload: Option<&str>) -> bool {
    match (surface_mode, payload) {
        (SurfaceMode::Hidden | SurfaceMode::TrayPanel, None) => true,
        (SurfaceMode::Hidden | SurfaceMode::TrayPanel, Some(_)) => false,
        (SurfaceMode::Settings, None) => true,
        (SurfaceMode::Settings, Some(tab)) => is_supported_settings_tab(tab),
        (SurfaceMode::PopOut, None) => true,
        (SurfaceMode::PopOut, Some(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LazyLock;

    static ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn tall_panel_bottom_edge_never_passes_work_bottom_minus_inset() {
        // 1920x1080 proof rig at 100% display scale: historical anchor y=252
        // (1040 work bottom - 776 default height - 12 margin); the tallest
        // legitimate settle is the frontend auto-fit cap, 920px.
        let max_settle = 920;
        // y is justified up to 112 so the capped settle bottoms out exactly
        // on the inset boundary...
        assert_eq!(proof_anchor_y(252, 1040, max_settle), 112);
        let bottom = proof_settled_bottom(252, max_settle, 1040, max_settle);
        assert!(bottom <= 1040 - PROOF_BOTTOM_INSET_PX);
        assert_eq!(bottom, 1032);
        // ...and any shorter settle keeps its bottom edge on screen too.
        assert!(proof_settled_bottom(252, 780, 1040, max_settle) <= 1040 - PROOF_BOTTOM_INSET_PX);
        assert!(proof_settled_bottom(252, 568, 1040, max_settle) <= 1040 - PROOF_BOTTOM_INSET_PX);
        // Tall displays where even the capped settle already fits keep the
        // historical anchor unchanged.
        assert_eq!(proof_anchor_y(252, 2000, max_settle), 252);
        // Degenerate anchors below the work bottom move fully above it.
        assert_eq!(proof_anchor_y(1500, 1040, max_settle), 112);
    }

    fn with_proof_mode_env(value: Option<&str>, test: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("CODEXBAR_PROOF_MODE").ok();

        match value {
            // SAFETY: `ENV_LOCK` is held for the whole `with_proof_mode_env`
            // scope, serializing this env mutation against other proof-mode
            // tests so no other thread reads or writes `CODEXBAR_PROOF_MODE`.
            Some(value) => unsafe { std::env::set_var("CODEXBAR_PROOF_MODE", value) },
            // SAFETY: same `ENV_LOCK` guard; the variable is unset only here.
            None => unsafe { std::env::remove_var("CODEXBAR_PROOF_MODE") },
        }

        test();

        match prev {
            // SAFETY: `ENV_LOCK` is still held, restoring the prior value so
            // no other proof-mode test observes the temporary mutation.
            Some(prev) => unsafe { std::env::set_var("CODEXBAR_PROOF_MODE", prev) },
            // SAFETY: same `ENV_LOCK` guard; restores the unset state.
            None => unsafe { std::env::remove_var("CODEXBAR_PROOF_MODE") },
        }
    }

    #[test]
    fn parse_simple_surface() {
        with_proof_mode_env(Some("trayPanel"), || {
            let cfg = ProofConfig::from_env().unwrap();
            assert_eq!(cfg.target_surface, "trayPanel");
            assert!(cfg.settings_tab.is_none());
            assert_eq!(cfg.surface_mode(), SurfaceMode::TrayPanel);
        });
    }

    #[test]
    fn parse_settings_with_tab() {
        with_proof_mode_env(Some("settings:menuBar"), || {
            let cfg = ProofConfig::from_env().unwrap();
            assert_eq!(cfg.target_surface, "settings");
            assert_eq!(cfg.settings_tab.as_deref(), Some("menuBar"));
            assert_eq!(cfg.surface_mode(), SurfaceMode::Settings);
            assert_eq!(
                cfg.surface_target(),
                SurfaceTarget::Settings {
                    tab: "menuBar".into()
                }
            );
        });
    }

    #[test]
    fn parse_settings_about_proof_target() {
        with_proof_mode_env(Some("settings:about"), || {
            let cfg = ProofConfig::from_env().unwrap();
            assert_eq!(cfg.target_surface, "settings");
            assert_eq!(cfg.settings_tab.as_deref(), Some("about"));
        });
    }

    #[test]
    fn parse_settings_provider_pane_proof_target() {
        with_proof_mode_env(Some("settings:providers:codex"), || {
            let cfg = ProofConfig::from_env().unwrap();
            assert_eq!(cfg.settings_tab.as_deref(), Some("providers:codex"));
            assert_eq!(
                cfg.surface_target(),
                SurfaceTarget::Settings {
                    tab: "providers:codex".into()
                }
            );
        });
        with_proof_mode_env(Some("settings:providers:nope"), || {
            assert!(ProofConfig::from_env().is_none());
        });
    }

    #[test]
    fn retired_popout_provider_proof_targets_are_rejected() {
        // The legacy PopOut layout (with provider deep links) is retired;
        // `popOut` only opens the tray-panel flyout.
        for raw in ["popOut:provider:codex", "popOut:dashboard"] {
            with_proof_mode_env(Some(raw), || {
                assert!(ProofConfig::from_env().is_none(), "{raw}");
            });
        }
    }

    #[test]
    fn empty_env_returns_none() {
        with_proof_mode_env(Some(""), || {
            assert!(ProofConfig::from_env().is_none());
        });
    }

    #[test]
    fn unset_env_returns_none() {
        with_proof_mode_env(None, || {
            assert!(ProofConfig::from_env().is_none());
        });
    }

    #[test]
    fn invalid_surface_returns_none() {
        with_proof_mode_env(Some("bogus"), || {
            assert!(ProofConfig::from_env().is_none());
        });
    }

    #[test]
    fn invalid_settings_tab_returns_none() {
        with_proof_mode_env(Some("settings:security"), || {
            assert!(ProofConfig::from_env().is_none());
        });
    }

    #[test]
    fn invalid_provider_target_returns_none() {
        with_proof_mode_env(Some("popOut:provider:not-a-provider"), || {
            assert!(ProofConfig::from_env().is_none());
        });
    }

    #[test]
    fn pop_out_surface() {
        with_proof_mode_env(Some("popOut"), || {
            let cfg = ProofConfig::from_env().unwrap();
            assert_eq!(cfg.surface_mode(), SurfaceMode::PopOut);
            assert_eq!(cfg.surface_target(), SurfaceTarget::Dashboard);
        });
    }

    #[test]
    fn parse_seed_snapshot_decodes_codex_usage() {
        let json = serde_json::json!({
            "providerId": "codex",
            "displayName": "Codex",
            "primary": {
                "usedPercent": 61.0,
                "windowMinutes": 300,
                "resetsAt": "2099-01-02T03:04:05Z",
                "resetDescription": "seeded 5h",
            },
            "primaryLabel": "Session",
            "secondary": { "usedPercent": 74.0, "windowMinutes": 10080 },
            "secondaryLabel": "Weekly",
            "extraRateWindows": [
                {
                    "id": "reset-credits",
                    "title": "Reset Credits",
                    "window": {
                        "usedPercent": 0.0,
                        "resetsAt": "2099-01-09T00:00:00Z",
                        "resetDescription": "2 reset credits available",
                        "isInformational": true,
                    },
                },
            ],
        })
        .to_string();

        let snapshot = parse_seed_usage_snapshot(&json).expect("seed parses into a snapshot");
        assert_eq!(snapshot.provider_id, "codex");
        assert_eq!(snapshot.primary.used_percent, 61.0);
        assert_eq!(
            snapshot.primary.resets_at.as_deref(),
            Some("2099-01-02T03:04:05Z")
        );
        assert_eq!(snapshot.primary.remaining_percent, 39.0);
        assert_eq!(snapshot.primary_label.as_deref(), Some("Session"));
        assert_eq!(
            snapshot.secondary.as_ref().map(|s| s.used_percent),
            Some(74.0)
        );
        assert_eq!(
            snapshot.secondary.as_ref().map(|s| s.remaining_percent),
            Some(26.0)
        );
        assert_eq!(snapshot.extra_rate_windows.len(), 1);
        assert_eq!(snapshot.extra_rate_windows[0].id, "reset-credits");
        assert!(snapshot.extra_rate_windows[0].window.is_informational);
        // Defaults applied: displayName, sourceLabel, updatedAt
        assert_eq!(snapshot.display_name, "Codex");
        assert_eq!(snapshot.source_label, "seed");
        assert!(!snapshot.updated_at.is_empty());
    }

    #[test]
    fn parse_seed_snapshot_rejects_non_codex_provider() {
        let json = r#"{"providerId": "claude", "primary": {"usedPercent": 10.0}}"#;
        assert!(parse_seed_usage_snapshot(json).is_err());
    }

    #[test]
    fn parse_seed_snapshot_rejects_malformed_json() {
        assert!(parse_seed_usage_snapshot("{not json}").is_err());
    }

    #[test]
    fn parse_seed_snapshot_fills_cost_defaults() {
        let json = r#"{
            "providerId": "codex",
            "primary": {"usedPercent": 50.0},
            "cost": {"used": 12.5, "limit": 100.0}
        }"#;
        let snapshot = parse_seed_usage_snapshot(json).unwrap();
        let cost = snapshot.cost.expect("cost present");
        assert_eq!(cost.used, 12.5);
        assert_eq!(cost.limit, Some(100.0));
        assert_eq!(cost.currency_code, "USD");
        assert_eq!(cost.period, "month");
        assert_eq!(cost.formatted_used, "$12.50");
    }

    #[test]
    fn seed_pace_is_computed_from_reset_time_and_work_days() {
        use chrono::TimeZone;
        // Fri 2026-06-12 12:00 UTC, weekly reset two days later, 50% used.
        let now = chrono::Utc.with_ymd_and_hms(2026, 6, 12, 12, 0, 0).unwrap();
        let json = r#"{"providerId":"codex","displayName":"Codex",
            "primary":{"usedPercent":10.0,"windowMinutes":300},
            "secondary":{"usedPercent":50.0,"windowMinutes":10080,
                         "resetsAt":"2026-06-14T12:00:00Z"}}"#;

        let mut linear = parse_seed_usage_snapshot(json).unwrap();
        fill_seed_pace(&mut linear, now, None);
        let linear_pace = linear.secondary.unwrap().pace.unwrap();
        assert!((linear_pace.expected_used_percent - 500.0 / 7.0).abs() < 1e-9);

        let mut workdays = parse_seed_usage_snapshot(json).unwrap();
        fill_seed_pace(&mut workdays, now, Some(5));
        let workday_pace = workdays.secondary.unwrap().pace.unwrap();
        assert!(workday_pace.expected_used_percent > linear_pace.expected_used_percent + 5.0);

        // No resetsAt: no pace, as before.
        assert!(linear.primary.pace.is_none());
    }

    #[test]
    fn seed_pace_uses_the_providers_reset_window_capability() {
        use chrono::TimeZone;
        // Copilot: no window length, monthly reset on Mar 1, half of Feb gone.
        let now = chrono::Utc.with_ymd_and_hms(2026, 2, 15, 0, 0, 0).unwrap();
        let json = r#"{"providerId":"copilot","displayName":"Copilot",
            "primary":{"usedPercent":60.0,"resetsAt":"2026-03-01T00:00:00Z"},
            "extraRateWindows":[{"id":"copilot-seat","title":"Seat",
                "window":{"usedPercent":60.0,"resetsAt":"2026-03-01T00:00:00Z"}}]}"#;
        // Non-Codex seeds arrive in a proof-mode array; parse the element directly.
        let mut snapshot: ProviderUsageSnapshot = serde_json::from_str(json).unwrap();
        normalize_seed_snapshot(&mut snapshot, false);
        fill_seed_pace(&mut snapshot, now, None);
        let pace = snapshot.primary.pace.expect("copilot primary pace");
        assert_eq!(pace.expected_used_percent, 50.0);
        assert_eq!(pace.delta_percent, 10.0);
        assert_eq!(pace.eta_seconds, Some(806_400.0));
        assert!(pace.reset_window);
        // Extras keep plain lane pace: no length means no pace.
        assert!(snapshot.extra_rate_windows[0].window.pace.is_none());
    }

    #[test]
    fn seed_daily_target_skips_regenerating_windows() {
        use chrono::TimeZone;
        let now = chrono::Utc.with_ymd_and_hms(2026, 2, 15, 0, 0, 0).unwrap();
        let seed = |regenerates: bool| {
            let json = format!(
                r#"{{"providerId":"copilot","displayName":"Copilot",
                "primary":{{"usedPercent":60.0,"resetsAt":"2026-03-01T00:00:00Z",
                            "resetRegenerates":{regenerates}}}}}"#
            );
            let mut snapshot: ProviderUsageSnapshot = serde_json::from_str(&json).unwrap();
            normalize_seed_snapshot(&mut snapshot, false);
            fill_seed_pace(&mut snapshot, now, None);
            snapshot.primary.daily_target
        };
        assert!(seed(false).is_some());
        assert_eq!(seed(true), None);
    }

    #[test]
    fn seed_pace_keeps_explicit_pace() {
        let json = r#"{"providerId":"codex","displayName":"Codex",
            "primary":{"usedPercent":10.0,"windowMinutes":10080,
                       "resetsAt":"2026-06-14T12:00:00Z",
                       "pace":{"stage":"on_track","deltaPercent":0.0,
                               "expectedUsedPercent":10.0,"actualUsedPercent":10.0}}}"#;
        let mut snapshot = parse_seed_usage_snapshot(json).unwrap();
        let before = snapshot.primary.pace.clone();
        fill_seed_pace(&mut snapshot, chrono::Utc::now(), Some(5));
        assert_eq!(snapshot.primary.pace, before);
    }

    fn valid_proof_config() -> ProofConfig {
        ProofConfig {
            target_surface: "trayPanel".into(),
            settings_tab: None,
            target_payload: None,
        }
    }

    fn seed_snapshot(provider_id: &str, used_percent: f64) -> serde_json::Value {
        serde_json::json!({
            "providerId": provider_id,
            "primary": { "usedPercent": used_percent, "windowMinutes": 300 }
        })
    }

    #[test]
    fn seed_snapshot_arrays_preserve_legacy_codex_object_behavior() {
        let json = seed_snapshot("codex", 61.0).to_string();
        let snapshots = parse_seed_usage_snapshots(&json, None).expect("legacy seed parses");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].provider_id, "codex");
        assert_eq!(snapshots[0].primary.remaining_percent, 39.0);
    }

    #[test]
    fn seed_snapshot_array_normalizes_multiple_supported_providers() {
        let json = serde_json::json!([seed_snapshot("codex", 61.0), seed_snapshot("claude", 24.0)])
            .to_string();
        let snapshots = parse_seed_usage_snapshots(&json, Some(&valid_proof_config()))
            .expect("supported snapshots parse in proof mode");

        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].provider_id, "codex");
        assert_eq!(snapshots[0].primary.remaining_percent, 39.0);
        assert_eq!(snapshots[1].provider_id, "claude");
        assert_eq!(snapshots[1].primary.remaining_percent, 76.0);
        assert!(
            snapshots
                .iter()
                .all(|snapshot| !snapshot.updated_at.is_empty())
        );
    }

    #[test]
    fn seed_snapshot_arrays_reject_empty_unknown_duplicate_and_non_finite_values() {
        let proof = valid_proof_config();
        for json in [
            "[]".to_string(),
            serde_json::json!([seed_snapshot("unknown-provider", 1.0)]).to_string(),
            // CLI aliases are not bridge provider ids.
            serde_json::json!([seed_snapshot("openai", 1.0)]).to_string(),
            serde_json::json!([seed_snapshot("codex", 1.0), seed_snapshot("codex", 2.0)])
                .to_string(),
            r#"[{"providerId":"codex","primary":{"usedPercent":1e400}}]"#.to_string(),
        ] {
            assert!(
                parse_seed_usage_snapshots(&json, Some(&proof)).is_err(),
                "{json}"
            );
        }
    }

    #[test]
    fn window_origin_parses_signed_physical_coordinates() {
        assert_eq!(parse_window_origin("100,200"), Ok((100, 200)));
        assert_eq!(parse_window_origin("-3600,-1188"), Ok((-3600, -1188)));
        assert_eq!(parse_window_origin(" -3600 , 25 "), Ok((-3600, 25)));
    }

    #[test]
    fn window_origin_rejects_malformed_values() {
        for raw in ["", "100", "100;200", "a,b", "1.5,2", "1,2,3", ",5"] {
            assert!(parse_window_origin(raw).is_err(), "{raw}");
        }
    }

    fn with_window_origin_env(value: Option<&str>, test: impl FnOnce()) {
        let _guard = ENV_LOCK.lock().unwrap();
        let prev = std::env::var(WINDOW_ORIGIN_ENV_VAR).ok();
        match value {
            // SAFETY: `ENV_LOCK` serializes every env mutation in these tests.
            Some(value) => unsafe { std::env::set_var(WINDOW_ORIGIN_ENV_VAR, value) },
            // SAFETY: same `ENV_LOCK` guard.
            None => unsafe { std::env::remove_var(WINDOW_ORIGIN_ENV_VAR) },
        }
        test();
        match prev {
            // SAFETY: `ENV_LOCK` is still held while the prior value returns.
            Some(prev) => unsafe { std::env::set_var(WINDOW_ORIGIN_ENV_VAR, prev) },
            // SAFETY: same `ENV_LOCK` guard.
            None => unsafe { std::env::remove_var(WINDOW_ORIGIN_ENV_VAR) },
        }
    }

    #[test]
    fn system_theme_is_honored_only_in_proof_mode() {
        assert_eq!(proof_system_theme(true, Some("light")), Some("light"));
        assert_eq!(proof_system_theme(true, Some(" Dark ")), Some("dark"));
        assert_eq!(proof_system_theme(false, Some("light")), None);
        assert_eq!(proof_system_theme(false, Some("dark")), None);
    }

    #[test]
    fn system_theme_ignores_unset_blank_and_invalid_values() {
        assert_eq!(proof_system_theme(true, None), None);
        assert_eq!(proof_system_theme(true, Some("")), None);
        assert_eq!(proof_system_theme(true, Some("   ")), None);
        for raw in ["auto", "system", "lightish", "1"] {
            assert_eq!(proof_system_theme(true, Some(raw)), None, "{raw}");
        }
    }

    #[test]
    fn window_origin_env_is_honored_only_in_proof_mode() {
        with_window_origin_env(Some("-3600,-1188"), || {
            assert_eq!(window_origin_from_env(true), Some((-3600, -1188)));
            assert_eq!(window_origin_from_env(false), None);
        });
        with_window_origin_env(Some("left,top"), || {
            assert_eq!(window_origin_from_env(true), None);
        });
        with_window_origin_env(None, || {
            assert_eq!(window_origin_from_env(true), None);
        });
    }

    fn display(x: i32, y: i32, width: u32, height: u32, scale: f64) -> OriginMonitor {
        let rect = crate::window_positioner::Rect::new(x, y, width, height);
        OriginMonitor {
            bounds: rect,
            work_area: rect,
            scale_factor: scale,
        }
    }

    #[test]
    fn window_origin_lands_on_the_monitor_that_contains_it() {
        let primary = OriginMonitor {
            work_area: crate::window_positioner::Rect::new(0, 0, 1920, 1032),
            ..display(0, 0, 1920, 1080, 1.0)
        };
        let secondary = display(-3600, -1188, 3600, 2400, 2.0);
        let monitors = [primary, secondary];

        // Inside the secondary display: kept 8 px inside its work area.
        assert_eq!(
            origin_on_monitor((-3600, -1188), &monitors, (310.0, 776.0)),
            Some((-3592, -1180))
        );
        assert_eq!(
            origin_on_monitor((-3000, -1000), &monitors, (310.0, 776.0)),
            Some((-3000, -1000))
        );
        // Near the bottom-right edge: pulled back so the 2x window fits.
        assert_eq!(
            origin_on_monitor((-10, 1200), &monitors, (310.0, 776.0)),
            Some((-628, 1212 - 1552 - 8))
        );
        // Off every monitor: no override.
        assert_eq!(
            origin_on_monitor((5000, 5000), &monitors, (310.0, 776.0)),
            None
        );
    }

    #[test]
    fn main_tray_panel_origin_reserves_the_settled_height() {
        let tray = SurfaceMode::TrayPanel.window_properties();
        let settings = SurfaceMode::Settings.window_properties();
        assert_eq!(
            origin_logical_size(SurfaceMode::TrayPanel),
            (tray.width, PROOF_MAX_SETTLE_HEIGHT_LOGICAL)
        );
        assert_eq!(
            origin_logical_size(SurfaceMode::PopOut),
            (tray.width, tray.height)
        );
        assert_eq!(
            origin_logical_size(SurfaceMode::Settings),
            (settings.width, settings.height)
        );

        // Near the work-area bottom of a 1080p display, the tallest settle
        // still ends above the taskbar.
        let work = crate::window_positioner::Rect::new(0, 0, 1920, 1032);
        let monitors = [OriginMonitor {
            work_area: work,
            ..display(0, 0, 1920, 1080, 1.0)
        }];
        let (_, y) = origin_on_monitor(
            (100, 1000),
            &monitors,
            origin_logical_size(SurfaceMode::TrayPanel),
        )
        .unwrap();
        assert!(y + PROOF_MAX_SETTLE_HEIGHT_LOGICAL as i32 <= work.bottom() - 8);
    }

    fn seed_error_state(json: &str) -> codexbar::core::ProviderStateKind {
        parse_seed_usage_snapshot(json).unwrap().error_state
    }

    #[test]
    fn omitted_seed_error_state_defaults_to_ready() {
        let json = r#"{"providerId":"codex","primary":{"usedPercent":10.0,"windowMinutes":300}}"#;
        assert_eq!(
            seed_error_state(json),
            codexbar::core::ProviderStateKind::Ready
        );
    }

    #[test]
    fn omitted_seed_error_state_with_error_is_unknown() {
        let json = r#"{"providerId":"codex","primary":{"usedPercent":10.0,"windowMinutes":300},"error":"boom"}"#;
        assert_eq!(
            seed_error_state(json),
            codexbar::core::ProviderStateKind::Unknown
        );
    }

    #[test]
    fn explicit_seed_error_state_is_kept() {
        let expired = r#"{"providerId":"codex","primary":{"usedPercent":10.0,"windowMinutes":300},"errorState":"expiredSession"}"#;
        assert_eq!(
            seed_error_state(expired),
            codexbar::core::ProviderStateKind::ExpiredSession
        );
        let unknown = r#"{"providerId":"codex","primary":{"usedPercent":10.0,"windowMinutes":300},"errorState":"unknown"}"#;
        assert_eq!(
            seed_error_state(unknown),
            codexbar::core::ProviderStateKind::Unknown
        );
    }

    #[test]
    fn omitted_seed_error_state_defaults_to_ready_in_arrays() {
        let json = serde_json::json!([seed_snapshot("codex", 10.0)]).to_string();
        let snapshots = parse_seed_usage_snapshots(&json, Some(&valid_proof_config())).unwrap();
        assert_eq!(
            snapshots[0].error_state,
            codexbar::core::ProviderStateKind::Ready
        );
    }

    #[test]
    fn malformed_seed_fails_only_in_proof_mode() {
        let now = chrono::Utc::now();
        let config = valid_proof_config();
        let strict = seed_usage_from_source(
            Ok("{not json".into()),
            "seed.json",
            Some(&config),
            true,
            None,
            now,
        );
        assert!(strict.is_err());
        let lenient =
            seed_usage_from_source(Ok("{not json".into()), "seed.json", None, false, None, now);
        assert!(matches!(lenient, Ok(None)));
    }

    #[test]
    fn unreadable_seed_fails_only_in_proof_mode() {
        let now = chrono::Utc::now();
        let missing = || Err("cannot read seed.json: not found".to_string());
        assert!(seed_usage_from_source(missing(), "seed.json", None, true, None, now).is_err());
        assert!(matches!(
            seed_usage_from_source(missing(), "seed.json", None, false, None, now),
            Ok(None)
        ));
    }

    #[test]
    fn seed_snapshot_arrays_are_rejected_without_valid_proof_config() {
        let json = serde_json::json!([seed_snapshot("codex", 10.0)]).to_string();
        assert!(parse_seed_usage_snapshots(&json, None).is_err());
    }
}
