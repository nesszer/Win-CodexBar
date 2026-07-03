//! Centralized shell behavior: surface transitions, window positioning,
//! and helpers shared across tray, shortcut, and single-instance entry points.

use std::sync::{LazyLock, Mutex};

use crate::surface::SurfaceMode;
use crate::surface_target::SurfaceTarget;

pub(crate) mod dwm;
pub mod flyout_window;
mod geometry;
mod position;
pub mod settings_window;
mod transition;
mod window;

#[cfg(test)]
mod tests;

pub(crate) use position::inferred_tray_panel_position_for_monitor_size;
#[allow(unused_imports)]
pub use position::{
    default_surface_position, inferred_tray_panel_position, remember_current_geometry_if_eligible,
    shortcut_panel_position, tray_panel_position,
};
pub use transition::{reopen_to_target, transition_to_target};
#[allow(unused_imports)]
pub use window::{
    apply_window_properties, hide_to_tray, hide_to_tray_if_current, hide_to_tray_state,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellTransitionRequest {
    pub mode: SurfaceMode,
    pub target: SurfaceTarget,
    pub position: Option<(i32, i32)>,
}

pub(super) static SHELL_TRANSITION_SERIAL: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
