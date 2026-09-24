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
pub use position::{remember_current_geometry_if_eligible, tray_panel_position};
pub use transition::{
    reopen_to_target, transition_to_target, transition_to_target_without_activation,
};
pub use window::hide_to_tray_if_current;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellTransitionRequest {
    pub mode: SurfaceMode,
    pub target: SurfaceTarget,
    pub position: Option<(i32, i32)>,
}

pub(super) static SHELL_TRANSITION_SERIAL: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
